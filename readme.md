# Auto Reverse

Auto Reverse is a Rust/macOS utility for reverse scrolling. The target product is feature parity with Scroll Reverser: keep a trackpad natural, reverse a wheel mouse, expose clear settings, and never make system input feel mysterious.

## Current Status

Implemented:

- macOS `CGEventTap` for scroll events;
- TOML config with `config_version`;
- global enable/disable;
- vertical and horizontal reverse flags;
- mouse, trackpad, Magic Mouse and unknown-device config flags;
- wheel step size;
- raw-input guard through `source_pid`;
- Accessibility and Input Monitoring checks;
- CLI diagnostics and simulation;
- 16 unit tests after the latest merge.

Still missing:

- menu bar UI;
- preferences window;
- start at login;
- hide/show menu bar icon;
- debug console;
- gesture/HID classifier for Magic Mouse vs trackpad;
- packaging/signing/update flow.

## Commands

```bash
cargo build
cargo run -- doctor
cargo run -- show-config
cargo run -- simulate --device mouse --dy 1 --dx 2 --continuous false
cargo run -- enable
cargo run -- disable
cargo run -- toggle
cargo run -- run
cargo test
cargo fmt
cargo clippy -- -D warnings
```

`run` installs the macOS event tap. It requires:

- System Settings -> Privacy & Security -> Accessibility;
- System Settings -> Privacy & Security -> Input Monitoring.

For safe checks without installing the event tap, use `doctor`, `show-config`, and `simulate`.

## Config

Default path on macOS:

```text
~/Library/Application Support/Auto Reverse/config.toml
```

Override path for testing:

```bash
AUTO_REVERSE_CONFIG=/tmp/auto-reverse.toml cargo run -- doctor
```

Important fields:

```toml
config_version = 1
enabled = true
reverse_vertical = true
reverse_horizontal = false
reverse_mouse = true
reverse_trackpad = false
reverse_magic_mouse = true
reverse_unknown = false
discrete_scroll_step_size = 3
reverse_only_raw_input = false
```

Current limitation: `reverse_magic_mouse` is present for parity, but the live classifier cannot distinguish Magic Mouse from trackpad yet because both report continuous scroll through the current public event-tap signal.

## Architecture

Current modules:

```text
src/main.rs          CLI entrypoint
src/lib.rs           library facade
src/config.rs        config schema and storage
src/device.rs        device kind and conservative classifier
src/input.rs         normalized scroll event
src/scroll.rs        transform rules and CoreGraphics field helpers
src/permissions.rs   macOS permission checks
src/event_tap.rs     macOS event tap runtime
src/error.rs         shared errors
```

Next target split:

- move CoreGraphics field helpers out of `scroll.rs`;
- introduce `platform/macos`;
- introduce `app/runtime`;
- introduce `ui/menu_bar`, `ui/settings`, `ui/diagnostics`;
- introduce `telemetry/ring_buffer`.

## Three Iterations

### Iteration 1: Core Safety

Keep CLI and pure logic solid:

- finish config validation and migration plan;
- add more simulation flags;
- separate platform helpers from pure transform;
- keep tests fast and deterministic.

### Iteration 2: Product UX

Build the app surface:

- menu bar utility;
- settings window;
- permission onboarding;
- debug console;
- start at login;
- hide/show icon.

### Iteration 3: Release Quality

Make it releasable:

- Magic Mouse/trackpad distinction;
- wake recovery;
- packaging/signing;
- localization;
- update strategy;
- privacy/security review.

## Design Direction

This should feel like a compact native utility:

- no landing page;
- no decorative gradients;
- dense but calm settings;
- icon buttons for common actions;
- visible permission states;
- system dark/light mode;
- clear recovery actions for errors.

## Documents

- `architecture.md` - current and target architecture, SOLID/DRY split, UX direction.
- `recommendation.md` - 500 updated recommendations, problems and improvements.
- `scroll-reverser-parity.md` - Scroll Reverser feature parity checklist.
