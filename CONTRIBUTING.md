# Contributing

Keep changes small and preserve the dependency direction documented in
`architecture.md`: pure policy must not import CoreGraphics or AppKit, and all
macOS FFI stays under `src/platform/macos`.

Before opening a change, run:

```bash
cargo fmt --check
cargo check
cargo check --no-default-features
cargo clippy --all-targets -- -D warnings
cargo test
scripts/build-app-bundle.sh
scripts/check-app-bundle.sh
```

Tests should reproduce the behavior being fixed, especially for scroll-field
mapping, config rollback, lifecycle transitions, and device-rule precedence.
Black-box CLI tests must use a unique temporary `HOME` and clear inherited
config, LaunchAgent, and XDG path overrides before spawning the binary.
Visual changes must update `DESIGN.md` when they change the selected handoff and
must add or complete the matching manual row in `QA.md`.
