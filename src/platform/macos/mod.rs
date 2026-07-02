//! macOS integration, split by concern:
//! - `permissions`: the two TCC permissions (Accessibility, Input
//!   Monitoring) a scroll event tap depends on - checking and prompting.
//! - `scroll_events`: reading/writing scroll data on raw CGEvents.
//! - `event_tap`: the CGEventTap runtime loop that ties it all together.

pub mod event_tap;
pub mod permissions;
pub mod scroll_events;
