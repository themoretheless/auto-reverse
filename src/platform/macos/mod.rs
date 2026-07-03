//! macOS integration, split by concern:
//! - `permissions`: the two TCC permissions (Accessibility, Input
//!   Monitoring) a scroll event tap depends on - checking and prompting.
//! - `scroll_events`: reading/writing scroll data on raw CGEvents.
//! - `hid`: IOHIDManager wheel monitor that attributes discrete scroll
//!   events to a specific physical device (vendor/product ID).
//! - `event_tap`: the CGEventTap runtime loop that ties it all together.
//! - `startup`: per-user LaunchAgent start-at-login support for the CLI.
//! - `daemon_lock`: exclusive-lock guard (`flock`) preventing two `run`
//!   daemons (manual, LaunchAgent, or GUI-spawned) from installing two live
//!   CGEventTaps at once.

pub mod daemon_lock;
pub mod event_tap;
pub mod hid;
pub mod permissions;
pub mod scroll_events;
pub mod startup;
