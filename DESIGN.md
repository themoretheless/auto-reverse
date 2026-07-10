# Auto Reverse Design

The selected source is the Claude Design handoff `Auto Reverse - UI
Design.dc.html`. Production code follows these variants:

- settings: `1b`, segmented General / Devices / Permissions tabs;
- menu-bar icon: `1c`, Concept B opposing arrows with a separate status dot;
- menu: `1e`, native rich menu;
- diagnostics: `1f`, live Debug Console.

## Product character

Auto Reverse is a quiet macOS utility. The first screen is the working settings
surface, never a landing page. Layout stays compact, uses native typography,
avoids nested cards, and keeps status plus the master toggle visible across all
tabs. Errors sit next to the state they affect and always offer a recovery
action when one exists.

## Tokens

| Role | Light | Dark |
| --- | --- | --- |
| Accent | `#2F6FE4` | `#5B93FF` |
| Active | `#34A853` | `#34C759` |
| Warning | `#E59E2F` | `#FF9F0A` |
| Primary text | `#1D1D1F` | `#F2F2F3` |
| Control surface | `#FFFFFF` | `#2C2C2E` |
| Control border | `#C7C7CC` | `#48484A` |
| Muted text | `#8E8E93` | `#9A9AA0` |

The implementation lives in `src/ui/theme.rs`. Controls use 4-8 px radii,
zero letter-spacing, stable dimensions, and SF Pro / SF Mono when available.

Debug export uses the native macOS Save Panel instead of an app-defined folder.
Cancel is silent. A successful export shows one compact, single-line receipt;
the filename truncates before the stable `Reveal in Finder` action, while the
full path remains available on hover. Export and Reveal errors are distinct and
stay inline with that action area.

## States

- Active: reversal is enabled, permissions are ready, and no temporary pause exists.
- Paused: persistent reversal is off.
- Temporarily paused: settings stay enabled but events pass through until the timer ends.
- Needs permission: Accessibility or Input Monitoring is missing.
- Error: tap, tray, login-item, config-load, or config-save failure is visible inline.

## Icon system

The menu-bar glyph is a template `NSImage`, so AppKit owns light/dark tinting.
Its colored status dot is a separate non-template `NSImageView`. The app icon
uses the same opposing-arrow geometry, the accent blue, and the active green,
rendered from `assets/AppIcon.svg` into `AutoReverse.icns` during bundle build.
