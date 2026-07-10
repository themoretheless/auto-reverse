# Security

## Boundaries

Auto Reverse is a local, unsandboxed macOS input utility. Its highest-risk
boundary is the `CGEventTap` / IOHID / AppKit FFI under `src/platform/macos`.
Pure config, device, input, runtime-control, and scroll-policy modules do not
import operating-system frameworks.

The callback changes only documented scroll delta fields. It never writes key,
button, pointer-position, or application data. Synthetic/injected events can be
ignored with `reverse_only_raw_input`. Diagnostics are bounded and local-only.

## Reporting

Do not include exported debug logs or personal device names in a public issue.
Report a vulnerability privately to the repository owner with the affected
version, macOS version, reproduction steps, and whether a physical or injected
input source was involved.

## Release requirements

Public distribution still requires Developer ID signing, hardened runtime,
notarization, and stapling. The current local bundle is ad-hoc signed and is not a
production trust model.
