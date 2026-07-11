//! Auto Reverse - independently controls physical mouse-wheel, trackpad, and
//! Magic Mouse scrolling direction on macOS.
//!
//! Layering, from pure to platform-bound (each layer only depends on the
//! ones above it):
//! - [`error`], [`device`], [`input`]: shared vocabulary types.
//! - [`device_classifier`]: pure device-source classification state.
//! - [`config`]: schema, pure physical-device rule resolution, and storage.
//! - [`runtime`]: process-local controls such as temporary pause.
//! - [`scroll`]: the pure reversal policy - config + event in, decision
//!   out. No OS types anywhere.
//! - [`platform`]: everything OS-specific and unsafe. `platform::macos`
//!   holds the CGEvent field mapping, the TCC permission calls, LaunchAgent
//!   startup, and the CGEventTap runtime.

pub mod config;
pub mod device;
pub mod device_classifier;
pub mod error;
pub mod input;
pub mod platform;
pub mod runtime;
pub mod scroll;
#[cfg(all(feature = "gui", target_os = "macos"))]
pub mod ui;
