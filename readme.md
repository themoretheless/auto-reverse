# Auto Reverse

Auto Reverse is a Rust/macOS utility for reverse scrolling. The target product is feature parity with Scroll Reverser: keep a trackpad natural, reverse a wheel mouse, expose clear settings, and never make system input feel mysterious.

## Current Status

Implemented:

- macOS `CGEventTap` for scroll events;
- TOML config with `config_version`, a persistent cross-process lock, private
  mode `0600`, durable temp-file and directory sync, atomic CLI transactions,
  exact-revision conflict detection for GUI/tray saves, read-only text/JSON
  validation, and explicit corrupted-config repair with exact backup/rollback;
  Advanced can export versioned TOML or securely review/import a <=256 KiB
  regular non-symlink, non-world-writable file with v0->v1 migration reporting
  and a field-level diff limited to changed sections;
- global enable/disable;
- vertical and horizontal reverse flags;
- mouse, trackpad, Magic Mouse and unknown-device config flags;
- independent live mouse, trackpad, and Magic Mouse policies: public IOHID
  inventory resolves an exclusive continuous source, while a listen-only
  AppKit gesture tap and pure timing state machine handle the `Both` case;
- wheel step size;
- raw-input guard through `source_pid`;
- Accessibility check and targeted first-run request; Input Monitoring is not
  separately required because Accessibility already grants event listening;
- local macOS `.app` bundle for Privacy & Security;
- a production release pipeline for Developer ID signing, hardened runtime,
  secure timestamping, `notarytool`, stapling, Gatekeeper assessment, and a
  checksummed final ZIP;
- LaunchAgent start at login via `enable-startup`/`disable-startup`;
- per-device rules: `[[device_rules]]` can inherit, reverse, or not reverse one
  physical wheel mouse and may independently set an alias, override wheel step
  size, or select a future smooth preset, using
  vendor/product plus a serial number when available, otherwise its IOKit
  connection location; old vendor/product-only rules remain model-wide;
- field-by-field profile resolution is fixed as exact serial, exact location,
  VID/PID hardware, device kind, then global default; no separate profile
  database is maintained;
- a pure device catalog separates Connected, Remembered, and Unavailable HID
  services; aliases are bounded and duplicate names get stable identity suffixes;
  connected serial/location identities can run a five-second local "Test this
  device" check that shows the effective direction/rule source and cannot be
  satisfied by another identical model;
- last-active HID wheel attribution is confidence-scored and expires after 50
  ms, so stale or missing observations cannot lend their identity to a later event;
- one pure input-provenance resolver owns hardware, posted/injected,
  self-synthetic, virtual-HID, and unknown-HID bypass precedence; an Advanced
  toggle controls posted/remote input without duplicating runtime policy;
- observed public HID transport `Virtual` and unknown/missing transport fail
  open without changing the event, with explicit Debug Console/trace reasons;
- egui settings window (`ui`, default `gui` feature); opening it starts the
  scroll event tap in the same process when enabled and permissions are ready,
  deduped against any other already-running tap via `daemon_lock`;
- single-instance GUI activation: a second launch exits without another tray
  icon and asks the existing process to reveal and focus its settings window;
- menu bar UI with a custom opposing-arrows template icon, a separate colored
  status dot, a rich native menu, a Reverse Scrolling toggle, per-device
  quick-pick submenu, temporary pause/resume, Open Settings, Open Debug Console,
  and Quit; Advanced can hide the status item without stopping reversal, while
  relaunch and `show-menu-bar-icon` provide recovery without a second instance;
- local Debug Console with search, decision filters, clear, and a bounded
  structured ring buffer; CSV export includes stable reason codes, source PID,
  synthetic flag, device kind, raw HID name, vendor/product IDs, attribution,
  classifier evidence, input provenance, and resolved profile sources, while
  serial/location qualifiers stay out of automatic exports; a native Save Panel
  chooses the destination and the receipt can reveal it in Finder; the same
  Export menu creates a versioned privacy trace with relative monotonic time and
  no process/device identity; Copy summary emits only aggregate runtime,
  permission, configuration, outcome, device-kind, and reason counts, with no
  raw trace, PID, timestamp, delta, hardware identifier, or target app/window data;
- deterministic pure trace replay and `trace-lab`, which reports magnitude,
  interval, direction, duration, clutch sessions, per-axis distance and an
  always-present constant-gain baseline without changing live scrolling;
- a local ScrollTest-style benchmark opened from Debug Console: known and
  unknown target sessions, compact/full distance x viewport x tolerance
  matrices, six explicit physical input classes, 66 ms settled completion,
  movement time, switchbacks, maximum overshoot, and atomic per-trial CSV
  export;
- observed event-rate p50/p95/max plus histogram bins per device class, with
  idle gesture gaps excluded and no claim that observed delivery equals
  advertised polling rate;
- an explicit on-demand min/average/max event-tap latency interval snapshot via
  public `CGGetEventTapList`; it is never polled because reading resets min/max;
  bounded history warns only after repeated callback-budget breaches;
- a pure, non-live two-axis discrete-wheel dynamics model with exact continuous
  bypass, independent velocity/residual/momentum, 1-50 ms input-time bounds,
  a median 3-of-8 observed-rate window, signed-distance ledger, and measurable
  Off/Precise/Balanced/Fast parameters; direction/gap sessions, subpixel stop,
  and explicit click/action cancellation are tested, while canceled distance
  remains visible in the ledger; the model has no CoreGraphics, timer, thread,
  or config I/O dependency;
- a 15-second local Off/Precise/Balanced/Fast preset preview with immediate/tail
  model bars; selection is never saved or shared with the tap until `Use preset`,
  and expiry, Revert, or an external config change restores the committed value;
- a non-live pure scheduler safety contract with unique wake ids, two-axis
  session generations, due-anchored 8 ms sample TTL, mandatory synthetic
  provenance, idle teardown, and latched fail-open; macOS normalization
  recognizes the public `kCGEventSourceUserData` self marker before policy;
- process-local 15-minute pause that leaves persisted settings untouched;
- typed event-tap lifecycle with explicit started/already-running/stopped/failed
  events rather than timeout-inferred booleans;
- notification-led permission/device refresh through app activation, workspace
  wake, and IOHID match/removal generations, with one coalescing 30-second timer
  only as a backstop; disabled launch performs no TCC prompt or permission nag;
- GUI sleep/wake recovery plus a public `CGEventTapIsEnabled` watchdog: two
  unhealthy one-second samples are required before rearm/restart, automatic
  recovery is capped at three attempts, and exhaustion is visible;
- a 64-record process-local recovery audit keeps wake, exact tap timeout,
  tap-disabled-by-input, watchdog, and permission-loss attempts independent;
  Debug Console and aggregate diagnostics expose only typed reason/action/counts;
- separate confirmed `Reset this device`, `Reset dynamics`, and `Restore all
  defaults` scopes; full restore unregisters the GUI login item only after the
  default config saves successfully;
- fuzzy settings search routes directly to General, Devices, Permissions,
  Advanced, or Diagnostics; common direction/device controls stay on the main
  tabs while input policy, config transfer, and diagnostic tools remain separated;
- an explicit manual update policy: About & Updates and `open-releases` open
  canonical stable/all GitHub release pages only after a user action; Auto
  Reverse itself performs no background version or network requests;
- branded opposing-arrows app icon and generated Retina `.icns` in the bundle;
- GUI Start at Login toggle via `SMAppService.mainAppService()`;
- CLI diagnostics, JSON startup status and simulation;
- separated CLI parser in `src/cli.rs`;
- black-box CLI integration tests with isolated `HOME`, explicit config paths,
  no-create diagnostics, startup plist paths, and concurrent mutations;
- historical config fixtures cover every supported v0 root/device-rule key and
  a catalog of rejected typo keys; five deterministic 512-seed property suites
  exercise trace/config parsers, migration round-trips, and dynamics invariants;
- an enforced PR-01..PR-06 release matrix covers Safari zoom, Launchpad,
  Catalyst/iOS apps, Universal Control, iPhone Mirroring, and remote desktop;
  production release requires build, `Pass`, and evidence in every row;
- dynamics stays fail-closed behind a runtime kill switch, source/manifest
  parity and measurable release thresholds; `rollback-dynamics` atomically
  clears only smooth presets without changing directions, aliases, or step size;
- macOS CI plus roadmap, design, QA, privacy, security, and contribution docs.

Still missing:

- guided onboarding beyond the compact permission-first state;
- a provisioned Developer ID/notary account and clean-machine release QA;
- an automatic signed in-app updater, intentionally deferred while the manual
  no-background-network strategy is sufficient;
- physical-device/manual visual validation of the new benchmark and live tap
  latency snapshot;
- platform timer/posting adapter, runtime opt-in, cancellation hooks, and
  physical acceptance for the experimental dynamics model;
- automatic in-place config migration remains intentionally separate from the
  reviewed importer;
- recorded manual results for the six platform regression rows on the exact
  stapled release candidate.

## Commands

