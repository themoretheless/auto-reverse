//! Auto Reverse - independently controls physical mouse-wheel, trackpad, and
//! Magic Mouse scrolling direction on macOS.
//!
//! Layering, from pure to platform-bound (each layer only depends on the
//! ones above it):
//! - [`error`], [`device`], [`input`]: shared vocabulary types.
//! - [`input_policy`]: pure source provenance and bypass precedence.
//! - [`diagnostics`]: pure axis and decision-reason vocabulary.
//! - [`diagnostics_summary`]: privacy-bounded aggregate support text.
//! - [`device_classifier`]: pure device-source classification state.
//! - [`device_source`]: pure public-HID transport trust classification.
//! - [`app_session`]: non-live target-PID pinning for future app rules.
//! - [`settings_search`]: pure settings and diagnostics lookup.
//! - [`preset_preview`], [`refresh_policy`], and [`tap_watchdog`]: temporary
//!   UX state plus notification/recovery policies with no OS imports.
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
//! - [`scroll_scheduler`]: pure generation/TTL wake leases and fail-open
//!   orchestration for that model; it owns no timer or platform object.
//! - [`platform`]: everything OS-specific and unsafe. `platform::macos`
//!   holds the CGEvent field mapping, the TCC permission calls, LaunchAgent
//!   startup, and the CGEventTap runtime.

pub mod app_session;
pub mod config;
pub mod device;
pub mod device_attribution;
pub mod device_catalog;
pub mod device_classifier;
pub mod device_source;
pub mod device_test;
pub mod diagnostics;
pub mod diagnostics_summary;
pub mod error;
pub mod event_rate;
pub mod input;
pub mod input_policy;
pub mod latency_budget;
pub mod platform;
pub mod preset_preview;
pub mod refresh_policy;
pub mod runtime;
pub mod scroll;
pub mod scroll_benchmark;
pub mod scroll_dynamics;
pub mod scroll_lab;
pub mod scroll_scheduler;
pub mod scroll_trace;
pub mod settings_search;
pub mod statistics;
pub mod tap_watchdog;
#[cfg(all(feature = "gui", target_os = "macos"))]
pub mod ui;
