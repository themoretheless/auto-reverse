use core_foundation::base::TCFType;
use core_foundation::mach_port::CFMachPortRef;
use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes};
use core_graphics::event::{
    CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType, CallbackResult,
};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock};
use std::time::Duration;

use crate::config::AppConfig;
use crate::device_attribution::assess_wheel_attribution;
use crate::device_classifier::ContinuousSourceHint;
use crate::device_source::HidSourceClass;
use crate::error::{AppError, AppResult};
use crate::recovery_audit::{RecoveryAction, RecoveryReason};
use crate::runtime::RuntimeControl;
use crate::scroll::TransformDecision;

use super::{daemon_lock, gesture, hid, recovery_log, scroll_events};

#[cfg(feature = "gui")]
use super::debug_log;
#[cfg(feature = "gui")]
use crate::device::DeviceKind;
#[cfg(feature = "gui")]
use crate::device_attribution::AttributionStatus;
#[cfg(feature = "gui")]
use crate::device_classifier::ClassificationEvidence;
#[cfg(feature = "gui")]
use crate::input_policy::{InputBypassReason, evaluate_input_policy};

/// How recently a HID wheel tick must have arrived for a discrete CGEvent
/// to be attributed to that device. Both callbacks share one run loop
/// thread, so in practice the HID value lands immediately before the tap
/// event; the window only needs to absorb run-loop scheduling jitter.
const WHEEL_OBSERVATION_WINDOW: Duration = Duration::from_millis(500);

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventTapIsEnabled(tap: CFMachPortRef) -> bool;
}