```bash
cargo build
scripts/build-app-bundle.sh
scripts/check-release-workflow.sh
scripts/release-app-bundle.sh --sign-identity "Developer ID Application: Name (TEAMID)" --keychain-profile auto-reverse-notary --plan
scripts/install-app-bundle.sh
scripts/check-install-workflow.sh
cargo run -- doctor
cargo run -- doctor --no-create
cargo run -- validate-config --json
cargo run -- repair-config
cargo run -- open-releases --latest
cargo run -- open-releases --all
cargo run -- devices
cargo run -- ui
cargo run -- show-config
cargo run -- simulate --device mouse --dy 1 --dx 2 --continuous false
cargo run -- simulate --device mouse --dy 1 --vendor-id 0x046d --product-id 0xc54d
cargo run -- simulate --device mouse --dy 1 --vendor-id 0x046d --product-id 0xc54d --serial-number ABC123
cargo run -- trace-lab /path/to/scroll-trace.toml --baseline-gain 1 --clutch-gap-ms 150
cargo run -- benchmark
cargo run -- enable
cargo run -- disable
cargo run -- toggle
cargo run -- enable-startup
cargo run -- disable-startup
cargo run -- show-menu-bar-icon
cargo run -- startup-status
cargo run -- startup-status --json
cargo run -- rollback-dynamics
cargo run -- run
cargo test
cargo test --test cli_integration
cargo fmt
cargo clippy -- -D warnings
scripts/check-app-bundle.sh
```

`run` installs an active macOS event tap that observes and modifies scroll
events. It requires:

- System Settings -> Privacy & Security -> Accessibility.

Input Monitoring is not a second requirement. Apple DTS confirms that
Accessibility grants both event posting and listening, while Input Monitoring
grants listening only. Requiring both caused a false `NEEDS PERMISSION` state
even when macOS had already enabled the app.

For safe checks without installing the event tap, use `doctor`, `startup-status`,
`validate-config`, `show-config`, `simulate`, and `trace-lab`.
`validate-config` never creates a directory, config, or lock; `doctor
--no-create` and `trace-lab` report against defaults without creating a config
when none exists. `repair-config` is intentionally explicit: it preserves an
invalid regular file in a unique `.broken...toml` sibling before writing safe
defaults and restores the original if replacement fails. The trace format and
privacy boundary are documented in `TRACE.md`; update behavior is specified in
`UPDATES.md`.

Interactive measurement lives under **Debug Console -> Benchmark...** and
**Observed input metrics**. `BENCHMARK.md` defines the target conditions,
physical matrix, completion rule, CSV fields, observed-rate boundary, and the
side-effecting interval latency sample. `DYNAMICS.md` defines the latency
budgets, smooth-scrolling contract, preset parameters, and the boundary that
keeps the experiment out of live input.

## App Bundle

Build a local bundle:

```bash
scripts/build-app-bundle.sh
```

Default output:

```text
target/debug/Auto Reverse.app
```

The bundle contains `Contents/Resources/AutoReverse.icns`; the build script
generates its complete Retina iconset from `assets/AppIcon.svg`, signs local
builds ad-hoc with hardened runtime and the least-privilege entitlement file,
and validates the Mach-O/plist/icon/signature structure. Ad-hoc still means
development-only: it has no stable public signing identity or notarization
ticket.

For a stable local TCC identity across rebuilds, use an Apple Development
certificate explicitly:

```bash
scripts/install-app-bundle.sh \
  --development-sign-identity "Apple Development: Name (TEAMID)"
```

This remains a development-only artifact and cannot pass the Developer ID
release gate.

For a stable daily path, build the release profile, atomically install it to
`/Applications`, and launch it:

```bash
scripts/install-app-bundle.sh
```

Use `--debug`, `--no-open`, or `--destination /path/to/Auto\ Reverse.app` for
development. An update first validates the source and existing bundle identity,
stages a complete copy beside the destination, stops only processes whose
command begins with that exact installed Mach-O path, swaps directories on the
same volume, and restores the previous copy if final validation fails. It never
merges new files into an old bundle.

Uninstall the app while preserving config and per-device rules:

```bash
scripts/uninstall-app-bundle.sh
```

The uninstaller invokes the bundled `prepare-uninstall` command to remove both
the `SMAppService` GUI login item and CLI LaunchAgent before deleting the
identity-checked app. Add `--remove-user-data` only when the local config,
runtime lock files, and `~/Library/Logs/auto-reverse.log` should also be erased.
The install path and bundle identifier are stable, but a local ad-hoc signature
is still content-dependent. Use the production workflow below for public
artifacts; TCC continuity still needs verification with the provisioned
Developer ID certificate on the exact release bundle.

### Production Signing And Notarization

The release path is separate from the convenient local installer. First store
notary credentials in Keychain, then inspect the side-effect-free plan:

```bash
xcrun notarytool store-credentials auto-reverse-notary
scripts/release-app-bundle.sh \
  --sign-identity "Developer ID Application: Name (TEAMID)" \
  --keychain-profile auto-reverse-notary \
  --plan
```

Remove `--plan` to build, Developer-ID sign, timestamp, upload, require an
`Accepted` response, download the audit log, staple and validate the ticket,
run Gatekeeper assessment, and create
`target/dist/Auto-Reverse-<version>-macOS.zip` plus its SHA-256. The script
rejects ad-hoc and Apple Development signatures and does not replace a previous
good ZIP if a new submission fails. It accepts only a Keychain profile, never a
password.

The complete prerequisites, trust boundary, output contract, and manual
release checklist live in `RELEASE.md`. This machine currently has only an
Apple Development certificate, so the orchestration and strict gates are
verified but a real Apple notarization submission remains external release QA.

Use that `.app` in macOS:

- System Settings -> Privacy & Security -> Accessibility -> add `target/debug/Auto Reverse.app`;

Then launch the bundled app:

```bash
open "target/debug/Auto Reverse.app"
```

Double-clicking the bundle opens the settings window (`ui`), which also starts the scroll event tap on a background thread in this same process when `enabled=true` in the config and Accessibility is granted, sharing one live config with the window so changes made in that window apply immediately with no restart. If the app was opened before Accessibility was granted, it keeps watching the permission state and retries starting the tap once the check becomes ready; if startup failed or stopped immediately, turning Reverse scrolling off clears that pending attempt so turning it on again can retry cleanly. The default menu-bar item uses an opposing-arrows template glyph plus a separate colored status dot for active/paused/permission-blocked states. Its native menu includes Reverse Scrolling, device quick-picks, Open Settings, Open Debug Console, and Quit; holding Option while opening the icon opens the Debug Console directly. Advanced can hide the icon without stopping reversal. While hidden, reopening Auto Reverse focuses the existing settings process and reloads its exact external config revision; `show-menu-bar-icon` provides the same recovery from Terminal. Closing the settings window hides it rather than quitting. A separate `ui.lock` prevents duplicate windows/menu-bar icons. When a second GUI launch finds that lock held, it atomically writes a PID-addressed `ui.activate` request and exits with success; the existing process consumes the request on its hidden-window tick, reloads a newer config, makes the settings viewport visible, and focuses it. An exclusive tap lock (`platform::macos::daemon_lock`) still guards tap installation, so this in-process tap and a separately started `run` (manual, or via a LaunchAgent) can never both hold a live event tap - whichever gets there first wins, and the other observes the lock held and does nothing. External CLI edits made while the settings window is already open are not continuously watched. They are nevertheless protected: activation reloads them, while the next GUI/tray save detects an exact TOML revision mismatch and asks the user to repeat the local action instead of silently overwriting it. For terminal diagnostics through the bundled identity:

Debug Console rows keep raw source metadata in memory and derive display text
only while the console is searching or rendering. Export preserves the raw HID
name in its own escaped CSV field while normalizing whitespace only in the
human-readable `device` column. The Save Panel writes only after the user picks
a destination, uses atomic replacement, and offers Reveal in Finder after
success. Cancel does not create a file or erase the previous receipt. Debug data
remains local to this Mac.

**Copy summary** is a separate privacy-bounded path, not a shortened CSV. Its
typed formatter receives no names, serial/location values, hardware IDs,
process IDs, timestamps, deltas, target applications, or windows; it can only
copy aggregate state and decision counts. Exact identity retained in memory for
the Devices test is therefore unavailable to this formatter and both trace
exports.

The bundle uses the real Mach-O binary as `CFBundleExecutable`
(`Contents/MacOS/auto-reverse`) rather than a shell launcher. With no
arguments, that binary detects it is running inside `.app` and opens `ui`;
explicit CLI arguments still work through the same bundled executable.

```bash
"target/debug/Auto Reverse.app/Contents/MacOS/auto-reverse" doctor --no-create
```

## Config

