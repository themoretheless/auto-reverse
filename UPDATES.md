# Update Policy

Auto Reverse uses an explicit manual update strategy. The application contains
no version-checking client, appcast reader, telemetry endpoint, download
service, or background network task.

## User Actions

- **Latest release** opens the canonical stable destination:
  `https://github.com/themoretheless/auto-reverse/releases/latest`.
- **All releases** opens the complete releases list, including prereleases:
  `https://github.com/themoretheless/auto-reverse/releases`.
- The egui Advanced tab and `open-releases --latest|--all` use the same pure
  `ReleaseChannel` constants and the same narrow macOS browser adapter.
- `/usr/bin/open` receives only those compile-time trusted URLs. No config or
  user-supplied string can become an arbitrary browser destination.

Without an explicit CLI flag, `include_beta_updates=true` selects **All
releases** as the manual destination. The legacy `check_for_updates` field is
retained for config compatibility but never initiates a request. `doctor`
reports both facts instead of implying an automatic updater exists.

## Why Manual

GitHub documents `/releases/latest` as the stable link for the most recent
non-prerelease release and `/releases` as the list of all releases. This gives
Auto Reverse deterministic stable and prerelease destinations without adding a
network stack to an input utility: [GitHub release links](https://docs.github.com/en/repositories/releasing-projects-on-github/linking-to-releases).

Sparkle is a reasonable future option, but its update feed, archive signatures,
and release process are a new security boundary rather than a checkbox. Its
documentation requires an appcast and supports EdDSA archive signatures:
[Sparkle documentation](https://sparkle-project.org/documentation/). Public
distribution must also complete Developer ID signing and Apple's notarization
workflow: [Apple notarization](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution).

## Automatic-Updater Gate

Do not enable automatic checks until all of these are designed and verified:

1. Developer ID signing, hardened runtime, notarization, stapling, and
   Gatekeeper validation on the exact release artifact.
2. HTTPS appcast ownership plus signed archives and protected signing keys.
3. Stable/prerelease channel semantics, downgrade prevention, staged rollout,
   and rollback behavior.
4. Atomic install continuity for Accessibility and `SMAppService` identity.
5. Explicit privacy documentation, failure UI, offline behavior, and
   clean-machine QA.

Until then, manual browser navigation is the complete update feature, not a
temporary hidden automatic checker.
