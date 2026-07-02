//! Auto Reverse - reverses physical mouse-wheel scrolling on macOS while
//! leaving the trackpad untouched.
//!
//! Layering, from pure to platform-bound (each layer only depends on the
//! ones above it):
//! - [`error`], [`device`], [`input`]: shared vocabulary types.
//! - [`config`]: what the settings are (`schema`) and where they live
//!   (`store`).
//! - [`scroll`]: the pure reversal policy - config + event in, decision
//!   out. No OS types anywhere.
//! - [`platform`]: everything OS-specific and unsafe. `platform::macos`
//!   holds the CGEvent field mapping, the TCC permission calls, LaunchAgent
//!   startup, and the CGEventTap runtime.

pub mod config;
pub mod device;
pub mod error;
pub mod input;
pub mod platform;
pub mod scroll;
#[cfg(all(feature = "gui", target_os = "macos"))]
pub mod ui;