`run` triggers the documented Accessibility consent dialog through
`AXIsProcessTrustedWithOptions`, not just a passive check; `doctor` reports the
required Accessibility state without prompting and prints the exact recovery
path when it is missing. It deliberately does not request Input Monitoring:
the active tap already receives listening access through Accessibility. Magic
Mouse/trackpad classification is wired into the real runtime: a separate
listen-only `NSEventTypeGesture` tap counts touching fingers through public
AppKit APIs, and `src/device_classifier.rs` turns those observations plus the
public momentum phase into a device kind. No private MultitouchSupport API is
used. See `recommendation.md` for the verified three-pass implementation and
review record.

Default path on macOS:

```text
~/Library/Application Support/Auto Reverse/config.toml
```

Override path for testing:

```bash
AUTO_REVERSE_CONFIG=/tmp/auto-reverse.toml cargo run -- doctor
```

Config writes use a persistent sibling lock file (`config.toml.lock`). CLI
read-modify-write commands hold that lock for the complete transaction; the
long-lived GUI and tray additionally compare the exact TOML revision they
loaded before replacing the file. The lock file is intentionally not deleted,
because replacing its inode would allow two processes to believe they both own
the lock.

The Advanced tab can export the validated current schema and import it through
a review rather than writing immediately. Import accepts at most 256 KiB of
UTF-8 from a regular file, rejects symlinks and world-writable sources, and
opens with `O_NOFOLLOW` before checking file identity, length, mode, and
modification time across the read. A missing
or zero version migrates to v1 with an explicit report; a future version or an
unknown current-schema key is rejected instead of silently discarded. The
dry-run lists only changed General, Devices, Startup, and Advanced fields, is
rebased if local settings move while open, and applies only those reviewed
sections through the normal exact-revision save path.

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
smooth_preset = "off" # schema only; live dynamics remains disabled
reverse_only_raw_input = false