static CONFIG: OnceLock<Arc<RwLock<AppConfig>>> = OnceLock::new();
static RUNTIME_CONTROL: OnceLock<Arc<RuntimeControl>> = OnceLock::new();
static TAP_PORTS: Mutex<TapPorts> = Mutex::new(TapPorts {
    active: None,
    gesture: None,
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TapPorts {
    active: Option<usize>,
    gesture: Option<usize>,
}

/// Keeps the raw CFMachPort pointer registered only while the owning
/// `CGEventTap` value is alive. The mutex serializes wake-thread rearming with
/// run-loop teardown so `CGEventTapEnable` can never observe a freed port.
struct TapPortRegistration {
    ports: TapPorts,
}

impl TapPortRegistration {
    fn new(active: usize, gesture: Option<usize>) -> AppResult<Self> {
        let mut registered = tap_port_guard();
        if registered.active.is_some() || registered.gesture.is_some() {
            return Err(AppError::Platform(
                "an event tap port is already registered in this process".to_string(),
            ));
        }
        let ports = TapPorts {
            active: Some(active),
            gesture,
        };
        *registered = ports;
        Ok(Self { ports })
    }
}

impl Drop for TapPortRegistration {
    fn drop(&mut self) {
        let mut registered = tap_port_guard();
        if *registered == self.ports {
            *registered = TapPorts {
                active: None,
                gesture: None,
            };
        }
    }
}

fn tap_port_guard() -> MutexGuard<'static, TapPorts> {
    match TAP_PORTS.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapEnabledState {
    NotInstalled,
    Enabled,
    Disabled,
}

/// Reads CoreGraphics' public enabled state while holding the same lifetime
/// guard used by re-arming, so the queried CFMachPort cannot be freed midway.
pub fn enabled_state() -> TapEnabledState {
    let registered = tap_port_guard();
    let Some(active) = registered.active else {
        return TapEnabledState::NotInstalled;
    };
    if unsafe { CGEventTapIsEnabled(active as CFMachPortRef) } {
        TapEnabledState::Enabled
    } else {
        TapEnabledState::Disabled
    }
}

/// Re-enables the currently registered tap, if its owning run loop is still
/// alive. Holding the registry lock across the CoreGraphics call prevents a
/// concurrent run-loop return from dropping the backing `CGEventTap` first.
pub fn rearm_if_installed() -> bool {
    let registered = tap_port_guard();
    let Some(active) = registered.active else {
        return false;
    };
    unsafe { CGEventTapEnable(active as CFMachPortRef, true) };
    if let Some(gesture) = registered.gesture {
        unsafe { CGEventTapEnable(gesture as CFMachPortRef, true) };
    }
    true
}

/// Why the blocking event-tap runner returned successfully.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapRunOutcome {
    /// Another process owns `run.lock`; no tap was installed here.
    AlreadyRunning,
    /// This process installed a tap, but its CFRunLoop later stopped.
    Stopped,
}

/// Installs a system-wide event tap that applies the configured direction to
/// mouse, trackpad, and Magic Mouse scrolling, then blocks running the current
/// thread's run loop forever. Returns an error if macOS refused to create the
/// active tap, which is almost always missing Accessibility permission.
/// Failure of the optional passive gesture tap is
/// non-fatal and falls back to conservative continuous-source behavior.
///
/// `config` is shared (`Arc<RwLock<_>>`) rather than owned outright so a
/// caller running this on a background thread (the merged GUI process) can
/// keep writing config changes from another thread - e.g. the settings
/// window - and have the very next scroll event observe them, without
/// restarting the tap. A standalone `run` process can just as well pass in
/// an `Arc` it never shares with anyone else; the behavior is identical.
///
/// Acquires the exclusive `daemon_lock` as the first thing it does, before
/// creating the tap - this is the one gate that prevents two live
/// `CGEventTap`s regardless of which of the two launch paths (headless
/// `run`, or the merged GUI process's in-process thread) got there first.
/// If the lock is already held, this returns
/// `Ok(TapRunOutcome::AlreadyRunning)` without installing a tap.
pub fn install_and_run(config: Arc<RwLock<AppConfig>>) -> AppResult<TapRunOutcome> {
    install_and_run_with_ready(config, Arc::new(RuntimeControl::default()), || {})
}

/// Same runner as [`install_and_run`], with a one-shot callback fired after
/// the tap is enabled and immediately before entering the blocking run loop.
/// The GUI uses this explicit handshake instead of inferring startup from a
/// timeout; the headless path uses [`install_and_run`] with a no-op callback.
pub fn install_and_run_with_ready(
    config: Arc<RwLock<AppConfig>>,
    runtime_control: Arc<RuntimeControl>,
    on_ready: impl FnOnce(),
) -> AppResult<TapRunOutcome> {
    let Some(_daemon_lock) = daemon_lock::try_acquire(&daemon_lock::default_path())? else {
        println!("auto-reverse: another instance is already running; exiting");
        return Ok(TapRunOutcome::AlreadyRunning);
    };

    let event_tap = CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::Default,
        vec![CGEventType::ScrollWheel],
        handle_event,
    )
    .map_err(|_| {
        AppError::Platform(
            "failed to install scroll event tap; Accessibility may be missing".to_string(),
        )
    })?;
    let loop_source = event_tap
        .mach_port()
        .create_runloop_source(0)
        .map_err(|_| {
            AppError::Platform("failed to create event tap run-loop source".to_string())
        })?;

    if let Some(existing) = CONFIG.get() {
        if !Arc::ptr_eq(existing, &config) {
            return Err(AppError::Platform(
                "event tap config was already initialized by another runtime".to_string(),
            ));
        }
    } else {
        CONFIG
            .set(Arc::clone(&config))
            .map_err(|_| AppError::Platform("event tap config initialization raced".to_string()))?;
    }
    if let Some(existing) = RUNTIME_CONTROL.get() {
        if !Arc::ptr_eq(existing, &runtime_control) {
            return Err(AppError::Platform(
                "event tap runtime control was already initialized by another runtime".to_string(),
            ));
        }
    } else {
        RUNTIME_CONTROL.set(runtime_control).map_err(|_| {
            AppError::Platform("event tap runtime control initialization raced".to_string())
        })?;
    }

    // Always start the HID monitor, even when config currently has no rules:
    // the merged UI can add its first rule live after startup, and the tap
    // thread has no safe way to schedule IOHIDManager later from the GUI
    // thread. Starting here keeps that first rule immediately effective.
    // This still happens after every fallible tap/source step so an early TCC
    // failure cannot leave a process-lifetime manager on an exiting thread.
    let device_rule_count = match config.read() {
        Ok(guard) => guard.device_rules.len(),
        Err(poisoned) => poisoned.into_inner().device_rules.len(),
    };
    match hid::start_wheel_monitor() {
        Ok(()) if device_rule_count == 0 => {
            println!("auto-reverse: HID wheel monitor started; no device_rules configured yet")
        }
        Ok(()) => println!(
            "auto-reverse: HID wheel monitor started for {device_rule_count} device rule(s)"
        ),
        Err(error) => eprintln!(
            "auto-reverse: HID wheel monitor failed ({error}); current and newly-added \
             device_rules will fall back to per-kind flags until restart"
        ),
    }

    let continuous_source_hint = match hid::live_continuous_source_hint()
        .map(Ok)
        .unwrap_or_else(hid::continuous_source_hint)
    {
        Ok(hint) => {
            println!(
                "auto-reverse: connected continuous devices: {}",
                hint.description()
            );
            hint
        }
        Err(error) => {
            eprintln!(
                "auto-reverse: continuous-device inventory failed ({error}); unknown input \
                 falls back to trackpad"
            );
            ContinuousSourceHint::Unknown
        }
    };
    let gesture_monitor = match gesture::GestureMonitor::start(continuous_source_hint) {
        Ok(monitor) => {
            println!(
                "auto-reverse: public gesture monitor started; Magic Mouse/trackpad heuristic enabled"
            );
            Some(monitor)
        }
        Err(error) => {
            eprintln!(
                "auto-reverse: gesture monitor failed ({error}); continuous scrolling falls back to trackpad"
            );
            None
        }
    };

    let _tap_port_registration = TapPortRegistration::new(
        event_tap.mach_port().as_concrete_TypeRef() as usize,
        gesture_monitor.as_ref().map(gesture::GestureMonitor::port),
    )?;
    CFRunLoop::get_current().add_source(&loop_source, unsafe { kCFRunLoopCommonModes });
    event_tap.enable();
    on_ready();
    CFRunLoop::run_current();
    Ok(TapRunOutcome::Stopped)
}

