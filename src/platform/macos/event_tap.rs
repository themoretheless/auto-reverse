use core_foundation::base::TCFType;
use core_foundation::mach_port::CFMachPortRef;
use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes};
use core_graphics::event::{
    CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType, CallbackResult,
};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use crate::config::AppConfig;
use crate::device::conservative_kind_from_continuity;
use crate::error::{AppError, AppResult};

use super::{daemon_lock, hid, scroll_events};

#[cfg(feature = "gui")]
use super::debug_log;
#[cfg(feature = "gui")]
use crate::device::{DeviceKind, HardwareId};
#[cfg(feature = "gui")]
use crate::scroll::TransformDecision;

/// How recently a HID wheel tick must have arrived for a discrete CGEvent
/// to be attributed to that device. Both callbacks share one run loop
/// thread, so in practice the HID value lands immediately before the tap
/// event; the window only needs to absorb run-loop scheduling jitter.
const WHEEL_ATTRIBUTION_WINDOW: Duration = Duration::from_millis(500);

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
}

static CONFIG: OnceLock<Arc<RwLock<AppConfig>>> = OnceLock::new();
static TAP_PORT: OnceLock<usize> = OnceLock::new();

/// Installs a system-wide event tap that reverses scroll direction for
/// physical mouse wheels, then blocks running the current thread's run loop
/// forever. Returns `Err(())` if macOS refused to create the tap, which is
/// almost always a missing Input Monitoring / Accessibility permission.
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
/// If the lock is already held, this returns `Ok(())` without installing a
/// tap, exactly like the standalone `run` command already did.
pub fn install_and_run(config: Arc<RwLock<AppConfig>>) -> AppResult<()> {
    let Some(_daemon_lock) = daemon_lock::try_acquire(&daemon_lock::default_path())? else {
        println!("auto-reverse: another instance is already running; exiting");
        return Ok(());
    };

    // Per-device rules need the HID wheel monitor; without rules it would
    // only burn cycles, so it stays off. Failure degrades gracefully to
    // per-kind behavior instead of aborting - the tap itself still works.
    let device_rule_count = config
        .read()
        .expect("event tap config lock poisoned")
        .device_rules
        .len();
    if device_rule_count == 0 {
        println!("auto-reverse: no device_rules in config; per-device attribution is off");
    } else {
        match hid::start_wheel_monitor() {
            Ok(()) => println!(
                "auto-reverse: HID wheel monitor started for {device_rule_count} device rule(s)"
            ),
            Err(error) => eprintln!(
                "auto-reverse: device_rules are configured but the HID wheel monitor failed \
                 ({error}); falling back to per-kind flags only"
            ),
        }
    }

    CONFIG
        .set(config)
        .map_err(|_| AppError::Platform("event tap config was already initialized".to_string()))?;

    let event_tap = CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::Default,
        vec![CGEventType::ScrollWheel],
        handle_event,
    )
    .map_err(|_| {
        AppError::Platform(
            "failed to install scroll event tap; Accessibility or Input Monitoring may be missing"
                .to_string(),
        )
    })?;

    let _ = TAP_PORT.set(event_tap.mach_port().as_concrete_TypeRef() as usize);
    let loop_source = event_tap
        .mach_port()
        .create_runloop_source(0)
        .map_err(|_| {
            AppError::Platform("failed to create event tap run-loop source".to_string())
        })?;
    CFRunLoop::get_current().add_source(&loop_source, unsafe { kCFRunLoopCommonModes });
    event_tap.enable();
    CFRunLoop::run_current();
    Ok(())
}

fn handle_event(
    _proxy: CGEventTapProxy,
    event_type: CGEventType,
    event: &CGEvent,
) -> CallbackResult {
    match event_type {
        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
            if let Some(port) = TAP_PORT.get() {
                // CGEventTapEnable takes the CFMachPortRef returned by
                // CGEventTapCreate. The callback proxy is a different opaque
                // token and crashes here on pointer-authenticated macOS.
                unsafe { CGEventTapEnable(*port as CFMachPortRef, true) };
            }
        }
        CGEventType::ScrollWheel => {
            let Some(config_lock) = CONFIG.get() else {
                return CallbackResult::Keep;
            };

            let continuous = !scroll_events::is_physical_mouse_wheel(event);
            let device_kind = conservative_kind_from_continuity(continuous);
            // Attribute only genuine hardware wheel ticks: discrete
            // (continuous scrolling never produces HID wheel values) AND
            // originating from the HID system (source_pid == 0). An event
            // some other process injected did not come from a real device,
            // so pinning it to whatever mouse scrolled last would be wrong -
            // it could inherit that device's rule purely by wall-clock luck.
            let from_hid = scroll_events::event_source_pid(event) == 0;
            let hardware = if continuous || !from_hid {
                None
            } else {
                hid::recent_wheel_device(WHEEL_ATTRIBUTION_WINDOW)
            };

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
            #[cfg_attr(not(feature = "gui"), allow(unused_variables))]
            let decision =
                scroll_events::apply_config_in_place(event, &config, device_kind, hardware);

            #[cfg(feature = "gui")]
            record_debug_event(&config, device_kind, hardware, decision);
        }
        _ => {}
    }
    CallbackResult::Keep
}