# Optional: pin one physical device regardless of the per-kind flags above.
# Run `auto-reverse devices` to see YOUR devices' IDs and paste them here -
# the values below are placeholders, not real hardware. Discrete wheels
# only; trackpad and Magic Mouse continuous scrolling cannot be attributed.
[[device_rules]]
vendor_id = 0x1234       # from `auto-reverse devices`
product_id = 0x5678      # from `auto-reverse devices`
serial_number = "ABC123" # preferred when the device exposes one
name = "My mouse"        # optional, display only
alias = "Desk mouse"     # optional user-facing name
reverse = false          # omit to inherit direction
step_size = 5            # optional; inherits global step when omitted
smooth_preset = "precise" # optional; non-live until dynamics gates pass
```

Use `location_id = 0x12345678` instead of `serial_number` when `devices`
reports no usable serial. A location rule distinguishes identical devices on
their current ports, but moving the device to another USB port can change that
value. Omitting both qualifiers preserves the old model-wide behavior and
matches every device with that vendor/product pair. The settings UI shows only
a bounded serial suffix, keeps inherited model-wide rules explicit, and never
removes a sibling device's shared fallback when one exact rule is edited.
Some low-cost firmware reports the same placeholder serial for every unit; use
the printed `location_id` manually in that case.

Each profile field resolves independently. A serial rule can provide only a
step size or alias while inheriting direction and a smooth preset from its
location, hardware, kind, or global fallback. The settings control exposes
Inherit / Reverse / Don't reverse, and changing it never deletes the other
fields. `smooth_preset` is deliberately stored but not connected to the event
tap yet.

The Devices tab and `auto-reverse devices` use the same pure catalog. Connected
devices have a stable identity and can be edited, Remembered profiles remain
visible while unplugged, and Unavailable services are shown without unsafe
profile controls. Repeated HID services for one identity collapse into one row.
For a connected serial- or location-qualified wheel, **Test this device** waits
five seconds for a later physical discrete event with that exact identity and
shows the active Reverse/Don't reverse source beside the result. Posted input,
continuous gestures, old buffered events, and another serial do not satisfy the
test. If only model-wide VID/PID is known, exact testing is disabled rather than
claiming which identical unit moved.

For attributed discrete wheels, Auto Reverse reads the public IOHID
`Transport` from the same snapshot as identity. Exact `Virtual`, an unknown
value, or a missing value passes through untouched. No snapshot is a distinct
`NotObserved` state and preserves the existing kind policy. Apple permits a
virtual device to advertise a non-virtual transport, so this is a conservative
compatibility guard, not proof of physical provenance.

IOHID and CGEvent do not provide a public pairing token here. Auto Reverse
therefore labels last-active correlation as `high` through 8 ms, `medium`
through 50 ms, and `timed_out` afterward. Only high/medium observations may
contribute identity, product name, and transport. Detailed CSV exports retain
that confidence code; privacy traces omit it together with device identity.

The Debug Console resolution chain is available by hovering an event row and
in Detailed CSV: snapshot confidence/HID class, classifier evidence, input
provenance, direction/step/preset values with their source, and final reason.
The replayable privacy trace intentionally keeps only behavior needed for
replay and omits this device/process context.

Application rules are still not live. A pure `AppTargetSessionPin` is ready for
their future platform adapter: a positive target PID is fixed across direct and
momentum events until end, cancellation, or a gap over 150 ms. Orphaned
momentum cannot adopt whichever app happens to be frontmost later.

`reverse_magic_mouse` is live and independent from `reverse_trackpad`. Public
IOHID product inventory now wins when only a trackpad or only a Magic Mouse is
connected, so a missing touch observation can no longer route a lone built-in
trackpad through the Magic Mouse setting. When both are connected, a two-finger
gesture within 222 ms identifies the trackpad, momentum keeps the last source,
and a normal continuous event after 333 ms without a fresh touch is Magic
Mouse-like. IOHID matching/removal callbacks update the hint after hot-plug and
sleep/wake. Failed or unknown probes conservatively use the trackpad policy.
Rapid dual-device alternation and third-party smooth wheels remain heuristic;
per-device rules remain limited to discrete HID wheel devices.

## Start At Login

There are two start-at-login mechanisms because there are two launch styles:

- the GUI settings window uses `SMAppService.mainAppService()` and registers the `.app` bundle itself;
- the CLI command `enable-startup` installs a per-user LaunchAgent for the current binary path.

The CLI LaunchAgent lives at:

```text
~/Library/LaunchAgents/com.auto-reverse.agent.plist
```

That agent starts the current executable with the `run` argument on the next login. Use it for terminal/no-GUI installs. Use the settings window's Start at Login toggle for the bundled `.app`.

`startup-status --json` prints the LaunchAgent state, config path, config `start_at_login` value, and whether both are in sync. It does not create a config file just to report status.

## Architecture

Current modules, layered from pure logic down to platform code. Read them
top to bottom to learn the project in small pieces - each file has one
reason to change, and nothing above `platform/` imports an OS framework:

```text
src/main.rs                          CLI entrypoint and command orchestration
src/cli.rs                           command/flag parser and CLI option structs
src/lib.rs                           library facade documenting the layering
src/error.rs                         shared AppError / AppResult
src/device.rs                        DeviceKind + HardwareId/DeviceIdentity vocabulary
src/device_classifier.rs             pure inventory/gesture/timing policy + fallback
src/device_source.rs                 pure public HID transport/fail-open policy
src/device_attribution.rs            bounded last-active confidence policy
src/device_catalog.rs                connected/remembered/unavailable projection
src/device_test.rs                   exact-identity recent-event state machine
src/diagnostics.rs                   pure axis and stable decision-reason vocabulary
src/diagnostics_summary.rs           aggregate privacy-bounded clipboard text
src/app_session.rs                   future app-target session pin, non-live
src/input_policy.rs                  shared input provenance and bypass
src/settings_search.rs               fuzzy settings/diagnostics navigation
src/update_policy.rs                 network-free manual release destinations
src/preset_preview.rs                temporary preset confirm/expiry policy
src/refresh_policy.rs                notification coalescing + timer backstop
src/tap_watchdog.rs                  bounded event-tap health/recovery policy
src/recovery_audit.rs                bounded typed recovery reasons/attempts
src/dynamics_gate.rs                 fail-closed runtime/release dynamics gate
src/input.rs                         normalized ScrollEvent with optional shared identity
src/runtime.rs                       lock-free process-local pause control
src/scroll.rs                        pure reversal policy (no CoreGraphics)
src/statistics.rs                    shared nearest-rank integer distributions
src/scroll_trace.rs                  bounded TOML schema + deterministic replay
src/scroll_lab.rs                    transfer metrics + constant-gain baseline
src/event_rate.rs                    observed per-device event-rate distributions
src/scroll_benchmark.rs              pure target-acquisition matrix/state machine
src/config/mod.rs                    facade re-exporting AppConfig/ConfigStore
src/config/schema.rs                 fields, defaults and validation
src/config/device_rules.rs           pure selector priority, matching and mutation
src/config/profiles.rs               field inheritance and fixed selector precedence
src/config/reset.rs                  exact-device and dynamics reset scopes
src/config/store.rs                  durable private save, lock, inspect/repair, snapshots/CAS
src/config/transfer/mod.rs           transfer facade and typed errors
src/config/transfer/document.rs      version/schema migration + TOML
src/config/transfer/diff.rs          pure section review/application
src/config/transfer/secure_file.rs   bounded O_NOFOLLOW file trust boundary
src/platform/mod.rs                  cfg-gated platform adapters
src/platform/macos/mod.rs            macOS integration overview
src/platform/macos/scroll_events.rs  CGEvent field mapping (read event, write decision)
src/platform/macos/permissions.rs    Accessibility TCC policy/check/request
src/platform/macos/hid.rs            IOHID inventory + wheel identity/attribution cache
src/platform/macos/gesture.rs        passive AppKit gesture tap + classifier adapter
src/platform/macos/startup.rs        LaunchAgent start-at-login support (headless `run`)
src/platform/macos/event_tap.rs      CGEventTap runtime loop, config shared via Arc<RwLock<_>>
src/platform/macos/app_events.rs     app-activation refresh notification bridge
src/platform/macos/power_events.rs   NSWorkspace sleep/wake observer and atomic signal
src/platform/macos/daemon_lock.rs    flock: only one live CGEventTap at a time, any launch path
src/platform/macos/activation.rs     second GUI launch -> existing-window focus mailbox
src/platform/macos/save_panel.rs     native config/diagnostic open-save panels + Finder reveal
src/platform/macos/external_url.rs   trusted release URLs -> default browser
src/platform/macos/tap_metrics.rs    on-demand CGGetEventTapList interval snapshot
src/platform/macos/debug_log.rs      structured decisions + local Debug Console ring buffer
src/platform/macos/recovery_log.rs   process-local adapter to pure recovery audit
src/platform/macos/quit_handler.rs   AppleEvent quit interception so only tray Quit exits
src/platform/macos/login_item.rs     SMAppService.mainAppService() wrapper (gui feature only)
src/platform/macos/tray.rs           rich native menu-bar tray icon/menu (gui feature only)
src/platform/macos/tray/device_rules.rs pure tray quick-pick rule mutation
src/ui.rs                            settings app coordinator and tab contents
src/ui/runtime.rs                    typed tap lifecycle and explicit event channel
src/ui/device_rules.rs               catalog/profile/test/reset device rows
src/ui/preset_preview.rs             temporary dynamics model controls
src/ui/theme.rs                      handoff tokens and custom egui controls
src/ui/local_export.rs               shared CSV escaping + atomic local replacement
src/ui/config_transfer.rs            config panels and pending section review
src/ui/debug_console.rs              Debug Console viewport/filter/table
src/ui/debug_console/export.rs       detailed CSV/privacy trace + atomic receipt
src/ui/scroll_benchmark.rs           interactive benchmark viewport + result CSV
tests/cli_integration.rs             real binary in isolated HOME/config sandboxes
tests/property_invariants.rs         five deterministic 512-seed property suites
tests/fixtures/config/               historical and invalid-key config fixtures
scripts/lib/app-bundle.sh            shared bundle identity + exact-process helpers
scripts/build-app-bundle.sh          debug/release bundle construction
scripts/check-app-bundle.sh          strict or identity-only bundle validation
scripts/install-app-bundle.sh        staged atomic install/update with rollback
scripts/uninstall-app-bundle.sh      startup cleanup + identity-checked removal
scripts/check-install-workflow.sh    isolated install/update/uninstall smoke
scripts/check-dynamics-release-gate.sh source/manifest/threshold parity gate
scripts/check-regression-matrix.sh   stable PR-01..PR-06 QA structure gate
packaging/dynamics-release-gate.toml exact-candidate acceptance manifest
```

The macOS framework crates are target-specific dependencies. The lean build
keeps only the small AppKit event/touch surface required by the classifier and
does not include eframe, windows, menus, images, or login-item integration; the
pure domain modules themselves still import no OS framework.

The remaining useful split is narrower: device rows, preset preview, config
transfer, benchmark, diagnostics and runtime lifecycle already have separate
owners. Keep the AppKit icon/menu adapter in `tray.rs`, move structured
diagnostics behind a small sink, and split another settings tab only when its
behavior grows enough to justify that boundary.

## Three Iterations

### Iteration 1: Core Safety

Keep CLI and pure logic solid:

- keep reviewed v0/bad-key migration fixtures exhaustive and non-destructive;
- add more simulation flags;
- separate platform helpers from pure transform;
- keep tests fast and deterministic.

### Iteration 2: Product UX

Build the app surface:

- menu bar utility;
- settings window;
- permission onboarding;
- debug console and its dedicated module;
- process-local pause and permission-first recovery;
- configurable menu-bar visibility with relaunch/CLI recovery (done).

### Iteration 3: Release Quality

Make it releasable:

- physical dual-device Magic Mouse/trackpad and rapid-alternation validation
  of the inventory-first public classifier;
- manual sleep/wake validation of the implemented recovery path;
- packaging/signing;
- localization;
- optional automatic updater only after signed-feed and rollback prerequisites;
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

## External Research Pass

`RESEARCH.md` records a source-level review of 10 additional popular macOS
utilities and input projects plus primary papers on scrolling accuracy,
transfer functions, filtering, and latency. It adds `R01-R60` to the backlog.

The implementation order is intentionally conservative:

1. Measurement is implemented without changing runtime behavior: privacy trace
   replay, transfer lab, ScrollTest-style harness, six-class physical matrix,
   observed event-rate distributions, and repeated-budget assessment over
   manual public tap-latency snapshots. Physical and visual QA remain before
   treating the benchmark as release-validated.
2. The pure two-axis model now has continuous bypass, independent axis state,
   bounded time/rate estimation, signed-distance/cancellation accounting,
   direction/gap sessions, stop threshold, and explicit click/action policy,
   but remains intentionally disconnected from live input. Its pure scheduler
   contract now adds tagged generation/TTL samples, idle teardown, and latched
   fail-open without creating a timer.
3. Per-device field inheritance, public HID source compatibility,
   confidence-scored attribution, visible device states, aliases, and tri-state
   direction are implemented. App-target session pinning and versioned config
   transfer now have pure contracts, but app rules remain research-only until a
   public adapter can identify the target under the cursor without changing it
   during momentum. Live dynamics still waits for measurements and the physical
   matrix.
4. Settings now has fuzzy navigation and reviewed config transfer; Diagnostics
   exposes the full policy resolution chain plus an aggregate-only clipboard
   summary; Devices can verify a serial/location-qualified wheel locally. These
   paths share domain projections but keep native panels and egui presentation
   outside the pure modules.
5. Preset preview and reset scopes are pure and explicit. Permission/device
   refresh is notification-led, while a public-state watchdog adds hysteresis
   and a finite recovery budget.
6. R56-R60 close the review loop with typed recovery audit, exhaustive config
   fixtures, deterministic property suites, a stable platform regression
   matrix, and a fail-closed dynamics release/rollback boundary. Physical rows
   and live scheduler integration remain explicit release evidence, not claims.

Trackpad and Magic Mouse continuous events are not smoothed again. Any future
scheduler must still write only `DeltaAxis1/2`; private touch APIs, HID seizure,
and default telemetry remain explicitly out of scope.

## S01-S10 Config and Update Pass

| ID | Status | Independently reviewable change |
| --- | --- | --- |
| S01 | Done | Sync the temporary config file before atomic rename. |
| S02 | Done | Sync the parent directory and commit config with mode `0600`. |
| S03 | Done | Inspect config without creating a directory, file, or lock. |
| S04 | Done | Report text/JSON validation status with nonzero invalid results. |
| S05 | Done | Repair only by explicit command and preserve exact invalid bytes. |
| S06 | Done | Allocate backups exclusively and restore the original on failure. |
| S07 | Done | Emit stable coarse CLI error codes, including permission errors. |
| S08 | Done | Keep update checks manual with no app-owned background network. |
| S09 | Done | Share canonical latest/all channels across CLI and egui. |
| S10 | Done | Show a compact version/release block with inline launch errors. |

The canonical rationale and source links live in `recommendation.md` and
`UPDATES.md`.

## Top 500

The full working backlog lives in `recommendation.md`: 500 base findings,
`N01-N400` implementation follow-ups, `R01-R60` research-derived items, and
the completed `S01-S10` config durability/manual update pass (970 total).
The collapsed mirror below keeps the requested 500 base items visible from the
README without making the first read impossible.

<details>
<summary>Show 500 base recommendations, problems and improvements</summary>

<!-- TOP500_README:START -->
| # | Type | Item |
| --- | --- | --- |
| 1 | Done | Проект уже имеет рабочий CLI вместо старого `Hello, world!`. |
| 2 | Done | `src/lib.rs` отделяет library facade от binary entrypoint. |
| 3 | Done | `src/main.rs` стал тонким CLI entrypoint. |
| 4 | Done | `AppConfig` хранит versioned config schema. |
| 5 | Done | TOML выбран как читаемый формат настроек. |
| 6 | Done | `ConfigStore::default_path` учитывает macOS Application Support. |
| 7 | Done | `AUTO_REVERSE_CONFIG` помогает безопасно тестировать конфиг. |
| 8 | Done | `load_or_create` делает first-run проще. |
| 9 | Done | Config save использует уникальный temporary file. |
| 10 | Done | Config save делает `sync_all` временного файла до rename и директории после rename. |
| 11 | Done | Durable save, unique `create_new` temp и mode `0600` покрыты тестами. |
| 12 | Done | Явный `repair-config` сохраняет corrupted config byte-for-byte в уникальном sibling backup. |
| 13 | Done | Backup получает имя `.broken.<timestamp>.<pid>.<counter>.toml`; valid config команда не переписывает. |
| 14 | Problem | Нет migration framework для `config_version`. |
| 15 | Improve | Добавить `config::migration` до schema v2. |
| 16 | Done | `ConfigStore` и `AppConfig` разделены: `config/schema.rs` и `config/store.rs`. |
| 17 | Done | Разделение `config/schema.rs` / `config/store.rs` выполнено. |
| 18 | Done | Монолитный `config.rs` удален; ответственности разнесены по SRP. |
| 19 | Done | `config/mod.rs` реэкспортирует `AppConfig`/`ConfigStore` как public facade. |
| 20 | Done | `DeviceKind::as_str` уменьшает DRY-дублирование. |
| 21 | Done | `Display` и `FromStr` используют единый device-name контракт. |
| 22 | Done | `DeviceKind` покрывает mouse, trackpad, Magic Mouse, unknown. |
| 23 | Done | Magic Mouse определяется live как отдельный best-effort continuous source. |
| 24 | Done | Добавлен отдельный public AppKit gesture classifier без private API. |
| 25 | Done | Continuous source сначала определяется по exclusive IOHID inventory; timing signal нужен только при `Both`. |
| 26 | Done | `doctor`, README и UI честно показывают отдельную Magic Mouse policy и heuristic caveat. |
| 27 | Done | Non-continuous scroll считается mouse. |
| 28 | Done | `DeviceIdentity` использует serial, затем port/location fallback. |
| 29 | Done | `DeviceIdentity` и `DeviceInfo` проведены через HID, policy, CLI, UI и tray. |
| 30 | Problem | Нет device registry. |
| 31 | Improve | Хранить known devices и last_seen metadata. |
| 32 | Problem | Unknown device config есть, но discovery отсутствует. |
| 33 | Improve | Показывать unknown devices в diagnostics. |
| 34 | Done | `ScrollEvent` нормализует vertical/horizontal delta. |
| 35 | Done | `ScrollEvent` содержит `continuous`. |
| 36 | Done | `ScrollEvent` содержит `synthetic`. |
| 37 | Done | `ScrollEvent` содержит `source_pid`. |
| 38 | Problem | `ScrollEvent` не содержит timestamp. |
| 39 | Improve | Добавить monotonic timestamp для diagnostics. |
| 40 | Problem | `ScrollEvent` не содержит event phase. |
| 41 | Improve | Добавить phase после gesture/HID spike. |
| 42 | Problem | `ScrollEvent` не содержит device id. |
| 43 | Improve | Добавить optional `device_id`. |
| 44 | Done | `scroll::transform_event` чисто тестируется. |
| 45 | Done | Disabled config делает pass-through. |
| 46 | Done | Synthetic event делает pass-through. |
| 47 | Done | Raw-input guard пропускает injected events. |
| 48 | Done | CLI simulate умеет задавать `source_pid`. |
| 49 | Improve | Добавить integration test для `simulate --source-pid`. |
| 50 | Done | CLI simulate умеет задавать `synthetic`. |
| 51 | Improve | Добавить integration test для `simulate --synthetic`. |
| 52 | Done | Vertical reverse включен по умолчанию. |
| 53 | Done | Horizontal reverse выключен по умолчанию. |
| 54 | Done | Mouse reverse включен по умолчанию. |
| 55 | Done | Trackpad reverse выключен по умолчанию. |
| 56 | Done | `reverse_magic_mouse` применяется live через `DeviceKind::MagicMouse`. |
| 57 | Done | UI показывает отдельный рабочий Magic Mouse checkbox. |
| 58 | Done | Step size применяется к non-continuous wheel delta. |
| 59 | Problem | Step size logic живет рядом с reverse logic. |
| 60 | Improve | Вынести wheel step в `scroll::wheel`. |
| 61 | Done | `discrete_scroll_step_size` валидируется диапазоном 0..=20. |
| 62 | Problem | Диапазон step size не объяснен в docs. |
| 63 | Improve | Добавить описание: 0 means system/default/no adjustment. |
| 64 | Done | `saturating_neg` предотвращает overflow. |
| 65 | Done | Step size multiplication использует `saturating_mul`. |
| 66 | Improve | Оставить regression test на будущий рост диапазона step size. |
| 67 | Done | CoreGraphics derived delta regression покрыт тестом. |
| 68 | Done | CoreGraphics helpers вынесены из `scroll.rs`; он теперь чистая политика. |
| 69 | Done | CGEvent field code живет в `platform/macos/scroll_events.rs`. |
| 70 | Done | Event tap disabled recovery re-enables через сохраненный |
| 71 | Problem | Event tap install не имеет integration smoke test. |
| 72 | Improve | Добавить mock listener для runtime contract tests. |
| 73 | Problem | `OnceLock<AppConfig>` делает event tap одноразовым в процессе. |
| 74 | Improve | Для будущего UI нужен runtime state с reloadable config snapshot. |
| 75 | Problem | Нет hot reload config. |
| 76 | Improve | Добавить command `reload` или runtime channel. |
| 77 | Problem | Нет pause без изменения config. |
| 78 | Done | Persistent `enabled` отделен от process-local temporary pause. |
| 79 | Done | CLI `enable`, `disable`, `toggle` меняют config. |
| 80 | Done | CLI commands теперь проходят через отдельный parser в `src/cli.rs`. |
| 81 | Improve | Для большего CLI все еще можно добавить `clap`, но текущий parser мал и покрыт тестами. |
| 82 | Done | `main.rs` больше не содержит ручной parsing flags для `simulate`. |
| 83 | Done | CLI parsing вынесен в отдельный command/options module. |
| 84 | Done | `parse_bool` принимает yes/no/1/0, и help теперь перечисляет эти значения. |
| 85 | Done | Help перечисляет accepted bool values: true/false/yes/no/1/0. |
| 86 | Done | Каждая семья CLI errors имеет стабильный machine-readable code. |
| 87 | Done | Реализованы `E_CONFIG_PARSE`, `E_CONFIG_INVALID`, `E_PERMISSION`, `E_PLATFORM` и остальные coarse codes. |
| 88 | Done | `AppError` отделяет IO, config, platform и usage. |
| 89 | Problem | `AppError::InvalidConfig` хранит plain string. |
| 90 | Improve | Сделать structured validation errors. |
| 91 | Problem | `AppError::Platform` слишком общий. |
| 92 | Done | Tap lifecycle типизирован через `ui::runtime::State` и `TapRunOutcome`. |
| 93 | Done | Accessibility check реализован. |
| 94 | Done | Лишний Input Monitoring preflight удалён из runtime gate; Accessibility достаточно. |
| 95 | Done | Неиспользуемый `request_input_monitoring_access` удалён вместе с лишним permission gate. |
| 96 | Done | Missing Input Monitoring больше не блокирует запуск и не требует request action. |
| 97 | Done | Accessibility prompt вызывается через documented trusted options. |
| 98 | Done | Один `request_scroll_control_access` обслуживает CLI и GUI startup. |
| 99 | Done | `doctor` показывает exact current executable path. |
| 100 | Done | `doctor` печатает current executable path рядом с config path. |
| 101 | Done | `doctor --no-create` убирает config creation side effect. |
| 102 | Done | `doctor --no-create` и first-run `init` теперь разделены. |
| 103 | Done | `doctor` показывает единственное обязательное Accessibility permission. |
| 104 | Problem | `doctor` не проверяет event tap installability. |
| 105 | Improve | Добавить dry install check или explicit explanation. |
| 106 | Problem | Нет runtime diagnostics buffer. |
| 107 | Improve | Добавить ring buffer для последних decisions. |
| 108 | Problem | Event hot path не должен логировать синхронно. |
| 109 | Improve | Использовать lock-free/ring buffer или sampled logging. |
| 110 | Problem | Нет tracing/log crate. |
| 111 | Improve | Ввести `tracing` только после выбора diagnostics design. |
| 112 | Problem | Нет benchmark hot path. |
| 113 | Improve | Добавить microbenchmark для `transform_event`. |
| 114 | Problem | Нет property tests для sign reversal. |
| 115 | Improve | Проверить invariant: magnitude сохраняется кроме wheel step. |
| 116 | Problem | Нет теста для `i64::MIN` vertical/horizontal. |
| 117 | Improve | Добавить regression tests для saturating behavior. |
| 118 | Done | Есть тест для step size 0. |
| 119 | Improve | Добавить CLI simulation example для step size 0 после command support. |
| 120 | Problem | Нет теста для `reverse_unknown`. |
| 121 | Improve | Добавить unknown-device transform test. |
| 122 | Done | Pure transform покрывает Magic Mouse config. |
| 123 | Done | Live Magic Mouse distinction подключен через passive gesture adapter. |
| 124 | Done | Pure classifier contract отделен от AppKit и покрывает inventory/timing/momentum transitions. |
| 125 | Done | Inventory matrix, conservative fallback и gesture classifier покрыты unit tests. |
| 126 | Done | Device parse/display round-trip покрыт. |
| 127 | Problem | Нет serde round-trip для `DeviceKind`. |
| 128 | Improve | Добавить TOML test для `magic-mouse`. |
| 129 | Problem | Нет CLI snapshot tests. |
| 130 | Done | CLI integration tests запускают real binary через `std::process::Command` в sandbox. |
| 131 | Problem | Нет golden output для `show-config`. |
| 132 | Improve | Зафиксировать config output или сделать формат explicit unstable. |
| 133 | Problem | Нет test tempdir crate. |
| 134 | Improve | Использовать `tempfile` вместо timestamp path helper. |
| 135 | Problem | Tests оставляют файл, если panic до cleanup. |
| 136 | Improve | `tempfile::NamedTempFile` решит cleanup. |
| 137 | Problem | Нет module-level docs. |
| 138 | Improve | Добавить краткие `//!` docs для модулей. |
| 139 | Problem | Публичный API слишком широк: все modules `pub`. |
| 140 | Improve | Экспортировать facade, скрывать platform internals. |
| 141 | Problem | `event_tap` публичен из lib. |
| 142 | Improve | После UI/runtime split сделать platform modules crate-private. |
| 143 | Done | `permissions` переехал под platform-слой. |
| 144 | Done | Модуль живет в `src/platform/macos/permissions.rs`. |
| 145 | Problem | Проект пока macOS-only, но docs говорят о future cross-platform. |
| 146 | Done | `src/platform/mod.rs` cfg-gate'ит `macos`; бинарь дает понятный compile_error! вне macOS. |
| 147 | Problem | Non-macOS build behavior не определен. |
| 148 | Improve | Сделать graceful compile error или stub platform. |
| 149 | Problem | Cargo features не разделяют platform code. |
| 150 | Improve | Добавить feature `macos-event-tap`. |
| 151 | Done | `core-graphics`/`core-foundation` стали target-specific dependencies. |
| 152 | Done | Cargo.toml: `[target.'cfg(target_os = "macos")'.dependencies]`. |
| 153 | Problem | Нет MSRV. |
| 154 | Improve | Зафиксировать Rust version через `rust-toolchain.toml`. |
| 155 | Problem | Edition 2024 требует свежий toolchain. |
| 156 | Improve | README должен назвать required Rust version. |
| 157 | Done | Есть macOS GitHub Actions CI. |
| 158 | Done | CI запускает полный local quality gate и bundle smoke. |
| 159 | Problem | Нет `cargo audit`. |
| 160 | Improve | Добавить audit в release checklist. |
| 161 | Problem | Нет license. |
| 162 | Improve | Выбрать MIT/Apache-2.0/другую license до публикации. |
| 163 | Problem | Нет changelog. |
| 164 | Improve | Добавить `CHANGELOG.md` с текущим first slice. |
| 165 | Problem | Нет ADR. |
| 166 | Improve | Создать ADR for event tap, config format, CLI first. |
| 167 | Done | `.idea/` добавлен в root `.gitignore`. |
| 168 | Improve | IDE metadata остается локальным и не попадает в commit. |
| 169 | Done | Remote `origin` настроен. |
| 170 | Done | Push в `origin/master` работает. |
| 171 | Done | Menu bar app есть: native AppKit `NSStatusItem`. |
| 172 | Done | macOS status item остается primary UI по умолчанию; visibility меняется live с безопасным recovery. |
| 173 | Done | CLI больше не единственный способ менять настройки. |
| 174 | Done | Preferences/settings window добавлен через egui. |
| 175 | Done | Missing permissions дают compact permission-first state без marketing welcome. |
| 176 | Done | Первый запуск выбирает Permissions и показывает targeted recovery actions. |
| 177 | Done | Visible active/paused/permission state есть в header окна и status-dot трея. |
| 178 | Done | Status icon отражает active, paused и permission-blocked через отдельную status-dot. |
| 179 | Done | Есть temporary pause на 15 минут. |
| 180 | Done | Pause process-local и не пишет config. |
| 181 | Problem | Right-click toggle не реализован. |
| 182 | Improve | Повторить Scroll Reverser: right/control click toggles app. |
| 183 | Done | Option-click debug console реализован через rich tray menu handling. |
| 184 | Done | Option-click открывает Debug Console; меню также имеет явный fallback пункт. |
| 185 | Done | Advanced UI управляет `show_menu_bar_icon`, не останавливая runtime. |
| 186 | Done | `show-menu-bar-icon` и повторный запуск reload-ят config и возвращают hidden icon/window. |
| 187 | Done | Start at login config теперь связан с LaunchAgent integration. |
| 188 | Done | Packaged `.app` дополнен `SMAppService.mainAppService()` для GUI login item. |
| 189 | Done | Выбрана явная manual-browser update strategy без фоновых сетевых запросов приложения. |
| 190 | Done | CLI и egui открывают только canonical GitHub Releases URLs через узкий macOS adapter. |
| 191 | Done | `include_beta_updates` выбирает all-releases channel для неявного CLI выбора; `--latest`/`--all` остаются явными overrides. |
| 192 | Done | `check_for_updates` сохранен для совместимости, но `doctor` честно сообщает, что automatic checking отключен. |
| 193 | Problem | `show_discrete_scroll_options` есть, UI нет. |
| 194 | Improve | Показывать wheel step section после wheel event. |
| 195 | Problem | Нет device list. |
| 196 | Improve | Settings first screen должен быть device-oriented. |
| 197 | Problem | Нет last active device. |
| 198 | Improve | Diagnostics should show last source and rule. |
| 199 | Problem | Нет device aliases. |
| 200 | Improve | Позволить переименовать устройства после registry. |
| 201 | Problem | Нет disconnected device state. |
| 202 | Improve | Показывать known/disconnected devices отдельно. |
| 203 | Done | Restore defaults реализован. |
| 204 | Done | Reset требует confirmation и показывает число device rules. |
| 205 | Problem | Нет undo для settings changes. |
| 206 | Improve | Добавить short undo toast для non-destructive changes. |
| 207 | Problem | Нет settings validation UI. |
| 208 | Improve | Ошибки config показывать рядом с полем. |
| 209 | Problem | Нет import/export config. |
| 210 | Improve | Export config для backup и support. |
| 211 | Problem | Import может принести invalid TOML. |
| 212 | Improve | Validate before applying imported config. |
| 213 | Done | Permissions panel имеет action button для открытия Privacy & Security. |
| 214 | Done | Permission UI открывает только обязательный Accessibility pane. |
| 215 | Done | Accessibility flow унифицирован между CLI, UI и tray. |
| 216 | Improve | Добавить OS-specific instructions. |
| 217 | Problem | Permission status только в CLI. |
| 218 | Improve | Показывать status badges in UI. |
| 219 | Problem | Нет state `Degraded`. |
| 220 | Improve | Runtime state: Active, Paused, NeedsPermission, Degraded, Error. |
| 221 | Done | Есть lightweight tap lifecycle runtime. |
| 222 | Done | `ui/runtime.rs` использует explicit channel events и typed state. |
| 223 | Problem | UI может напрямую дергать config store. |
| 224 | Improve | UI должен отправлять `AppCommand`. |
| 225 | Done | Design tokens вынесены из app coordinator. |
| 226 | Done | `ui/theme.rs` содержит handoff colors, spacing, radii и custom controls. |
| 227 | Problem | Product может стать слишком декоративным. |
| 228 | Improve | Использовать native compact utility layout. |
| 229 | Problem | Cards могут захламить настройки. |
| 230 | Improve | Использовать tables/lists вместо card grid. |
| 231 | Problem | Первый экран может стать landing page. |
| 232 | Improve | Первый экран должен быть рабочей панелью. |
| 233 | Problem | UI labels могут быть техническими. |
| 234 | Improve | Использовать понятные тексты: Mouse, Trackpad, Wheel step. |
| 235 | Problem | `Natural` не всем понятно. |
| 236 | Improve | Добавить microcopy: content moves with fingers vs opposite. |
| 237 | Problem | Слишком много helper text перегрузит UI. |
| 238 | Improve | Основные пояснения в tooltip/help popover. |
| 239 | Problem | Tooltips недоступны keyboard-only users. |
| 240 | Improve | Важные permission explanations держать inline. |
| 241 | Problem | Цветом нельзя единственным способом показывать статус. |
| 242 | Improve | Добавить labels/icons for state. |
| 243 | Problem | Нет accessibility labels. |
| 244 | Improve | Все controls должны иметь accessible names. |
| 245 | Problem | Нет keyboard navigation plan. |
| 246 | Improve | Tab order должен проходить все settings. |
| 247 | Problem | Нет dark mode QA. |
| 248 | Improve | Follow system appearance and test both themes. |
| 249 | Problem | Иконки могут не соответствовать macOS conventions. |
| 250 | Improve | Использовать native symbols или аккуратные monochrome assets. |
| 251 | Problem | Нет retina status icon review. |
| 252 | Improve | Проверить icon на light/dark menu bar. |
| 253 | Problem | Длинные device names ломают layout. |
| 254 | Improve | Truncate middle with tooltip. |
| 255 | Problem | Compact UI может обрезать русский текст. |
| 256 | Improve | Проверить localization expansion 30 percent. |
| 257 | Problem | Нет i18n structure. |
| 258 | Improve | Вынести strings до добавления второго языка. |
| 259 | Problem | README смешивает English и Russian. |
| 260 | Improve | Выбрать docs language или разделить localized docs. |
| 261 | Problem | Русский пользователь просит русскую документацию. |
| 262 | Improve | Добавить `README.ru.md` или перевести основной README. |
| 263 | Problem | Product name не закреплен визуально. |
| 264 | Improve | Settings title and about panel should say Auto Reverse. |
| 265 | Problem | Нет about panel. |
| 266 | Improve | About panel: version, config path, repo, privacy. |
| 267 | Problem | Нет privacy UX. |
| 268 | Improve | Сказать: no network telemetry by default. |
| 269 | Problem | Update checks могут противоречить privacy. |
| 270 | Improve | Automatic update checks only opt-in. |
| 271 | Problem | Debug console может показать sensitive data. |
| 272 | Improve | Log only scroll metadata, never text input. |
| 273 | Problem | Input hooks вызывают trust concerns. |
| 274 | Improve | UI должен объяснять, зачем нужны permissions. |
| 275 | Problem | Нет recovery when icon hidden. |
| 276 | Improve | CLI `show-icon` или relaunch opens preferences. |
| 277 | Problem | Нет `open-settings` CLI. |
| 278 | Improve | Добавить command to open preferences when UI exists. |
| 279 | Problem | Нет `doctor --json`. |
| 280 | Improve | JSON diagnostics помогут support. |
| 281 | Done | Debug Console экспортирует filtered structured events через native Save Panel. |
| 282 | Partial | CSV local-only и structured; отдельный redaction/config-snapshot режим еще открыт. |
| 283 | Problem | Нет copy-to-clipboard action. |
| 284 | Improve | Diagnostics UI: copy summary. |
| 285 | Problem | Нет manual test window. |
| 286 | Improve | Добавить scroll test area в debug console. |
| 287 | Problem | Test area может перехватить реальные expectations. |
| 288 | Improve | Clearly label it as simulation-only. |
| 289 | Problem | Нет visual preview of direction. |
| 290 | Improve | Small scroll preview can show content movement. |
| 291 | Problem | Preview animations могут отвлечь. |
| 292 | Improve | Keep animations minimal and disable-able. |
| 293 | Problem | Нет профилей. |
| 294 | Improve | Profiles можно отложить до real device registry. |
| 295 | Problem | App-specific rules слишком сложны. |
| 296 | Improve | Не делать app-specific rules до stable v1. |
| 297 | Problem | Нет quick reset for bad settings. |
| 298 | Improve | Add `auto-reverse reset-config`. |
| 299 | Problem | Reset может потерять useful config. |
| 300 | Improve | Reset should create backup first. |
| 301 | Problem | Нет clear disabled state in menu. |
| 302 | Improve | Disabled controls should show reason and re-enable action. |
| 303 | Problem | Нет separation persistent vs session settings. |
| 304 | Improve | Mark session-only controls clearly. |
| 305 | Problem | Нужен дизайн для error states. |
| 306 | Improve | Error rows: plain language, technical details hidden. |
| 307 | Problem | Нет loading states. |
| 308 | Improve | Device scan and permissions refresh need non-jumpy states. |
| 309 | Problem | Нет empty state. |
| 310 | Improve | If no devices, show permissions and "scroll to detect". |
| 311 | Problem | Нет menu hierarchy. |
| 312 | Improve | Menu: Enable, Preferences, Diagnostics, Quit. |
| 313 | Problem | Menu может стать слишком длинным. |
| 314 | Improve | Keep advanced actions inside preferences. |
| 315 | Problem | Нет keyboard shortcut policy. |
| 316 | Improve | Avoid global hotkey until conflicts are handled. |
| 317 | Problem | Нет native alerts strategy. |
| 318 | Improve | Use alerts only for destructive actions. |
| 319 | Problem | Нет onboarding completion state. |
| 320 | Improve | Store first-run flag separately from config rules. |
| 321 | Problem | Нет welcome copy. |
| 322 | Improve | Welcome: one sentence goal, two permission steps, open settings. |
| 323 | Problem | Нет visual hierarchy. |
| 324 | Improve | Status first, devices second, advanced third. |
| 325 | Problem | Нет responsive window sizing. |
| 326 | Improve | Define minimum width and resizable constraints. |
| 327 | Problem | Нет high-contrast review. |
| 328 | Improve | Test contrast in light/dark/high contrast modes. |
| 329 | Problem | Нет reduced motion support. |
| 330 | Improve | Honor reduce motion for preview animations. |
| 331 | Problem | Нет localization QA. |
| 332 | Improve | Test English/Russian strings in compact window. |
| 333 | Problem | Нет icon-only tooltip plan. |
| 334 | Improve | Every icon button needs tooltip. |
| 335 | Problem | Нет docs for hidden advanced flags. |
| 336 | Improve | `reverse_only_raw_input` needs docs and UI explanation. |
| 337 | Problem | Raw-input mode wording confusing. |
| 338 | Improve | Label it "Ignore injected/remote scroll events". |
| 339 | Done | Hidden menu icon восстанавливается через relaunch или `show-menu-bar-icon`. |
| 340 | Done | Recovery задокументирован в UI, README, architecture, parity и `doctor`. |
| 341 | Implemented | Local install/update/uninstall и production release pipeline готовы; реальный Developer ID/notary QA остаётся. |
| 342 | Done | Local app bundle structure выбран: `target/<profile>/Auto Reverse.app`. |
| 343 | Implemented | Developer ID signing path и strict authority gate готовы; сертификат является external prerequisite. |
| 344 | Done | Developer ID plan и release contract документированы. |
| 345 | Implemented | `notarytool`/stapler/Gatekeeper pipeline готов; реальная submission ждёт credentials. |
| 346 | Done | `RELEASE.md` содержит canonical notarization checklist. |
| 347 | Done | Есть atomic installer/updater и identity-checked uninstaller. |
| 348 | Done | Packaging включает debug/release bundle, stable install path, rollback и isolated workflow smoke. |
| 349 | Done | LaunchAgent implementation добавлен в `platform/macos/startup.rs`. |
| 350 | Done | Add `SMAppService` path when the app bundle exists. |
| 351 | Partial | Wake recovery реализован; реальный sleep/wake на собранном bundle еще не отмечен в QA. |
| 352 | Done | NSWorkspace DidWake проверяет permissions и re-arm/restart tap через bounded recovery. |
| 353 | Problem | Event tap can stop silently in edge cases. |
| 354 | Improve | Runtime health should detect no events/disabled tap. |
| 355 | Problem | Нет watchdog. |
| 356 | Improve | Add lightweight health timer after UI runtime exists. |
| 357 | Problem | Нет crash-safe state restoration. |
| 358 | Improve | Ensure failure path keeps pass-through behavior. |
| 359 | Problem | Panic in callback would be dangerous. |
| 360 | Improve | Keep callback small and panic-free; wrap risky code. |
| 361 | Problem | `toml::to_string_pretty` in save can fail but no recovery UX. |
| 362 | Improve | Surface config write errors in UI. |
| 363 | Done | Persistent `config.toml.lock` защищает cooperating config writers. |
| 364 | Done | `File::lock` сериализует CLI/UI/tray операции на одном lock inode. |
| 365 | Done | Exact TOML revision CAS отклоняет stale GUI/tray save вместо last-writer-wins. |
| 366 | Done | CLI использует locked `update`, GUI/tray - `save_if_unchanged` с reload конфликта. |
| 367 | Done | Базовый single-instance behavior есть: `run.lock` для tap и `ui.lock` для окна/tray. |
| 368 | Done | Relaunch publishes `ui.activate`; the owner reveals and focuses its existing window. |
| 369 | Problem | `OnceLock` blocks multiple install attempts in one process. |
| 370 | Improve | Runtime should own tap lifecycle explicitly. |
| 371 | Problem | Нет graceful shutdown tests. |
| 372 | Improve | Add shutdown path before UI. |
| 373 | Problem | Нет signal handling for CLI run. |
| 374 | Improve | Handle Ctrl+C gracefully. |
| 375 | Done | Manual QA checklist хранится в repo. |
| 376 | Done | Добавлен `QA.md` с automated/manual matrix. |
| 377 | Problem | Нет test matrix for devices. |
| 378 | Improve | Matrix: wheel mouse, Magic Mouse, built-in trackpad, Magic Trackpad. |
| 379 | Problem | Нет remote desktop test. |
| 380 | Improve | Test `reverse_only_raw_input` with injected source_pid. |
| 381 | Problem | Нет high-resolution wheel test. |
| 382 | Improve | Test fractional/pixel-like fields on real devices. |
| 383 | Problem | Нет horizontal wheel test. |
| 384 | Improve | Test tilt wheel and horizontal gestures. |
| 385 | Problem | Нет Wacom compatibility. |
| 386 | Improve | Document Wacom behavior after hardware test. |
| 387 | Problem | Нет accessibility-device review. |
| 388 | Improve | Avoid breaking assistive input devices. |
| 389 | Problem | Нет "shake to locate cursor" regression review. |
| 390 | Improve | Include macOS accessibility gestures in manual QA. |
| 391 | Problem | Нет Notification Center/gesture edge-case QA. |
| 392 | Improve | Test system gestures while tap is active. |
| 393 | Problem | Swipe gestures not reversed. |
| 394 | Improve | Document limitation prominently. |
| 395 | Problem | Custom scroll surfaces may bypass CGEvent. |
| 396 | Improve | Document app-specific limitations. |
| 397 | Problem | iPhone Mirroring-like cases may bypass transform. |
| 398 | Improve | Keep limitations list updated. |
| 399 | Problem | Нет source attribution in docs for Scroll Reverser parity. |
| 400 | Improve | Keep links in `scroll-reverser-parity.md`. |
| 401 | Problem | Нет legal review of feature parity wording. |
| 402 | Improve | Avoid implying affiliation with Scroll Reverser. |
| 403 | Problem | Нет release version policy. |
| 404 | Improve | Use SemVer after first tagged release. |
| 405 | Problem | Нет tag workflow. |
| 406 | Improve | Create release tags with changelog. |
| 407 | Problem | Нет build reproducibility notes. |
| 408 | Improve | Document toolchain and target. |
| 409 | Problem | Нет binary size budget. |
| 410 | Improve | Track size before adding GUI toolkit. |
| 411 | Problem | GUI toolkit may dominate app size. |
| 412 | Improve | Prefer native AppKit or small wrapper for macOS. |
| 413 | Problem | Cross-platform promise could overreach. |
| 414 | Improve | Market as macOS-first until adapters exist. |
| 415 | Problem | Linux/Windows support undefined. |
| 416 | Improve | Add future notes, not product promise. |
| 417 | Problem | Нет dependency policy. |
| 418 | Improve | Add dependencies only for clear use cases. |
| 419 | Done | Есть security policy. |
| 420 | Done | Добавлен `SECURITY.md` с FFI/release boundaries. |
| 421 | Done | Есть contribution guide. |
| 422 | Done | Добавлен `CONTRIBUTING.md` с full gate и layering rules. |
| 423 | Done | Есть issue template. |
| 424 | Done | Bug template собирает device/macOS/diagnostics без лишних personal данных. |
| 425 | Done | Есть privacy policy. |
| 426 | Done | `PRIVACY.md` фиксирует local-only data handling. |
| 427 | Problem | Update checks could send network requests. |
| 428 | Improve | Make network behavior explicit and opt-in. |
| 429 | Problem | Нет telemetry boundary tests. |
| 430 | Improve | Ensure no network crate enters default build without review. |
| 431 | Problem | Нет static analysis. |
| 432 | Improve | Run `cargo deny` or equivalent later. |
| 433 | Problem | Нет dependency license review. |
| 434 | Improve | Add license review to release checklist. |
| 435 | Problem | Нет localization pipeline. |
| 436 | Improve | Start with English and Russian string files. |
| 437 | Problem | Нет translation credit policy. |
| 438 | Improve | Track translator credits in changelog. |
| 439 | Problem | Нет screenshots. |
| 440 | Improve | Add real screenshots after UI exists. |
| 441 | Done | Есть template status glyph, colored state dot и branded app icon asset. |
| 442 | Done | Active/paused/temporary/permission icon states спроектированы. |
| 443 | Done | App icon добавлен. |
| 444 | Done | Bundle генерирует `.icns` до codesign. |
| 445 | Done | Design review artifact хранится в repo. |
| 446 | Done | Добавлен `DESIGN.md` с выбранным handoff и tokens. |
| 447 | Problem | Нет final product review process. |
| 448 | Improve | Review UX, reliability, privacy before each milestone. |
| 449 | Problem | Нет branch strategy. |
| 450 | Improve | Use `codex/` branch prefix for agent work. |
| 451 | Problem | Current work happened on `master`. |
| 452 | Improve | Next tasks should branch before larger changes. |
| 453 | Problem | Нет remote configured. |
| 454 | Improve | Add `origin` before expecting push. |
| 455 | Problem | Push cannot complete in current repo state. |
| 456 | Improve | User must provide repo URL or create remote. |
| 457 | Problem | Merge was local only. |
| 458 | Improve | Push merge commit after remote setup. |
| 459 | Done | `.idea/` is ignored at repository root. |
| 460 | Improve | Keep IDE metadata local unless the project intentionally standardizes IDE settings. |
| 461 | Done | `.gitignore` was reviewed after merge. |
| 462 | Improve | Later add patterns for generated release artifacts when packaging exists. |
| 463 | Problem | Docs use mixed Russian/English. |
| 464 | Improve | Split user docs by language. |
| 465 | Problem | README still says "target product" in English. |
| 466 | Improve | Translate README if primary user language is Russian. |
| 467 | Problem | Architecture doc is Russian-only. |
| 468 | Improve | Keep architecture Russian if it helps project learning. |
| 469 | Problem | Recommendation list can become stale quickly. |
| 470 | Improve | Refresh it after every milestone. |
| 471 | Done | 900-item audit больше не используется как ежедневная task queue. |
| 472 | Done | Есть `ROADMAP.md` с 25 P0/P1/P2 задачами. |
| 473 | Problem | Нет issue tracker mapping. |
| 474 | Improve | Convert top 20 recommendations to issues. |
| 475 | Problem | Нет owner per area. |
| 476 | Improve | Add ownership notes for config, platform, UI. |
| 477 | Done | Roadmap и QA задают acceptance criteria по типу задачи. |
| 478 | Done | Definition of Done требует tests/docs/manual check для visual changes. |
| 479 | Done | Definition of Done зафиксирован. |
| 480 | Done | `ROADMAP.md` определяет code/tests/docs/review/QA gate. |
| 481 | Problem | Нет automated review checklist. |
| 482 | Improve | Add checklist: bugs, risks, missing tests, UX, privacy. |
| 483 | Problem | Нет code review notes file. |
| 484 | Improve | Add `REVIEW.md` or keep review section in architecture. |
| 485 | Problem | Нет benchmark baseline. |
| 486 | Improve | Capture current transform performance. |
| 487 | Problem | Нет memory allocation audit. |
| 488 | Improve | Ensure hot path does not allocate. |
| 489 | Problem | Нет unsafe boundary documentation. |
| 490 | Improve | Document each FFI call and invariant. |
| 491 | Done | FFI permissions компилируются только под `#[cfg(target_os = "macos")]` через platform/mod.rs. |
| 492 | Done | Весь macOS FFI живет за cfg-gated `platform::macos`. |
| 493 | Problem | FFI function availability depends on macOS version. |
| 494 | Improve | Document minimum macOS version and fallback behavior. |
| 495 | Problem | No app-level state machine. |
| 496 | Improve | Add explicit state enum before UI. |
| 497 | Done | Final review fixes and updated docs are included in the merge/push commit. |
| 498 | Done | Full gate is required immediately before the final commit. |
| 499 | Done | Push destination exists today. |
| 500 | Done | Remote configured; `master` can be pushed with docs/review commits. |
<!-- TOP500_README:END -->

