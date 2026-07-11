# Security

## Boundaries

Auto Reverse is a local, unsandboxed macOS input utility. Its highest-risk
boundary is the `CGEventTap` / IOHID / AppKit FFI under `src/platform/macos`.
Pure config, device, input, runtime-control, and scroll-policy modules do not
import operating-system frameworks.

The callback changes only documented scroll delta fields. It never writes key,
button, pointer-position, or application data. Synthetic/injected events can be
ignored with `reverse_only_raw_input`. Diagnostics are bounded and local-only.

Magic Mouse/trackpad classification uses a second listen-only session event tap
and public `NSEvent`/`NSTouch` APIs. It counts touching fingers but does not
modify gesture events. The raw FFI callback represents the AppKit-only gesture
event type as `u32` because the `core-graphics` Rust enum omits value 29; this
avoids constructing an invalid Rust enum discriminant. No private
MultitouchSupport framework or copied IOHID event SPI is used.

The installer stages updates beside the destination, validates bundle ID,
Mach-O, plist, icon, LSUIElement mode, and signature, then swaps paths on the
same volume. A failed final validation restores the previous bundle. Existing
or damaged copies are recognized by exact bundle identity before replacement;
symlink destinations and unexpected app names are refused. Process termination
matches the exact installed executable path, not a broad process-name pattern.
The uninstaller applies the same identity check before recursive removal and
does not delete user data without an explicit flag.

The second-launch `ui.activate` mailbox accepts only the PID of the process that
already owns `ui.lock`. A local process able to write the application-support
directory can at most request that the settings window come to the front; the
mailbox cannot change configuration, control the event tap, or claim ownership.
`flock` remains the single-instance authority.

## Reporting

Do not include exported debug logs or personal device names in a public issue.
Report a vulnerability privately to the repository owner with the affected
version, macOS version, reproduction steps, and whether a physical or injected
input source was involved.

## Release requirements

Public distribution still requires Developer ID signing, hardened runtime,
notarization, and stapling. The current local bundle is ad-hoc signed and is not a
production trust model.
