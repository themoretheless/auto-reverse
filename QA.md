# Auto Reverse QA

Automated checks prove code and bundle structure. They do not prove that a
physical wheel feels correct or that AppKit pixels are visible on every menu bar.

## Automated

| Check | Command | Status |
| --- | --- | --- |
| Formatting | `cargo fmt --check` | Required |
| Default GUI compile | `cargo check` | Required |
| Lean CLI compile | `cargo check --no-default-features` | Required |
| Lints | `cargo clippy --all-targets -- -D warnings` | Required |
| Tests | `cargo test` | Required |
| Bundle | `scripts/build-app-bundle.sh` | Required |
| Bundle structure | `scripts/check-app-bundle.sh` | Required |

## Manual macOS matrix

Mark these on the exact bundle intended for release. Blank means not verified.

| Date | macOS | Device / scenario | Light | Dark | Result | Tester |
| --- | --- | --- | --- | --- | --- | --- |
|  |  | Discrete vertical wheel |  |  |  |  |
|  |  | Horizontal / tilt wheel |  |  |  |  |
|  |  | Built-in trackpad natural scrolling |  |  |  |  |
|  |  | Magic Trackpad |  |  |  |  |
|  |  | Magic Mouse documented fallback |  |  |  |  |
|  |  | Per-device Reverse / Don't reverse |  |  |  |  |
|  |  | Pause 15 minutes / Resume now |  |  |  |  |
|  |  | Missing then granted permissions |  |  |  |  |
|  |  | Start at Login after reboot |  |  |  |  |
|  |  | Sleep/wake: hidden app re-arms live tap or restarts one stopped tap |  |  |  |  |
|  |  | Cmd-W, Cmd-Q, Dock Quit, tray Quit |  |  |  |  |
|  |  | Hide window, launch app again: one icon and focused existing window |  |  |  |  |
|  |  | Menu, device submenu, Option-click console |  |  |  |  |
|  |  | Debug search/filter/export/clear |  |  |  |  |
|  |  | Finder and System Settings app icon |  |  |  |  |

Also verify high contrast, larger text, remote desktop with raw-input guard,
Notification Center/system gestures, and two simultaneously connected mice.
