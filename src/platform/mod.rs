//! Platform adapters. Everything unsafe or FFI-bound lives under here; the
//! rest of the crate (config, device, input, scroll) compiles without any
//! OS framework. One honest exception to a stricter reading: config's
//! default-path resolution keeps a small cfg(target_os) branch for the
//! macOS Application Support convention - std-only, no frameworks, but
//! OS-aware.

#[cfg(target_os = "macos")]
pub mod macos;
