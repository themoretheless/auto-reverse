# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project state

Auto Reverse is a working macOS CLI utility that reverses physical
mouse-wheel scroll direction via a CGEventTap while leaving the trackpad
untouched. It is layered so pure logic never touches OS frameworks:

- `src/scroll.rs` - pure reversal policy (no CoreGraphics imports)
- `src/device_classifier.rs` - pure Magic Mouse/trackpad timing and momentum
  state machine, plus the conservative fallback
- `src/config/` - `schema.rs` (fields, defaults, validation) + `store.rs`
  (paths, TOML I/O, atomic save), re-exported through `mod.rs`
- `src/platform/macos/` - ALL unsafe/FFI code: `scroll_events.rs` (CGEvent
  field mapping), `permissions.rs` (Accessibility + Input Monitoring TCC),
  `hid.rs` (IOHIDManager wheel monitor attributing discrete scrolls to a
  serial/location-qualified identity), `gesture.rs` (public listen-only
  AppKit gesture tap), `startup.rs` (LaunchAgent start-at-login),
  `event_tap.rs` (tap runtime), `tray.rs` (native AppKit status item),
  `login_item.rs` (`SMAppService.mainAppService()` for the GUI bundle)
- `src/ui.rs` - egui settings window; starts the tap in-process on a
  background thread and keeps it live while the window is hidden
- `src/main.rs` - CLI (`run`, `doctor`, `enable`, `disable`, `toggle`,
  `enable-startup`, `disable-startup`, `startup-status`, `devices`,
  `prepare-uninstall`, `init`, `config-path`, `show-config`, `simulate`, `help`)
- `scripts/` - hardened local bundle build/check, strict Developer ID and
  notarization release orchestration, staged install/update, identity-checked
  uninstall, exact-path process control, and isolated workflow smokes

Per-device rules: `[[device_rules]]` config blocks (vendor_id/product_id/
reverse) pin one exact physical device's direction; an exact rule wins over
the per-kind flags. Attribution correlates IOHIDManager wheel values with
tap events on the same run loop thread and works for DISCRETE wheels only -
continuous scrolling (trackpad, Magic Mouse) is synthesized from touch data
and never appears as a HID wheel value, so rules cannot target it. `devices`
lists connected pointing devices with their IDs.

The macOS crates are target-specific dependencies. The lean build keeps only
the minimal AppKit event/touch bindings needed by the classifier and excludes
eframe/windows/menus/login-item support; the binary is macOS-only (explicit
`compile_error!`).

Key invariants, both empirically verified - do not "fix" them backwards:

- Only write the CGEvent DeltaAxis1/2 fields. macOS derives
  FixedPtDelta/PointDelta from them automatically; writing the derived
  fields too re-applies the change and silently un-reverses direction
  (see the regression test in `platform/macos/scroll_events.rs`).
- `AXIsProcessTrusted`/`AXIsProcessTrustedWithOptions` return Carbon
  `Boolean` (`unsigned char`), NOT a C99 bool - they are bound as `u8`
  with an explicit `!= 0` on purpose (Rust `bool` has a 0x00/0x01
  validity invariant; other bindings would be unsound).
- AppKit's gesture event type is 29, but `core-graphics` 0.25 models event
  types as a Rust enum that omits 29. The passive callback therefore takes a
  raw `u32`; passing this event through that enum would be invalid. The
  callback has its own autorelease pool and uses only public AppKit APIs.

Known accepted limitations (documented in `doctor` output and
`recommendation.md`): Magic Mouse vs trackpad is a public best-effort
two-finger timing heuristic, not physical identity; failed passive-monitor
startup falls back to the trackpad policy, and rapid device alternation or
third-party smooth wheels may classify imperfectly. `reverse_unknown` has no
live source yet. Four config fields
(`show_menu_bar_icon`, `check_for_updates`, `include_beta_updates`,
`show_discrete_scroll_options`) are stored for planned UI/updater behavior
but are not applied by the runtime yet. `start_at_login` has two intentional
paths: CLI `enable-startup`/`disable-startup` writes a per-user LaunchAgent,
while the GUI settings window registers the `.app` bundle through
`SMAppService.mainAppService()`.

Bundle invariant: `scripts/build-app-bundle.sh` must copy the real Mach-O
binary to `Contents/MacOS/auto-reverse` and keep `CFBundleExecutable` pointed
there. Do not reintroduce a shell launcher that execs a differently named
binary; that broke macOS Control Center status-item scene creation.

Install invariant: validate old destinations by identity only so a damaged app
can be repaired, but validate source/stage/final bundles strictly. Stage and
backup live beside the destination for same-volume moves and rollback. Never
replace exact-path PID matching with broad `pkill auto-reverse`, and never
delete config during uninstall without the explicit user-data flag.

Release invariant: local builds may be ad-hoc, but public artifacts must pass
the Developer ID Application, hardened runtime, secure timestamp, `Accepted`
notarization, stapled ticket, and Gatekeeper checks in `RELEASE.md`. Never
replace those gates with Apple Development signing or plaintext credentials.

See `readme.md` (overview + module map), `architecture.md` (target
architecture, SOLID/DRY layering), `recommendation.md` (backlog + verified
review findings), and `RELEASE.md` (canonical distribution checklist).

Development caveat: macOS ties the Accessibility/Input Monitoring grants to
the binary's identity, so every rebuild requires re-approving the binary in
System Settings > Privacy & Security.

## Commands

- Build: `cargo build`
- Run: `cargo run -- <command>` (e.g. `cargo run -- doctor`; plain `run`
  needs the two privacy permissions)
- Check: `cargo check` (or `cargo check --lib` for the cross-platform core)
- Test: `cargo test` (run a single test with `cargo test <test_name>`)
- Format: `cargo fmt`
- Lint: `cargo clippy --all-targets`
- Install workflow: `scripts/check-install-workflow.sh`
- Release workflow: `scripts/check-release-workflow.sh`
- Safe manual checks without touching real input: `cargo run -- simulate
  --device mouse --dy 1`, `cargo run -- show-config`, `cargo run -- doctor`
- Config path override for tests: `AUTO_REVERSE_CONFIG=/tmp/x.toml`
