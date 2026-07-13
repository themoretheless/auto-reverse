# AGENTS.md

This file provides guidance to Codex when working with code in this repository.

## Project State

Auto Reverse is a working macOS Rust utility for reverse scrolling. It has:

- a macOS `CGEventTap` runtime;
- TOML config with validation and atomic save;
- CLI commands in `src/main.rs` with parsing isolated in `src/cli.rs`;
- pure scroll policy in `src/scroll.rs`;
- pure Magic Mouse/trackpad inventory and timing policy in
  `src/device_classifier.rs`;
- field-by-field device profile resolution in `src/config/profiles.rs` and
  pure public HID source policy in `src/device_source.rs`;
- pure bounded wheel-attribution confidence in `src/device_attribution.rs` and
  connected/remembered/unavailable presentation in `src/device_catalog.rs`;
- macOS-specific FFI under `src/platform/macos/`;
- a public listen-only AppKit gesture monitor in
  `src/platform/macos/gesture.rs`;
- an egui settings window in `src/ui.rs`;
- a native AppKit menu-bar item in `src/platform/macos/tray.rs`;
- a process-local temporary pause in `src/runtime.rs`;
- privacy-bounded trace/replay and observed-rate diagnostics in
  `src/scroll_trace.rs`, `src/scroll_lab.rs`, and `src/event_rate.rs`;
- a pure ScrollTest-style state machine in `src/scroll_benchmark.rs` plus an
  interactive egui benchmark viewport in `src/ui/scroll_benchmark.rs`;
- on-demand public event-tap interval latency snapshots in
  `src/platform/macos/tap_metrics.rs`;
- pure repeated-stall budgets in `src/latency_budget.rs` and a non-live
  two-axis dynamics model split across `src/scroll_dynamics.rs` and
  `src/scroll_dynamics/`;
- split UI helpers under `src/ui/` and pure tray rules under
  `src/platform/macos/tray/`;
- CLI LaunchAgent startup support in `src/platform/macos/startup.rs`;
- GUI login-item support via `SMAppService.mainAppService()` in
  `src/platform/macos/login_item.rs`;
- a local `.app` bundle builder plus atomic install/update/uninstall workflow
  under `scripts/`.
- a strict Developer ID/hardened-runtime/notarization/stapling release pipeline
  under `scripts/`, with its checklist in `RELEASE.md`.

The pure domain layer should stay free of CoreGraphics/AppKit imports. Keep OS
framework code inside `platform/macos`.

## Important Invariants

- Only write CGEvent `DeltaAxis1/2`; macOS derives fixed-point/pixel deltas.
- Accessibility APIs return Carbon `Boolean` (`u8`), not Rust `bool`.
- The active modifying event tap requires Accessibility only. Accessibility
  grants both posting and listening; never block runtime startup on a separate
  Input Monitoring preflight.
- AppKit gesture event type 29 must cross the passive callback as raw `u32`;
  the `core-graphics` crate's Rust enum omits it, so constructing that enum is
  invalid. Do not replace this bridge with private MultitouchSupport APIs.
- Missing two-finger observations do not prove a Magic Mouse. An exclusive
  public IOHID product inventory wins; unknown inventory falls back to
  Trackpad, and gesture timing is used only when both sources are connected.
- Profile fields resolve independently by serial, location, VID/PID, device
  kind, then global default. Keep them in the existing `device_rules`; do not
  add a second profile database. Direction is optional: `None` inherits and
  must not delete alias, step-size, or preset overrides.
- Last-active IOHID wheel data is correlation evidence, not an exact event
  identifier. Accept identity only at `high`/`medium` confidence within the
  50 ms attribution timeout; stale or missing observations must not inherit it.
- An attributed wheel with exact public transport `Virtual`, or with an
  unknown/missing observed transport, must pass through. No wheel snapshot is
  `NotObserved` and preserves the existing fallback. Transport alone does not
  prove physical provenance because CoreHID virtual devices may declare any
  transport.
- Do not persist the undocumented IORegistry literal `DeviceAddress`. Do not
  merge receiver children solely by parent, `PhysicalDeviceUniqueID`, location,
  or `HIDRMHash`.
- `CGGetEventTapList` resets listed taps' min/max latency to their average.
  Keep latency sampling explicit and label it as an interval snapshot; never
  poll it from the UI.
- A latency warning requires repeated readings; use interval maxima for tail
  stalls and never turn one maximum sample into a persistent warning.
- Experimental dynamics remains discrete-wheel-only and outside the live event
  tap until cancellation, scheduler, and fail-open gates pass.
- Continuous input must use exact dynamics bypass. Vertical and horizontal
  velocity, residual, momentum, rate window, and deadline remain independent.
- Direction/gap/external cancellation must record signed canceled distance;
  never hide it as conservation error. Sub-threshold momentum is flushed, not
  dropped.
- Every experimental tail sample carries the two-axis session generation,
  wake id, due-anchored 8 ms TTL, and synthetic provenance. A late wake or
  dynamics/scheduler fault must clear the active wake and latch exact fail-open
  until explicit reset.
- Events posted by a future scheduler must set the `AUTORVRS` marker in public
  `kCGEventSourceUserData`; normalized marked events are synthetic and cannot
  re-enter reversal or dynamics policy.
- The GUI and CLI runtime paths share `daemon_lock`; never allow two runtime
  instances. One runtime owns the active scroll tap and optional passive
  gesture tap together.
- The `.app` bundle must launch the real Mach-O binary at
  `Contents/MacOS/auto-reverse`; do not reintroduce a shell wrapper.
- Public release artifacts must pass the Developer ID, secure timestamp,
  notarization, stapling, and Gatekeeper gates in `RELEASE.md`; an ad-hoc or
  Apple Development signature is never a distributable substitute.

## Commands

- Build: `cargo build`
- Bundle: `scripts/build-app-bundle.sh`
- Bundle smoke: `scripts/check-app-bundle.sh`
- Release workflow smoke: `scripts/check-release-workflow.sh`
- Production release: `scripts/release-app-bundle.sh --help`
- Install workflow smoke: `scripts/check-install-workflow.sh`
- Install release app: `scripts/install-app-bundle.sh`
- Uninstall app: `scripts/uninstall-app-bundle.sh`
- Run GUI: `cargo run -- ui`
- Run benchmark: `cargo run -- benchmark`
- Run headless tap: `cargo run -- run`
- Diagnostics: `cargo run -- doctor --no-create`
- Devices: `cargo run -- devices`
- Check: `cargo check`
- Lean check: `cargo check --no-default-features`
- Test: `cargo test`
- Format: `cargo fmt`
- Lint: `cargo clippy -- -D warnings`
