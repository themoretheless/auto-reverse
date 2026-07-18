# AGENTS.md

This file provides guidance to Codex when working with code in this repository.

## Project State

Auto Reverse is a working macOS Rust utility for reverse scrolling. It has:

- a macOS `CGEventTap` runtime;
- TOML config with validation, durable private atomic save, read-only inspect,
  and explicit exact-backup repair;
- CLI commands in `src/main.rs` with parsing isolated in `src/cli.rs`;
- pure scroll policy in `src/scroll.rs`;
- pure Magic Mouse/trackpad inventory and timing policy in
  `src/device_classifier.rs`;
- field-by-field device profile resolution in `src/config/profiles.rs` and
  pure public HID source policy in `src/device_source.rs`;
- pure bounded wheel-attribution confidence in `src/device_attribution.rs` and
  connected/remembered/unavailable presentation in `src/device_catalog.rs`;
- pure future app-target session pinning in `src/app_session.rs`, shared input
  provenance in `src/input_policy.rs`, and settings lookup in
  `src/settings_search.rs`;
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
- pure temporary preset, scoped reset, notification refresh, and bounded tap
  watchdog policies in `src/preset_preview.rs`, `src/config/reset.rs`,
  `src/refresh_policy.rs`, and `src/tap_watchdog.rs`;
- a pure bounded recovery audit in `src/recovery_audit.rs` with the process-local
  macOS adapter in `src/platform/macos/recovery_log.rs`;
- on-demand public event-tap interval latency snapshots in
  `src/platform/macos/tap_metrics.rs`;
- pure repeated-stall budgets in `src/latency_budget.rs` and a non-live
  two-axis dynamics model split across `src/scroll_dynamics.rs` and
  `src/scroll_dynamics/`;
- split UI helpers under `src/ui/` and pure tray rules under
  `src/platform/macos/tray/`;
- AppKit activation/wake and IOHID generation signals feeding cached UI/tray
  permission and device state;
- CLI LaunchAgent startup support in `src/platform/macos/startup.rs`;
- GUI login-item support via `SMAppService.mainAppService()` in
  `src/platform/macos/login_item.rs`;
- a local `.app` bundle builder plus atomic install/update/uninstall workflow
  under `scripts/`.
- a strict Developer ID/hardened-runtime/notarization/stapling release pipeline
  under `scripts/`, with its checklist in `RELEASE.md`.
- an explicit manual update policy in `src/update_policy.rs` and `UPDATES.md`;
  the app opens trusted release URLs only after user action and has no
  background network client.
- deterministic config/trace/dynamics property suites and fail-closed dynamics
  release evidence under `tests/` and `packaging/`.

The pure domain layer should stay free of CoreGraphics/AppKit imports. Keep OS
framework code inside `platform/macos`.

## Important Invariants

- Only write CGEvent `DeltaAxis1/2`; macOS derives fixed-point/pixel deltas.
- Accessibility APIs return Carbon `Boolean` (`u8`), not Rust `bool`.
- The active modifying event tap requires Accessibility only. Accessibility
  grants both posting and listening; never block runtime startup on a separate
  Input Monitoring preflight.
- A disabled persisted utility must not prompt for Accessibility or show a
  blocked-feature callout. Passive refreshes are read-only; prompting is
  reserved for enabled startup or an explicit Enable action.
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
- `input_policy` is the sole source of truth for synthetic, virtual, unknown,
  and posted-process bypass. Transform and diagnostic reasons must not grow
  separate precedence trees.
- Application rules are not live. A future adapter must feed target PIDs
  through `AppTargetSessionPin`; never change target policy inside momentum or
  claim that frontmost application is necessarily the scroll target.
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
- Event-tap health checks use public `CGEventTapIsEnabled` under the registered
  port lifetime guard. Keep two-sample hysteresis, the three-attempt episode
  budget, and sustained-healthy reset; never introduce an unbounded retry loop.
- Recovery audit reasons must stay exact and independent. Only the callback may
  claim `TapDisabledByTimeout`; watchdog failure is not timeout evidence. Keep
  the ring at 64 privacy-bounded records without PID, device identity, or wall
  time, and audit permission loss only while the utility is enabled.
- Experimental dynamics remains discrete-wheel-only and outside the live event
  tap until cancellation, scheduler, and fail-open gates pass.
- Dynamics default enablement is fail-closed: the runtime kill switch wins,
  source and manifest defaults must match, and all release thresholds require
  exact-candidate evidence. Rollback clears only smooth presets and must
  preserve direction, alias, step size, startup, and unrelated config.
- Keep PR-01..PR-06 ID/scenario pairs stable. Structure smoke may accept blank
  manual rows, but a production release must require build, `Pass`, and
  evidence/tester for every row before build or Apple service access.
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
- Config commits use a unique `create_new` temporary file with mode `0600`,
  sync the file before rename, and sync the parent directory after rename.
  Read-only inspection must not create a directory, config, or lock.
- Corrupted-config repair is explicit. Preserve invalid bytes in an exclusive
  sibling backup, never overwrite an existing backup, leave valid TOML
  byte-for-byte unchanged, and restore the original path if replacement fails.
- Release URLs are compile-time canonical GitHub destinations. The legacy
  automatic-update flag must never cause background network I/O; beta preference
  may choose only the manual all-releases destination.
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
- Validate config: `cargo run -- validate-config --json`
- Repair config: `cargo run -- repair-config`
- Open releases: `cargo run -- open-releases --latest`
- Devices: `cargo run -- devices`
- Roll back dynamics presets: `cargo run -- rollback-dynamics`
- Check: `cargo check`
- Lean check: `cargo check --no-default-features`
- Test: `cargo test`
- Format: `cargo fmt`
- Lint: `cargo clippy -- -D warnings`
