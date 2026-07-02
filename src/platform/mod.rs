//! Platform adapters. Everything unsafe, FFI-bound, or OS-specific lives
//! under here; the rest of the crate (config, device, input, scroll) is
//! pure logic that compiles and tests without any OS framework.

#[cfg(target_os = "macos")]
pub mod macos;