/// Builds a `debug_log::DebugEvent` from the same context
/// `handle_event`/`apply_config_in_place` already computed and pushes it to
/// the ring buffer. Does not recompute or duplicate any part of
/// `scroll::transform_event`'s policy - `decision` is exactly what that
/// function already returned; the "why" behind Ignored/Passed is derived
/// from the same `config`/`device_kind`/`hardware` values already in scope
/// in `handle_event`, using the same public accessors (`config.enabled`,
/// `config.should_reverse`, `config.reverse_only_raw_input`) the pure policy
/// itself is built on, not a second reimplementation of it.
#[cfg(feature = "gui")]
fn record_debug_event(
    config: &AppConfig,
    device_kind: DeviceKind,
    hardware: Option<HardwareId>,
    decision: TransformDecision,
) {
    let device_name =
        hardware.and_then(|_| hid::recent_wheel_device_name(WHEEL_ATTRIBUTION_WINDOW));
    let device_description = debug_log::device_description(device_kind, device_name.as_deref());

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

        let (decision_text, category) = decision_text_and_category(
            config,
            device_kind,
            hardware,
            decision.original.synthetic,
            decision.original.source_pid,
            axis_reversed,
        );

        debug_log::push(debug_log::DebugEvent {
            timestamp_ms: debug_log::now_millis(),
            device_description: device_description.clone(),
            axis,
            raw_delta: raw,
            output_delta: out,
            decision_text,
            category,
        });
    }
}

/// Pure derivation of one Debug Console row's text/category for a single
/// axis - split out of `record_debug_event` so the branch order (the actual
/// source of a real, previously-shipped bug) is unit-testable without a
/// live CGEventTap or IOHIDManager. Does not recompute or duplicate
/// `scroll::transform_event`'s policy - `axis_reversed` is exactly what
/// that function already decided; this only chooses WORDING for the cases
/// where it decided "not reversed".
#[cfg(feature = "gui")]
fn decision_text_and_category(
    config: &AppConfig,
    device_kind: DeviceKind,
    hardware: Option<HardwareId>,
    synthetic: bool,
    source_pid: i64,
    axis_reversed: bool,
) -> (String, debug_log::DecisionCategory) {
    if !config.enabled {
        return (
            "Ignored – scroll reversal is off".to_string(),
            debug_log::DecisionCategory::Ignored,
        );
    }
    if synthetic {
        return (
            "Ignored – synthetic event".to_string(),
            debug_log::DecisionCategory::Ignored,
        );
    }
    if config.reverse_only_raw_input && source_pid != 0 {
        return (
            "Ignored – raw input guard (remote desktop)".to_string(),
            debug_log::DecisionCategory::Ignored,
        );
    }
    if axis_reversed {
        return (
            "Reversed".to_string(),
            debug_log::DecisionCategory::Reversed,
        );
    }

    if !config.should_reverse(device_kind, hardware) {
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
        // vertical axis. See decision_text_trackpad_off_by_default_reads_natural_only_when_should_reverse_is_false.
        let has_dont_reverse_rule = hardware.is_some_and(|hw| {
            config
                .device_rules
                .iter()
                .any(|rule| rule.matches(hw) && !rule.reverse)
        });
        return if device_kind == DeviceKind::Unknown {
            (
                "Ignored – unknown devices not reversed".to_string(),
                debug_log::DecisionCategory::Ignored,
            )
        } else if has_dont_reverse_rule {
            (
                "Ignored – this device has a Don't reverse rule".to_string(),
                debug_log::DecisionCategory::Ignored,
            )
        } else if device_kind == DeviceKind::Trackpad {
            // Trackpad-off-by-default is the normal, expected state (not
            // an error) - matches the design handoff's own example
            // wording/category for this exact case.
            (
                "Passed through – trackpad natural".to_string(),
                debug_log::DecisionCategory::Passed,
            )
        } else {
            (
                format!("Ignored – {device_kind} reversal is off"),
                debug_log::DecisionCategory::Ignored,
            )
        };
    }

    // should_reverse(device_kind, hardware) is true here, so this axis's
    // own direction flag (reverse_vertical/reverse_horizontal) is what's
    // off - a real, deliberate "not this direction" outcome, not an error.
    (
        "Passed through".to_string(),
        debug_log::DecisionCategory::Passed,
    )
}

