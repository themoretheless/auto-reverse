//! macOS integration, split by concern:
//! - `permissions`: the Accessibility permission required by the active
//!   scroll event tap - checking and prompting.
//! - `scroll_events`: reading/writing scroll data on raw CGEvents.
//! - `hid`: IOHIDManager wheel monitor that attributes discrete scroll
//!   events to a specific physical device (vendor/product ID).
//! - `gesture`: passive public AppKit gesture monitor and the adapter from
//!   macOS event fields into the pure Magic Mouse/trackpad classifier.
//! - `event_tap`: the CGEventTap runtime loop that ties it all together.
//! - `external_url`: opens canonical product release pages in the browser.
//! - `startup`: per-user LaunchAgent start-at-login support for the
//!   headless CLI (`enable-startup`/`disable-startup`, targets `run`).
//! - `daemon_lock`: exclusive-lock guard (`flock`) preventing two live
//!   CGEventTaps at once, regardless of which process/thread installs them.
//! - `activation` (gui only): PID-addressed file mailbox that lets a second
//!   GUI launch reveal and focus the existing settings window.
//! - `app_events` (gui only): coalesced app-activation notification used to
//!   refresh cached permission/device state after returning from Settings.
//! - `debug_log` (gui only): bounded ring buffer of structured scroll
//!   decisions. `event_tap` records raw fields plus a stable reason enum; the
//!   Debug Console formats them only while presenting or exporting.
//! - `login_item` (gui only): `SMAppService.mainAppService()` wrapper -
//!   login-item registration for the bundled GUI app. Deliberately separate
//!   from `startup` (see recommendation.md risk #6): two mechanisms for two
//!   different binaries/use cases, not meant to be unified.
//! - `power_events` (gui only): `NSWorkspace` sleep/wake notifications used
//!   to re-arm or restart the in-process event tap after wake.
//! - `recovery_log`: process-local adapter for typed, bounded recovery audit.
//! - `save_panel` (gui only): native CSV destination picker and Finder reveal.
//! - `tap_metrics`: on-demand public `CGGetEventTapList` interval snapshots;
//!   never polled because each read resets the tap's min/max interval.
//! - `tray` (gui only): native AppKit menu-bar item for the merged
//!   settings-window + event-tap process.
//! - `quit_handler` (gui only): overrides the `kAEQuitApplication` Apple
//!   Event (Cmd-Q, Dock quit, AppleScript `quit`) so only the tray's own
//!   Quit action can end the merged process.

#[cfg(feature = "gui")]
pub mod activation;
#[cfg(feature = "gui")]
pub mod app_events;
pub mod daemon_lock;
#[cfg(feature = "gui")]
pub mod debug_log;
pub mod event_tap;
pub mod external_url;
pub mod gesture;
pub mod hid;
#[cfg(feature = "gui")]
pub mod login_item;
pub mod permissions;
#[cfg(feature = "gui")]
pub mod power_events;
#[cfg(feature = "gui")]
pub mod quit_handler;
pub mod recovery_log;
#[cfg(feature = "gui")]
pub mod save_panel;
pub mod scroll_events;
pub mod startup;
pub mod tap_metrics;
#[cfg(feature = "gui")]
pub mod tray;