fn handle_event(
    _proxy: CGEventTapProxy,
    event_type: CGEventType,
    event: &CGEvent,
) -> CallbackResult {
    match event_type {
        CGEventType::TapDisabledByTimeout => {
            recover_disabled_tap(RecoveryReason::TapTimeout);
        }
        CGEventType::TapDisabledByUserInput => {
            recover_disabled_tap(RecoveryReason::TapUserInput);
        }
        CGEventType::ScrollWheel => {
            let Some(config_lock) = CONFIG.get() else {
                return CallbackResult::Keep;
            };

            let continuous = !scroll_events::is_physical_mouse_wheel(event);
            let classification =
                gesture::classify_scroll(event, continuous, hid::live_continuous_source_hint());
            let device_kind = classification.kind;
            // Attribute only genuine hardware wheel ticks: discrete
            // (continuous scrolling never produces HID wheel values) AND
            // originating from the HID system (source_pid == 0). An event
            // some other process injected did not come from a real device,
            // so pinning it to whatever mouse scrolled last would be wrong -
            // it could inherit that device's rule purely by wall-clock luck.
            let source_pid = scroll_events::event_source_pid(event);
            let from_hid = source_pid == 0;
            let wheel_snapshot = if continuous || !from_hid {
                None
            } else {
                hid::recent_wheel_snapshot(WHEEL_OBSERVATION_WINDOW)
            };
            let attribution_status = assess_wheel_attribution(
                continuous,
                source_pid,
                wheel_snapshot.as_ref().map(|snapshot| snapshot.age),
            );
            let accepted_snapshot = wheel_snapshot
                .as_ref()
                .filter(|_| attribution_status.accepts_identity());
            let identity = accepted_snapshot.and_then(|snapshot| snapshot.identity.clone());
            let hid_source = accepted_snapshot
                .map(|snapshot| {
                    HidSourceClass::from_observed_transport(snapshot.transport.as_deref())
                })
                .unwrap_or(HidSourceClass::NotObserved);
            #[cfg(feature = "gui")]
            let device_name = accepted_snapshot.and_then(|snapshot| snapshot.name.clone());

            // Hot path: hold the read lock only long enough to clone the
            // config, then drop it before touching the CGEvent fields. A
            // writer (the settings window, on every checkbox change) only
            // ever needs a brief exclusive window between reads, so this
            // never blocks the GUI thread noticeably, and this thread never
            // holds the lock across the actual field-write syscalls below.
            let config = {
                let guard = match config_lock.read() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                guard.clone()
            };
            let temporarily_paused = RUNTIME_CONTROL
                .get()
                .is_some_and(|control| control.is_paused());
            #[cfg_attr(not(feature = "gui"), allow(unused_variables))]
            let decision = if temporarily_paused {
                TransformDecision::passthrough(scroll_events::event_from_cg_event(
                    event,
                    device_kind,
                    identity,
                    hid_source,
                ))
            } else {
                scroll_events::apply_config_in_place(
                    event,
                    &config,
                    device_kind,
                    identity,
                    hid_source,
                )
            };

            #[cfg(feature = "gui")]
            record_debug_event(
                &config,
                device_kind,
                device_name,
                attribution_status,
                classification.evidence,
                temporarily_paused,
                decision,
            );
        }
        _ => {}
    }
    CallbackResult::Keep
}

