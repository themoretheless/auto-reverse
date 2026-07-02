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
- LaunchAgent start at login via `enable-startup`/`disable-startup`;
- per-device rules: `[[device_rules]]` pins one exact mouse (vendor/product
  ID) on or off, attributed via an IOHIDManager wheel monitor; `devices`
  lists connected pointing devices with their IDs;
- CLI diagnostics and simulation;
- 37 unit tests (25 before the startup and per-device features).

Still missing:

- menu bar UI;
- preferences window;
- hide/show menu bar icon;
- debug console;
- gesture/HID classifier for Magic Mouse vs trackpad;
- packaging/signing/update flow.

## Commands

```bash
cargo build
cargo run -- doctor
cargo run -- devices
cargo run -- show-config
cargo run -- simulate --device mouse --dy 1 --dx 2 --continuous false
cargo run -- simulate --device mouse --dy 1 --vendor-id 0x046d --product-id 0xc54d
cargo run -- enable
cargo run -- disable
cargo run -- toggle
cargo run -- enable-startup
cargo run -- disable-startup
cargo run -- startup-status
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

`run` actually triggers both OS consent dialogs now (`AXIsProcessTrustedWithOptions` for Accessibility, `CGRequestListenEventAccess` for Input Monitoring), not just passive checks; `doctor` reports both permission states without prompting and prints the fix instructions when something is missing. An earlier experimental `SourceClassifier` (a touch-count/phase heuristic meant to separate Magic Mouse from trackpad) was removed as dead code: it was never wired into the real event tap (nothing in the codebase feeds it real touch data), and its own passing tests created false confidence that the distinction already worked. See `recommendation.md` for the full list of verified findings and fixes across 3 review iterations.

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

# Optional: pin one exact device regardless of the per-kind flags above.
# Run `auto-reverse devices` to see YOUR devices' IDs and paste them here -
# the values below are placeholders, not real hardware. Discrete wheels
# only; trackpad and Magic Mouse continuous scrolling cannot be attributed.
[[device_rules]]
vendor_id = 0x1234       # from `auto-reverse devices`
product_id = 0x5678      # from `auto-reverse devices`
name = "My mouse"        # optional, display only
reverse = false
```

Current limitation: `reverse_magic_mouse` is present for parity, but the live classifier cannot distinguish Magic Mouse from trackpad yet because both report continuous scroll through the current public event-tap signal.

## Start At Login

`enable-startup` installs a per-user LaunchAgent:

```text
~/Library/LaunchAgents/com.auto-reverse.agent.plist
```

That agent starts the current executable with the `run` argument on the next login. This works for the CLI build today. A future packaged `.app` can switch to `SMAppService`, but LaunchAgent keeps the feature real before the GUI exists.

## Architecture

Current modules, layered from pure logic down to platform code. Read them
top to bottom to learn the project in small pieces - each file has one
reason to change, and nothing above `platform/` imports an OS framework:

```text
src/main.rs                          CLI entrypoint and command dispatch
src/lib.rs                           library facade documenting the layering
src/error.rs                         shared AppError / AppResult
src/device.rs                        DeviceKind + conservative classifier
src/input.rs                         normalized ScrollEvent
src/scroll.rs                        pure reversal policy (no CoreGraphics)
src/config/mod.rs                    facade re-exporting AppConfig/ConfigStore
src/config/schema.rs                 what the settings ARE: fields, defaults, validation
src/config/store.rs                  where they LIVE: paths, TOML I/O, atomic save
src/platform/mod.rs                  cfg-gated platform adapters
src/platform/macos/mod.rs            macOS integration overview
src/platform/macos/scroll_events.rs  CGEvent field mapping (read event, write decision)
src/platform/macos/permissions.rs    Accessibility + Input Monitoring TCC calls
src/platform/macos/hid.rs            IOHIDManager wheel monitor (per-device attribution)
src/platform/macos/startup.rs        LaunchAgent start-at-login support
src/platform/macos/event_tap.rs      CGEventTap runtime loop
src/ui.rs                            egui settings window (gui feature)
```

The macOS framework crates (`core-foundation`, `core-graphics`) are
target-specific dependencies: the pure core compiles without them.

Next target split (future, GUI phase):

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
- `recommendation.md` - 560 recommendations, problems and improvements (500 base items + N01-N60 added after the startup feature).
- `scroll-reverser-parity.md` - Scroll Reverser feature parity checklist.
