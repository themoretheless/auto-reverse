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
            scroll_events::apply_config_in_place(event, &config, device_kind, hardware);
        }
        _ => {}
    }
    CallbackResult::Keep
}