fn recover_disabled_tap(reason: RecoveryReason) {
    // CGEventTapEnable takes the CFMachPortRef returned by CGEventTapCreate.
    // The callback proxy is a different opaque token and crashes here on
    // pointer-authenticated macOS.
    let action = if rearm_if_installed() {
        RecoveryAction::Rearmed
    } else {
        RecoveryAction::Failed
    };
    recovery_log::record_attempt(reason, action);
}

/// Builds a `debug_log::DebugEvent` from the same context
/// `handle_event`/`apply_config_in_place` already computed and pushes it to
/// the ring buffer. Does not recompute or duplicate any part of
/// `scroll::transform_event`'s policy - `decision` is exactly what that
/// function already returned; the "why" behind Ignored/Passed is derived
/// from the same `config`/`device_kind`/identity values already in scope
/// in `handle_event`, using the same public accessors (`config.enabled`,
/// `config.should_reverse`, `config.reverse_only_raw_input`) the pure policy
/// itself is built on, not a second reimplementation of it.
#[cfg(feature = "gui")]
fn record_debug_event(
    config: &AppConfig,
    device_kind: DeviceKind,
    device_name: Option<Arc<str>>,
    attribution_status: AttributionStatus,
    classification_evidence: ClassificationEvidence,
    temporarily_paused: bool,
    decision: TransformDecision,
) {
    let timestamp_ms = debug_log::now_millis();
    let monotonic_us = debug_log::now_monotonic_micros();
    let identity = decision.original.identity.as_deref();
    let hardware = identity.map(|value| value.hardware);
    let continuous = decision.original.continuous;
    let input_policy = evaluate_input_policy(
        decision.original.synthetic,
        decision.original.hid_source,
        decision.original.source_pid,
        config.reverse_only_raw_input,
    );
    let profile = config.resolve_device_profile(device_kind, identity);

    // Two rows (vertical/horizontal) whenever the event carries a nonzero
    // delta on that axis - mirrors the handoff's per-axis rows exactly.
    let axes: &[(debug_log::Axis, i64, i64)] = &[
        (
            debug_log::Axis::Vertical,
            decision.original.delta_vertical,
            decision.transformed.delta_vertical,
        ),
        (
            debug_log::Axis::Horizontal,
            decision.original.delta_horizontal,
            decision.transformed.delta_horizontal,
        ),
    ];

    for &(axis, raw, out) in axes {
        if raw == 0 && out == 0 {
            continue;
        }

        let axis_reversed = match axis {
            debug_log::Axis::Vertical => decision.vertical_reversed,
            debug_log::Axis::Horizontal => decision.horizontal_reversed,
        };

        let reason = decision_reason(
            config,
            &decision.original,
            temporarily_paused,
            axis_reversed,
        );

        debug_log::push(debug_log::DebugEvent {
            timestamp_ms,
            monotonic_us,
            device_kind,
            device_name: device_name.clone(),
            identity: decision.original.identity.clone(),
            hardware,
            attribution_status,
            classification_evidence,
            input_provenance: input_policy.provenance,
            hid_source: decision.original.hid_source,
            profile,
            source_pid: decision.original.source_pid,
            synthetic: decision.original.synthetic,
            continuous,
            axis,
            raw_delta: raw,
            output_delta: out,
            reason,
        });
    }
}

