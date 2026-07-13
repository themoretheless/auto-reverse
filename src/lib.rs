//! Auto Reverse - independently controls physical mouse-wheel, trackpad, and
//! Magic Mouse scrolling direction on macOS.
//!
//! Layering, from pure to platform-bound (each layer only depends on the
//! ones above it):
//! - [`error`], [`device`], [`input`]: shared vocabulary types.
//! - [`diagnostics`]: pure axis and decision-reason vocabulary.
//! - [`device_classifier`]: pure device-source classification state.
//! - [`config`]: schema, pure physical-device rule resolution, and storage.
//! - [`runtime`]: process-local controls such as temporary pause.
//! - [`scroll`]: the pure reversal policy - config + event in, decision
//!   out. No OS types anywhere.
//! - [`scroll_trace`] and [`scroll_lab`]: bounded replay data and pure
//!   transfer-function measurements.
//! - [`event_rate`] and [`scroll_benchmark`]: observed arrival-rate and
//!   target-acquisition diagnostics, with no GUI or macOS dependencies.
//! - [`latency_budget`] and [`scroll_dynamics`]: repeated-stall assessment
//!   and the pure experimental transactional two-axis wheel model.
//! - [`platform`]: everything OS-specific and unsafe. `platform::macos`
//!   holds the CGEvent field mapping, the TCC permission calls, LaunchAgent
//!   startup, and the CGEventTap runtime.

pub mod config;
pub mod device;
pub mod device_classifier;
pub mod diagnostics;
pub mod error;
pub mod event_rate;
pub mod input;
pub mod latency_budget;
pub mod platform;
pub mod runtime;
pub mod scroll;
pub mod scroll_benchmark;
pub mod scroll_dynamics;
pub mod scroll_lab;
pub mod scroll_trace;
pub mod statistics;
#[cfg(all(feature = "gui", target_os = "macos"))]
pub mod ui;