</details>

## Documents

- `architecture.md` - current and target architecture, SOLID/DRY split, UX direction.
- `recommendation.md` - 970 recommendations, problems and improvements (500 base items + N01-N400 implementation follow-ups + R01-R60 research follow-ups + S01-S10 config/update follow-ups).
- `RESEARCH.md` - 10-repository source review, scientific/platform sources, rejected approaches, and three incremental implementation iterations.
- `TRACE.md` - privacy trace schema, limits, replay semantics, CLI lab, and ownership boundaries.
- `BENCHMARK.md` - target conditions, physical matrix, metrics, event-rate
  semantics, tap-latency snapshots, export, and ownership boundaries.
- `DYNAMICS.md` - measurable smooth-scrolling contract, latency budgets,
  experimental presets, continuous bypass, per-axis state, bounded rate/time,
  conservation, and the non-live pure engine boundary.
- `ROADMAP.md` - the executable top 25, grouped P0/P1/P2.
- `RELEASE.md` - canonical Developer ID, notarization, stapling, and
  distribution checklist.
- `DESIGN.md`, `QA.md`, `PRIVACY.md`, `SECURITY.md`, `CONTRIBUTING.md` - focused product and engineering contracts.
- `scroll-reverser-parity.md` - Scroll Reverser feature parity checklist.