/// Pure derivation of one Debug Console row's stable reason for a single axis.
/// Split out of `record_debug_event` so branch order remains unit-testable
/// without a live CGEventTap or IOHIDManager. User-facing wording belongs to
/// the diagnostics presentation layer, not this callback path.
#[cfg(feature = "gui")]
fn decision_reason(
    config: &AppConfig,
    event: &crate::input::ScrollEvent,
    temporarily_paused: bool,
    axis_reversed: bool,
) -> debug_log::DecisionReason {
    let device_kind = event.device_kind;
    let identity = event.identity.as_deref();
    if !config.enabled {
        return debug_log::DecisionReason::ScrollReversalOff;
    }
    if temporarily_paused {
        return debug_log::DecisionReason::TemporarilyPaused;
    }
    let input_policy = evaluate_input_policy(
        event.synthetic,
        event.hid_source,
        event.source_pid,
        config.reverse_only_raw_input,
    );
    if let Some(reason) = input_policy.bypass {
        return match reason {
            InputBypassReason::SelfSynthetic => debug_log::DecisionReason::SyntheticEvent,
            InputBypassReason::VirtualHid => debug_log::DecisionReason::VirtualHidSource,
            InputBypassReason::UnknownHid => debug_log::DecisionReason::UnknownHidSource,
            InputBypassReason::PostedInputGuard => debug_log::DecisionReason::RawInputGuard,
        };
    }
    if axis_reversed {
        let has_reverse_rule = config
            .resolve_device_profile(device_kind, identity)
            .reverse
            .source
            .is_device_rule();
        return if has_reverse_rule {
            debug_log::DecisionReason::DeviceRuleReversed
        } else {
            debug_log::DecisionReason::Reversed
        };
    }

    if !config.should_reverse(device_kind, identity) {
        // Only reachable when reversal is genuinely off for this
        // device/kind (no rule, or an explicit Don't-reverse rule) -
        // checked before, not after, the device_kind == Trackpad case
        // below. Checking device_kind first (as an earlier version of this
        // function did) mislabeled ANY unreversed trackpad axis as
        // "trackpad natural" even when reverse_trackpad was genuinely on
        // and should_reverse() was true - e.g. a horizontal trackpad axis
        // with reverse_horizontal off would falsely read "trackpad
        // natural" instead of naming the real reason (that direction is
        // off), contradicting a "Reversed" row from the same gesture's
        // vertical axis. See
        // trackpad_off_by_default_reads_natural_only_when_should_reverse_is_false.
        let has_dont_reverse_rule = config
            .resolve_device_profile(device_kind, identity)
            .reverse
            .source
            .is_device_rule();
        return if device_kind == DeviceKind::Unknown {
            debug_log::DecisionReason::UnknownDeviceNotReversed
        } else if has_dont_reverse_rule {
            debug_log::DecisionReason::DeviceRuleDisabled
        } else if device_kind == DeviceKind::Trackpad {
            // Trackpad-off-by-default is the normal, expected state (not
            // an error) - matches the design handoff's own example
            // wording/category for this exact case.
            debug_log::DecisionReason::TrackpadNatural
        } else {
            debug_log::DecisionReason::DeviceReversalOff
        };
    }

    // should_reverse(device_kind, identity) is true here, so this axis's
    // own direction flag (reverse_vertical/reverse_horizontal) is what's
    // off - a real, deliberate "not this direction" outcome, not an error.
    debug_log::DecisionReason::AxisDisabled
}

