//! Neutralizes the standard AppKit application-termination pathway (Cmd-Q,
//! Dock/Activity Monitor "Quit", or an AppleScript
//! `tell application "Auto Reverse" to quit`), so that only the tray menu's
//! own "Quit" action (`platform::macos::tray::TrayAction::Quit`, which calls
//! `std::process::exit(0)` directly) can end this process. See `ui.rs`'s
//! module doc comment for why the process must outlive a hidden window.
//!
//! ## Why this is not done via `NSApplicationDelegate`
//!
//! The obvious-looking fix is to implement `applicationShouldTerminate:` on
//! `NSApp`'s delegate and always return `NSTerminateCancel`. That does not
//! work here: eframe 0.35 (via winit 0.30's AppKit backend) already installs
//! its own delegate object (`WinitApplicationDelegate`,
//! `winit::platform_impl::macos::app_state::ApplicationDelegate`) on
//! `NSApp.delegate` when the event loop is created, and several of winit's
//! own run-loop-observer callbacks
//! (`platform_impl::macos::observer::{control_flow_handler, ...}`) fetch it
//! back via a helper that does:
//!
//! ```text
//! let delegate = app.delegate().expect(...);
//! if delegate.is_kind_of::<Self>() { ... } else { panic!(...) }
//! ```
//!
//! `is_kind_of` checks the Objective-C class of the *actual* delegate
//! object. Replacing `NSApp.delegate` with a different object - even one
//! that forwards every other selector straight back to winit's original
//! delegate - is still a different class, so that `is_kind_of::<Self>()`
//! check fails and winit panics on close to the very next event-loop tick.
//! This was confirmed by reading winit 0.30.13's source directly
//! (`~/.cargo/registry/.../winit-0.30.13/src/platform_impl/macos/observer.rs`
//! and `app_state.rs`) before writing this module, rather than by trial and
//! error against a panic.
//!
//! ## What this does instead
//!
//! Cmd-Q, Dock "Quit", and the AppleScript/Apple Event Manager `quit`
//! command all resolve to the exact same mechanism under the hood: the
//! `kAEQuitApplication` Apple Event (class `kCoreEventClass` = `'aevt'`,
//! ID `kAEQuitApplication` = `'quit'`). `NSApplication` itself handles that
//! event by registering a normal, ordinary Apple Event handler for that
//! class/ID pair via the classic Carbon/Core Services Apple Event Manager
//! (`AEInstallEventHandler`, still present and fully supported on modern
//! macOS in the `AE.framework` subframework of `ApplicationServices`/
//! `CoreServices` - it predates `NSApplicationDelegate` and is completely
//! independent of it) - that registered handler is what eventually calls
//! `applicationShouldTerminate:` on the delegate and then proceeds to
//! terminate.
//!
//! `AEInstallEventHandler` lets any part of the process re-register a
//! handler for the same class/ID pair, which simply replaces whatever
//! handler was there before in this process's own Apple Event dispatch
//! table - it does not touch `NSApp.delegate` or any object winit owns, so
//! it cannot trip winit's `is_kind_of` check. Installing our own no-op
//! handler here means `NSApplication`'s own quit handler (and therefore
//! `applicationShouldTerminate:`) is simply never invoked at all for this
//! event again; the Apple Event Manager considers the event handled the
//! moment our handler returns `noErr`, so no termination sequence ever
//! starts. The handler also flips a flag `ui.rs` polls once per frame to
//! hide the window, matching the existing close-button/Cmd-W UX exactly.
//!
//! This has been verified empirically (see the change's own report) to
//! survive `osascript -e 'tell application "Auto Reverse" to quit'`, Cmd-Q,
//! and Dock "Quit", while the tray's "Quit" item (`std::process::exit(0)`,
//! which bypasses `NSApplication` and the Apple Event Manager entirely)
//! still ends the process.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set by the Apple Event handler when a quit was requested and swallowed;
/// polled once per frame by `SettingsApp::logic` and turned into the same
/// `ViewportCommand::Visible(false)` used for the window-close-to-hide path.
static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);

