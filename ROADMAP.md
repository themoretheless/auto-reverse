# Auto Reverse Roadmap

This is the executable view of the 860-item audit in `recommendation.md`.
Items are intentionally small enough to understand, implement, and verify
independently.

## P0 - Correctness and recovery

1. [Done] Replace inferred tap startup flags with a typed lifecycle channel.
2. [Done] Distinguish `AlreadyRunning`, `Running`, `Stopped`, and `Failed`.
3. [Done] Keep one consistent HID wheel snapshot per CGEvent.
4. [Done] Roll tray config changes back when persistence fails.
5. [Done] Add a process-local 15-minute pause with an explicit resume action.
6. [Next] Store structured debug reason enums instead of hot-path display strings.
7. [Next] Observe sleep/wake and re-arm a disabled or stopped tap safely.
8. [Next] Serialize external CLI and GUI config writes or detect stale revisions.
9. [Next] Focus the existing settings window when a second launch hits `ui.lock`.
10. [Next] Add integration tests with isolated `HOME` and config paths.

## P1 - Product and design

11. [Done] Match handoff 1b settings tabs and native compact control styling.
12. [Done] Match Concept B menu-bar arrows and status dot.
13. [Done] Match handoff 1e rich menu and 1f Debug Console.
14. [Done] Add a branded app icon and `.icns` bundle pipeline.
15. [Done] Route first launch with missing permissions to the Permissions tab.
16. [Done] Protect Restore defaults with an explicit confirmation step.
17. [Done] Open Accessibility and Input Monitoring panes independently.
18. [Next] Add Save Panel / Reveal in Finder for Debug Console exports.
19. [Next] Add stable device identity v2 using serial/location data where available.
20. [Research] Determine whether public APIs can distinguish Magic Mouse gestures.

## P2 - Distribution and maintenance

21. [Done] Run fmt, default/lean checks, clippy, tests, and bundle smoke in CI.
22. [Done] Add QA, design, privacy, security, and contribution documents.
23. [Next] Add Developer ID signing, hardened runtime, notarization, and stapling.
24. [Next] Add a release/install/uninstall workflow with stable bundle identity.
25. [Decision] Choose an update strategy before enabling update-related config flags.

## Definition of Done

A roadmap item is done only when behavior is implemented, relevant tests pass,
documentation is current, `cargo fmt --check`, both `cargo check` profiles,
`cargo clippy --all-targets -- -D warnings`, `cargo test`, and bundle smoke pass.
Visual/AppKit changes also need the matching manual rows in `QA.md` checked on a
real Mac; until then they are implemented but visually unverified.
