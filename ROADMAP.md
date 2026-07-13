# Auto Reverse Roadmap

This is the executable view of the 960-item audit in `recommendation.md`.
Items are intentionally small enough to understand, implement, and verify
independently.

The research-derived `R01-R60` queue is analyzed in `RESEARCH.md` and executed
below in fixed batches of five.

## Research batches

| Batch | Items | Status |
| --- | --- | --- |
| 1 | R01-R05 | Implemented; native privacy-trace Save Panel QA remains |
| 2 | R06-R10 | Implemented; physical benchmark and live latency UI QA remain |
| 3 | R11-R15 | Implemented; six-device physical and live latency UI QA remain |
| 4 | R16-R20 | Implemented in pure model; live runtime remains unchanged |
| 5 | R21-R25 | Next |
| 6 | R26-R30 | Pending |
| 7 | R31-R35 | Pending |
| 8 | R36-R40 | Pending |
| 9 | R41-R45 | Pending |
| 10 | R46-R50 | Pending |
| 11 | R51-R55 | Pending |
| 12 | R56-R60 | Pending |

## P0 - Correctness and recovery

1. [Done] Replace inferred tap startup flags with a typed lifecycle channel.
2. [Done] Distinguish `AlreadyRunning`, `Running`, `Stopped`, and `Failed`.
3. [Done] Keep one consistent HID wheel snapshot per CGEvent.
4. [Done] Roll tray config changes back when persistence fails.
5. [Done] Add a process-local 15-minute pause with an explicit resume action.
6. [Done] Store structured debug reason enums instead of hot-path display strings.
7. [Implemented] Observe sleep/wake and re-arm a disabled or stopped tap safely; real sleep/wake manual QA remains open.
8. [Done] Serialize CLI writes with a persistent lock; reject and reload stale GUI/tray revisions.
9. [Implemented] A second GUI launch activates the existing window; hidden-window manual QA remains open.
10. [Done] Run black-box CLI integration tests in isolated `HOME` and config paths.

## P1 - Product and design

11. [Done] Match handoff 1b settings tabs and native compact control styling.
12. [Done] Match Concept B menu-bar arrows and status dot.
13. [Done] Match handoff 1e rich menu and 1f Debug Console.
14. [Done] Add a branded app icon and `.icns` bundle pipeline.
15. [Done] Route first launch with missing permissions to the Permissions tab.
16. [Done] Protect Restore defaults with an explicit confirmation step.
17. [Done] Require and open only Accessibility; remove the unnecessary Input Monitoring gate.
18. [Implemented] Native Save Panel and Finder reveal are built; manual panel workflow QA remains open.
19. [Implemented] Serial-first device identity with location fallback is built; identical-device/reconnect manual QA remains open.
20. [Implemented] Public IOHID inventory identifies an exclusive trackpad or
    Magic Mouse; the listen-only AppKit timing heuristic handles the `Both`
    case. Physical dual-device and rapid-alternation QA remains open.

## P2 - Distribution and maintenance

21. [Done] Run fmt, default/lean checks, clippy, tests, and bundle smoke in CI.
22. [Done] Add QA, design, privacy, security, and contribution documents.
23. [Implemented] Developer ID signing, hardened runtime, secure timestamps,
    notarization, stapling, Gatekeeper assessment, audit logs, and checksummed
    artifacts are scripted and smoke-tested; a real Developer ID submission and
    clean-machine release QA remain open because this Mac has only an Apple
    Development certificate.
24. [Implemented] Atomic release install/update and identity-checked uninstall
    scripts are covered by an isolated workflow smoke; a running `/Applications`
    update is verified, while fresh install, login-item cleanup, full uninstall,
    and permission continuity QA remains open.
25. [Next] Choose an update strategy before enabling update-related config flags.

## Definition of Done

A roadmap item is done only when behavior is implemented, relevant tests pass,
documentation is current, `cargo fmt --check`, both `cargo check` profiles,
`cargo clippy --all-targets -- -D warnings`, `cargo test`, and bundle smoke pass.
Visual/AppKit changes also need the matching manual rows in `QA.md` checked on a
real Mac; until then they are implemented but visually unverified.
