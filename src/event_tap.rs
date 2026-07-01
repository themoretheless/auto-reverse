use core_foundation::runloop::CFRunLoop;
use core_graphics::event::{
    CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType, CallbackResult,
};
use std::sync::OnceLock;

use crate::config::AppConfig;
use crate::device::conservative_kind_from_continuity;
use crate::error::{AppError, AppResult};
use crate::scroll;

// The `core-graphics` crate only exposes `CGEventTapEnable` on an owned,
// already-installed `CGEventTap`, but the tap-disabled recovery path in
// `handle_event` only has the raw `CGEventTapProxy` the callback receives.
// Apple's own docs for the callback say that proxy is what you pass back
// into `CGEventTapEnable` to re-arm a tap the system disabled, so we bind
// the C symbol directly (already linked in via core-graphics' "link"
// feature, which is on by default).
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventTapEnable(tap: CGEventTapProxy, enable: bool);
}

static CONFIG: OnceLock<AppConfig> = OnceLock::new();

/// Installs a system-wide event tap that reverses scroll direction for
/// physical mouse wheels, then blocks running the current thread's run loop
/// forever. Returns `Err(())` if macOS refused to create the tap, which is
/// almost always a missing Input Monitoring / Accessibility permission.
pub fn install_and_run(config: AppConfig) -> AppResult<()> {
    CONFIG
        .set(config)
        .map_err(|_| AppError::Platform("event tap config was already initialized".to_string()))?;

    CGEventTap::with_enabled(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::Default,
        vec![CGEventType::ScrollWheel],
        handle_event,
        CFRunLoop::run_current,
    )
    .map_err(|_| {
        AppError::Platform(
            "failed to install scroll event tap; Accessibility or Input Monitoring may be missing"
                .to_string(),
        )
    })
}

fn handle_event(
    proxy: CGEventTapProxy,
    event_type: CGEventType,
    event: &CGEvent,
) -> CallbackResult {
    match event_type {
        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
            // macOS disables a tap it thinks is too slow or that the user
            // paused; without re-enabling it here, scrolling silently stops
            // being reversed until the process is restarted.
            unsafe { CGEventTapEnable(proxy, true) };
        }
        CGEventType::ScrollWheel => {
            let Some(config) = CONFIG.get() else {
                return CallbackResult::Keep;
            };

            let continuous = !scroll::is_physical_mouse_wheel(event);
            let device_kind = conservative_kind_from_continuity(continuous);
            scroll::apply_config_in_place(event, config, device_kind);
        }
        _ => {}
    }
    CallbackResult::Keep
}