#[cfg(all(test, feature = "gui"))]
mod debug_log_decision_tests {
    use super::*;

    fn config_with(mutate: impl FnOnce(&mut AppConfig)) -> AppConfig {
        let mut config = AppConfig {
            enabled: true,
            ..Default::default()
        };
        mutate(&mut config);
        config
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

        let (text, category) =
            decision_text_and_category(&config, DeviceKind::Trackpad, None, false, 0, false);

        assert_eq!(text, "Passed through");
        assert_eq!(category, debug_log::DecisionCategory::Passed);
    }

    #[test]
    fn trackpad_natural_wording_applies_when_should_reverse_is_genuinely_false() {
        let config = config_with(|c| c.reverse_trackpad = false);
        assert!(!config.should_reverse(DeviceKind::Trackpad, None));

        let (text, category) =
            decision_text_and_category(&config, DeviceKind::Trackpad, None, false, 0, false);

        assert_eq!(text, "Passed through – trackpad natural");
        assert_eq!(category, debug_log::DecisionCategory::Passed);
    }

    #[test]
    fn explicit_dont_reverse_rule_names_itself_over_the_generic_reason() {
        use crate::config::DeviceRule;
        use crate::device::HardwareId;

        let hardware = HardwareId {
            vendor_id: 0x1234,
            product_id: 0x5678,
        };
        let config = config_with(|c| {
            c.reverse_mouse = true;
            c.device_rules.push(DeviceRule {
                vendor_id: hardware.vendor_id,
                product_id: hardware.product_id,
                name: None,
                reverse: false,
            });
        });

        let (text, category) =
            decision_text_and_category(&config, DeviceKind::Mouse, Some(hardware), false, 0, false);

        assert_eq!(text, "Ignored – this device has a Don't reverse rule");
        assert_eq!(category, debug_log::DecisionCategory::Ignored);
    }

    #[test]
    fn unknown_device_kind_names_itself() {
        let config = config_with(|_| {});
        let (text, category) =
            decision_text_and_category(&config, DeviceKind::Unknown, None, false, 0, false);
        assert_eq!(text, "Ignored – unknown devices not reversed");
        assert_eq!(category, debug_log::DecisionCategory::Ignored);
    }

    #[test]
    fn mouse_reversal_off_names_the_device_kind() {
        let config = config_with(|c| c.reverse_mouse = false);
        let (text, category) =
            decision_text_and_category(&config, DeviceKind::Mouse, None, false, 0, false);
        assert_eq!(text, "Ignored – mouse reversal is off");
        assert_eq!(category, debug_log::DecisionCategory::Ignored);
    }

    #[test]
    fn axis_reversed_wins_over_every_other_check() {
        let config = config_with(|_| {});
        let (text, category) =
            decision_text_and_category(&config, DeviceKind::Mouse, None, false, 0, true);
        assert_eq!(text, "Reversed");
        assert_eq!(category, debug_log::DecisionCategory::Reversed);
    }

    #[test]
    fn disabled_config_is_ignored_before_anything_else() {
        let config = config_with(|c| c.enabled = false);
        let (text, category) =
            decision_text_and_category(&config, DeviceKind::Mouse, None, false, 0, true);
        assert_eq!(text, "Ignored – scroll reversal is off");
        assert_eq!(category, debug_log::DecisionCategory::Ignored);
    }

    #[test]
    fn synthetic_event_is_ignored_even_when_axis_reversed_would_otherwise_apply() {
        let config = config_with(|_| {});
        let (text, category) =
            decision_text_and_category(&config, DeviceKind::Mouse, None, true, 0, true);
        assert_eq!(text, "Ignored – synthetic event");
        assert_eq!(category, debug_log::DecisionCategory::Ignored);
    }

    #[test]
    fn raw_input_guard_is_ignored_even_when_axis_reversed_would_otherwise_apply() {
        let config = config_with(|c| c.reverse_only_raw_input = true);
        let non_zero_source_pid = 4242;
        let (text, category) = decision_text_and_category(
            &config,
            DeviceKind::Mouse,
            None,
            false,
            non_zero_source_pid,
            true,
        );
        assert_eq!(text, "Ignored – raw input guard (remote desktop)");
        assert_eq!(category, debug_log::DecisionCategory::Ignored);
    }
}
