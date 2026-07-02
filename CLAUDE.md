# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project state

Auto Reverse is a working macOS CLI utility that reverses physical
mouse-wheel scroll direction via a CGEventTap while leaving the trackpad
untouched. It is layered so pure logic never touches OS frameworks:

- `src/scroll.rs` - pure reversal policy (no CoreGraphics imports)
- `src/config/` - `schema.rs` (fields, defaults, validation) + `store.rs`
  (paths, TOML I/O, atomic save), re-exported through `mod.rs`
- `src/platform/macos/` - ALL unsafe/FFI code: `scroll_events.rs` (CGEvent
  field mapping), `permissions.rs` (Accessibility + Input Monitoring TCC),
  `event_tap.rs` (tap runtime)
- `src/main.rs` - CLI (`run`, `doctor`, `enable`, `disable`, `toggle`,
  `init`, `config-path`, `show-config`, `simulate`, `help`)

The macOS crates are target-specific dependencies; `cargo check --lib`
builds on any OS, the binary is macOS-only (explicit `compile_error!`).

Key invariants, both empirically verified - do not "fix" them backwards:

- Only write the CGEvent DeltaAxis1/2 fields. macOS derives
  FixedPtDelta/PointDelta from them automatically; writing the derived
  fields too re-applies the change and silently un-reverses direction
  (see the regression test in `platform/macos/scroll_events.rs`).
- `AXIsProcessTrusted`/`AXIsProcessTrustedWithOptions` return Carbon
  `Boolean` (`unsigned char`), NOT a C99 bool - they are bound as `u8`
  with an explicit `!= 0` on purpose (Rust `bool` has a 0x00/0x01
  validity invariant; other bindings would be unsound).

Known accepted limitations (documented in `doctor` output and
`recommendation.md`): a Magic Mouse cannot be distinguished from the
trackpad through the public CGEventTap API, so `reverse_magic_mouse` and
`reverse_unknown` currently have no effect; five config fields
(`start_at_login`, `show_menu_bar_icon`, `check_for_updates`,
`include_beta_updates`, `show_discrete_scroll_options`) are reserved for a
future menu-bar app.

See `readme.md` (overview + module map), `architecture.md` (target
architecture, SOLID/DRY layering), `recommendation.md` (backlog + verified
review findings).

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
- Safe manual checks without touching real input: `cargo run -- simulate
  --device mouse --dy 1`, `cargo run -- show-config`, `cargo run -- doctor`
- Config path override for tests: `AUTO_REVERSE_CONFIG=/tmp/x.toml`