// Opaque stand-ins for the Carbon/Apple Event Manager types this handler's
// C signature needs (AEDataModel.h, AppleEvents.h). Only pointers to these
// ever cross the FFI boundary in this file - the handler never inspects the
// event or writes a reply - so representing both as zero-sized opaque types
// (rather than binding every `AEDesc` field) is both sufficient and safer:
// there is no real struct layout for Rust to get wrong.
#[repr(C)]
struct OpaqueAppleEvent {
    _private: [u8; 0],
}

type FourCharCode = u32;
type OsErr = i16;

const NO_ERR: OsErr = 0;
// 'aevt' - kCoreEventClass (AppleEvents.h).
const CORE_EVENT_CLASS: FourCharCode = 0x6165_7674;
// 'quit' - kAEQuitApplication (AppleEvents.h).
const AE_QUIT_APPLICATION: FourCharCode = 0x7175_6974;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AEInstallEventHandler(
        the_ae_event_class: FourCharCode,
        the_ae_event_id: FourCharCode,
        handler: unsafe extern "C" fn(
            the_apple_event: *const OpaqueAppleEvent,
            reply: *mut OpaqueAppleEvent,
            handler_refcon: *mut c_void,
        ) -> OsErr,
        handler_refcon: *mut c_void,
        is_sys_handler: bool,
    ) -> OsErr;
}

/// Replaces `NSApplication`'s own `kAEQuitApplication` handler with a no-op
/// that never lets the process terminate. Must be called once, after
/// `NSApplication`/eframe has started (so there is an existing Cocoa handler
/// to override) - in practice this means from inside `SettingsApp::logic`'s
/// first tick, same as `tray::build()`, since that is the first point this
/// codebase is guaranteed to be running on the main thread with the
/// application fully launched. Calling it again just replaces the handler
/// again, so it is harmless (if pointless) to call more than once.
pub fn install() {
    let status = unsafe {
        AEInstallEventHandler(
            CORE_EVENT_CLASS,
            AE_QUIT_APPLICATION,
            handle_quit_event,
            std::ptr::null_mut(),
            false,
        )
    };
    if status != NO_ERR {
        // Nothing sensible to do with a failure here beyond leaving the
        // default Cocoa quit behavior in place - there is no fallback
        // AE-level mechanism to retry with, and this is called from a
        // context (`logic`) that does not thread errors back to the UI's
        // existing error-banner fields. A failure would only mean Cmd-Q
        // and Dock quit behave as they did before this fix; the tray's
        // Quit path is unaffected either way.
        eprintln!("auto-reverse: could not install the quit-event handler (OSErr {status})");
    }
}

/// The actual Apple Event handler. Matches `AEEventHandlerProcPtr` exactly
/// (`AEDataModel.h`): `OSErr (*)(const AppleEvent *, AppleEvent *, SRefCon)`.
/// Ignores both descriptors entirely - swallowing the event (returning
/// `noErr` without touching `reply`) is sufficient to tell the Apple Event
/// Manager the event was handled, which is what prevents `NSApplication`'s
/// own (now-overridden) handler, and therefore `applicationShouldTerminate:`
/// and the entire termination sequence, from ever running.
unsafe extern "C" fn handle_quit_event(
    _the_apple_event: *const OpaqueAppleEvent,
    _reply: *mut OpaqueAppleEvent,
    _handler_refcon: *mut c_void,
) -> OsErr {
    QUIT_REQUESTED.store(true, Ordering::SeqCst);
    NO_ERR
}

/// Non-blocking poll, meant to be called once per eframe update tick
/// alongside `tray::poll_action()`. Returns `true` at most once per quit
/// attempt (Cmd-Q, Dock quit, or an AppleScript `quit` command).
pub fn poll_quit_requested() -> bool {
    QUIT_REQUESTED.swap(false, Ordering::SeqCst)
}
