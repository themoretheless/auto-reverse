# Auto Reverse Release

This document is the single source of truth for direct macOS distribution.
Local ad-hoc bundles are useful for development, but they are not public
release artifacts.

## Trust Model

A production artifact must have all of these properties:

- a `Developer ID Application` signature;
- hardened runtime enabled with `--options runtime`;
- a secure Apple timestamp;
- no hardened-runtime exception entitlements unless the runtime proves one is
  necessary;
- an `Accepted` response from Apple's notary service;
- a stapled and validated notarization ticket;
- a successful Gatekeeper execution assessment.

`packaging/AutoReverse.entitlements` is intentionally empty. Auto Reverse is an
unsandboxed Accessibility utility; that user approval is managed by TCC and
does not require weakening hardened runtime.

Apple references:

- [Creating distribution-signed code for macOS](https://developer.apple.com/documentation/xcode/creating-distribution-signed-code-for-the-mac/)
- [Configuring the hardened runtime](https://developer.apple.com/documentation/xcode/configuring-the-hardened-runtime)
- [Notarizing macOS software before distribution](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)
- [Customizing the notarization workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow)

## One-Time Setup

Install a `Developer ID Application` certificate and verify that Keychain can
see it:

```bash
security find-identity -v -p codesigning
```

Store notary credentials in Keychain. Omitting credential flags keeps the
password out of shell history and lets `notarytool` prompt interactively:

```bash
xcrun notarytool store-credentials auto-reverse-notary
```

The profile can use an Apple ID plus app-specific password or an App Store
Connect API key. The release script accepts only the profile name; it never
accepts a password.

## Build A Release

Inspect the resolved paths and steps without changing files or contacting
Apple:

```bash
scripts/release-app-bundle.sh \
  --sign-identity "Developer ID Application: Name (TEAMID)" \
  --keychain-profile auto-reverse-notary \
  --plan
```

Run the production workflow:

```bash
scripts/release-app-bundle.sh \
  --sign-identity "Developer ID Application: Name (TEAMID)" \
  --keychain-profile auto-reverse-notary
```

Environment alternatives are `AUTO_REVERSE_SIGN_IDENTITY`,
`AUTO_REVERSE_NOTARY_PROFILE`, and `AUTO_REVERSE_DIST_DIR`.

The script:

1. Builds `target/release/Auto Reverse.app`.
2. Signs it with the chosen identity, least-privilege entitlements, hardened
   runtime, and a secure timestamp.
3. Rejects ad-hoc, Apple Development, and other non-Developer-ID signatures.
4. Creates a `ditto` ZIP, submits it with `notarytool --wait`, and requires
   `Accepted`.
5. Saves the result plist and downloads the JSON audit log when a submission ID
   is available.
6. Staples and validates the ticket, then runs `spctl` Gatekeeper assessment.
7. Creates a new ZIP from the stapled app and writes its SHA-256.

The prior ZIP for the same version is not replaced until every release gate
passes. A failed submission preserves its result/log while leaving the
previous distributable untouched.

Default outputs:

```text
target/dist/Auto-Reverse-<version>-macOS.zip
target/dist/Auto-Reverse-<version>-macOS.zip.sha256
target/dist/Auto-Reverse-<version>-macOS.notary.plist
target/dist/Auto-Reverse-<version>-macOS.notary.json
```

## Release Checklist

- The worktree is clean and the release commit is pushed.
- `Cargo.toml` version and release notes are final.
- `scripts/check-release-workflow.sh` and the complete gate in `QA.md` pass.
- `security find-identity` shows the intended Developer ID certificate.
- The `notarytool` Keychain profile validates.
- The production release script finishes without a skipped step.
- The notary result says `Accepted`; its JSON log has been reviewed.
- `scripts/check-app-bundle.sh --require-notarized --app <path>` passes.
- Gatekeeper accepts a quarantined copy on a clean supported Mac.
- Accessibility, login item, scrolling, and uninstall rows in
  `QA.md` pass on the exact stapled artifact.
- The final ZIP and SHA-256 are published together.

## Current Verification Boundary

On 2026-07-12 this development Mac had working `codesign`, `notarytool`, and
`stapler`, but Keychain exposed only an `Apple Development` identity. The
automated smoke proved the hardened ad-hoc path, ad-hoc release-gate rejection,
and plan purity; a separate local signing pass proved that Apple Development is
also rejected. Adversarial review fixed prior-artifact preservation and a
strict-check bypass. A real Developer ID submission, ticket stapling, and
clean-machine Gatekeeper/TCC continuity remain manual release QA until the
production certificate and notary profile are provisioned.
