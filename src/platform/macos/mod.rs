//! macOS integration, split by concern:
//! - `permissions`: the two TCC permissions (Accessibility, Input
//!   Monitoring) a scroll event tap depends on - checking and prompting.
//! - `scroll_events`: reading/writing scroll data on raw CGEvents.
//! - `hid`: IOHIDManager wheel monitor that attributes discrete scroll
//!   events to a specific physical device (vendor/product ID).
//! - `event_tap`: the CGEventTap runtime loop that ties it all together.
//! - `startup`: per-user LaunchAgent start-at-login support for the
//!   headless CLI (`enable-startup`/`disable-startup`, targets `run`).
//! - `daemon_lock`: exclusive-lock guard (`flock`) preventing two live
//!   CGEventTaps at once, regardless of which process/thread installs them.
//! - `debug_log` (gui only): bounded ring buffer of scroll-reversal
//!   decisions, recorded from `event_tap::handle_event` and shown live by
//!   the Debug Console window.
//! - `login_item` (gui only): `SMAppService.mainAppService()` wrapper -
//!   login-item registration for the bundled GUI app. Deliberately separate
//!   from `startup` (see recommendation.md risk #6): two mechanisms for two
//!   different binaries/use cases, not meant to be unified.
//! - `tray` (gui only): native AppKit menu-bar item for the merged
//!   settings-window + event-tap process.
//! - `quit_handler` (gui only): overrides the `kAEQuitApplication` Apple
//!   Event (Cmd-Q, Dock quit, AppleScript `quit`) so only the tray's own
//!   Quit action can end the merged process.

pub mod daemon_lock;
#[cfg(feature = "gui")]
pub mod debug_log;
pub mod event_tap;
pub mod hid;
#[cfg(feature = "gui")]
pub mod login_item;
pub mod permissions;
#[cfg(feature = "gui")]
pub mod quit_handler;
pub mod scroll_events;
pub mod startup;
#[cfg(feature = "gui")]
pub mod tray;