#[cfg(all(test, feature = "gui"))]
mod debug_log_decision_tests {
    use super::*;
    use crate::device::DeviceIdentity;

    fn config_with(mutate: impl FnOnce(&mut AppConfig)) -> AppConfig {
        let mut config = AppConfig {
            enabled: true,
            ..Default::default()
        };
        mutate(&mut config);
        config
    }

    fn event(device_kind: DeviceKind) -> crate::input::ScrollEvent {
        crate::input::ScrollEvent::new(device_kind, 1, 0, false)
    }

    #[test]
    fn trackpad_off_by_default_reads_natural_only_when_should_reverse_is_false() {
        // The actual regression this test locks in: reverse_trackpad is ON
        // (should_reverse(Trackpad) == true) but reverse_horizontal is OFF,
        // so the horizontal axis of a trackpad gesture is not reversed for
        // an ordinary policy reason, not because trackpads are naturally
        // exempt. A prior version of this function checked
        // `device_kind == Trackpad` before `should_reverse` and mislabeled
        // this "Passed through – trackpad natural", implying trackpad
        // reversal was off/inherent when it was genuinely enabled.
        let config = config_with(|c| {
            c.reverse_trackpad = true;
            c.reverse_horizontal = false;
        });
        assert!(config.should_reverse(DeviceKind::Trackpad, None));

        let reason = decision_reason(&config, &event(DeviceKind::Trackpad), false, false);

        assert_eq!(reason, debug_log::DecisionReason::AxisDisabled);
    }

    #[test]
    fn trackpad_natural_wording_applies_when_should_reverse_is_genuinely_false() {
        let config = config_with(|c| c.reverse_trackpad = false);
        assert!(!config.should_reverse(DeviceKind::Trackpad, None));

        let reason = decision_reason(&config, &event(DeviceKind::Trackpad), false, false);

        assert_eq!(reason, debug_log::DecisionReason::TrackpadNatural);
    }

    #[test]
    fn explicit_dont_reverse_rule_names_itself_over_the_generic_reason() {
        use crate::config::DeviceRule;
        use crate::device::HardwareId;

        let identity = DeviceIdentity::hardware_only(HardwareId {
            vendor_id: 0x1234,
            product_id: 0x5678,
        });
        let config = config_with(|c| {
            c.reverse_mouse = true;
            c.device_rules
                .push(DeviceRule::for_hardware(identity.hardware, None, false));
        });

        let event = crate::input::ScrollEvent {
            identity: Some(Arc::new(identity)),
            ..event(DeviceKind::Mouse)
        };
        let reason = decision_reason(&config, &event, false, false);

        assert_eq!(reason, debug_log::DecisionReason::DeviceRuleDisabled);
    }

    #[test]
    fn explicit_reverse_rule_marks_identity_context_for_trace_replay() {
        use crate::config::DeviceRule;
        use crate::device::HardwareId;

        let identity = DeviceIdentity::hardware_only(HardwareId {
            vendor_id: 0x1234,
            product_id: 0x5678,
        });
        let config = config_with(|c| {
            c.reverse_mouse = false;
            c.device_rules
                .push(DeviceRule::for_hardware(identity.hardware, None, true));
        });

        let event = crate::input::ScrollEvent {
            identity: Some(Arc::new(identity)),
            ..event(DeviceKind::Mouse)
        };
        let reason = decision_reason(&config, &event, false, true);

        assert_eq!(reason, debug_log::DecisionReason::DeviceRuleReversed);
    }

    #[test]
    fn unknown_device_kind_names_itself() {
        let config = config_with(|_| {});
        let reason = decision_reason(&config, &event(DeviceKind::Unknown), false, false);
        assert_eq!(reason, debug_log::DecisionReason::UnknownDeviceNotReversed);
    }

