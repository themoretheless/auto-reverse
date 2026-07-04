# AGENTS.md

This file provides guidance to Codex when working with code in this repository.

## Project State

Auto Reverse is a working macOS Rust utility for reverse scrolling. It has:

- a macOS `CGEventTap` runtime;
- TOML config with validation and atomic save;
- CLI commands in `src/main.rs` with parsing isolated in `src/cli.rs`;
- pure scroll policy in `src/scroll.rs`;
- macOS-specific FFI under `src/platform/macos/`;
- an egui settings window in `src/ui.rs`;
- a native AppKit menu-bar item in `src/platform/macos/tray.rs`;
- CLI LaunchAgent startup support in `src/platform/macos/startup.rs`;
- GUI login-item support via `SMAppService.mainAppService()` in
  `src/platform/macos/login_item.rs`;
- a local `.app` bundle builder in `scripts/build-app-bundle.sh`.

The pure domain layer should stay free of CoreGraphics/AppKit imports. Keep OS
framework code inside `platform/macos`.

## Important Invariants

- Only write CGEvent `DeltaAxis1/2`; macOS derives fixed-point/pixel deltas.
- Accessibility APIs return Carbon `Boolean` (`u8`), not Rust `bool`.
- The GUI and CLI tap paths share `daemon_lock`; never allow two live taps.
- The `.app` bundle must launch the real Mach-O binary at
  `Contents/MacOS/auto-reverse`; do not reintroduce a shell wrapper.

## Commands

- Build: `cargo build`
- Bundle: `scripts/build-app-bundle.sh`
- Run GUI: `cargo run -- ui`
- Run headless tap: `cargo run -- run`
- Diagnostics: `cargo run -- doctor --no-create`
- Devices: `cargo run -- devices`
- Check: `cargo check`
- Lean check: `cargo check --no-default-features`
- Test: `cargo test`
- Format: `cargo fmt`
- Lint: `cargo clippy -- -D warnings`