    #[test]
    fn mouse_reversal_off_names_the_device_kind() {
        let config = config_with(|c| c.reverse_mouse = false);
        let reason = decision_reason(&config, &event(DeviceKind::Mouse), false, false);
        assert_eq!(reason, debug_log::DecisionReason::DeviceReversalOff);
    }

    #[test]
    fn axis_reversed_wins_over_every_other_check() {
        let config = config_with(|_| {});
        let reason = decision_reason(&config, &event(DeviceKind::Mouse), false, true);
        assert_eq!(reason, debug_log::DecisionReason::Reversed);
    }

    #[test]
    fn disabled_config_is_ignored_before_anything_else() {
        let config = config_with(|c| c.enabled = false);
        let reason = decision_reason(&config, &event(DeviceKind::Mouse), false, true);
        assert_eq!(reason, debug_log::DecisionReason::ScrollReversalOff);
    }

    #[test]
    fn synthetic_event_is_ignored_even_when_axis_reversed_would_otherwise_apply() {
        let config = config_with(|_| {});
        let event = crate::input::ScrollEvent {
            synthetic: true,
            ..event(DeviceKind::Mouse)
        };
        let reason = decision_reason(&config, &event, false, true);
        assert_eq!(reason, debug_log::DecisionReason::SyntheticEvent);
    }

    #[test]
    fn virtual_and_unknown_hid_sources_have_explicit_fail_open_reasons() {
        let config = config_with(|_| {});

        for (source, expected) in [
            (
                HidSourceClass::Virtual,
                debug_log::DecisionReason::VirtualHidSource,
            ),
            (
                HidSourceClass::Unknown,
                debug_log::DecisionReason::UnknownHidSource,
            ),
        ] {
            let event = crate::input::ScrollEvent {
                hid_source: source,
                ..event(DeviceKind::Mouse)
            };
            let reason = decision_reason(&config, &event, false, true);
            assert_eq!(reason, expected);
        }
    }

    #[test]
    fn synthetic_reason_precedes_hid_source_reason() {
        let config = config_with(|_| {});
        let event = crate::input::ScrollEvent {
            synthetic: true,
            hid_source: HidSourceClass::Virtual,
            ..event(DeviceKind::Mouse)
        };
        let reason = decision_reason(&config, &event, false, true);

        assert_eq!(reason, debug_log::DecisionReason::SyntheticEvent);
    }

    #[test]
    fn raw_input_guard_is_ignored_even_when_axis_reversed_would_otherwise_apply() {
        let config = config_with(|c| c.reverse_only_raw_input = true);
        let non_zero_source_pid = 4242;
        let event = crate::input::ScrollEvent {
            source_pid: non_zero_source_pid,
            ..event(DeviceKind::Mouse)
        };
        let reason = decision_reason(&config, &event, false, true);
        assert_eq!(reason, debug_log::DecisionReason::RawInputGuard);
    }

    #[test]
    fn temporary_pause_has_an_explicit_debug_reason() {
        let config = config_with(|_| {});
        let reason = decision_reason(&config, &event(DeviceKind::Mouse), true, false);

        assert_eq!(reason, debug_log::DecisionReason::TemporarilyPaused);
    }
}

#[cfg(test)]
mod tap_port_tests {
    use super::*;

    #[test]
    fn registration_rejects_overlap_and_clears_on_drop() {
        assert_eq!(
            *tap_port_guard(),
            TapPorts {
                active: None,
                gesture: None
            }
        );

        let registration =
            TapPortRegistration::new(0x1234, Some(0x5678)).expect("first registration");
        assert_eq!(
            *tap_port_guard(),
            TapPorts {
                active: Some(0x1234),
                gesture: Some(0x5678)
            }
        );
        assert!(TapPortRegistration::new(0x9abc, None).is_err());

        drop(registration);
        assert_eq!(
            *tap_port_guard(),
            TapPorts {
                active: None,
                gesture: None
            }
        );
    }
}
