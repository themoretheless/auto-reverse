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
| Release signing/notarization orchestration | `scripts/check-release-workflow.sh` | Required |
| Install/update/uninstall | `scripts/check-install-workflow.sh` | Required |

## Manual macOS matrix

Mark these on the exact bundle intended for release. Blank means not verified.

| Date | macOS | Device / scenario | Light | Dark | Result | Tester |
| --- | --- | --- | --- | --- | --- | --- |
|  |  | Discrete vertical wheel |  |  |  |  |
|  |  | Horizontal / tilt wheel |  |  |  |  |
|  |  | Built-in trackpad: two-finger scroll and momentum use Trackpad policy |  |  |  |  |
|  |  | Magic Trackpad: two-finger scroll and momentum use Trackpad policy |  |  |  |  |
|  |  | Magic Mouse continuous scroll uses independent Magic Mouse policy |  |  |  |  |
| 2026-07-13 | 26.6 | Internal trackpad with no Magic Mouse yields `trackpad only` hardware hint | N/A | N/A | Pass | Codex |
|  |  | Hot-plug/remove Magic Mouse updates the live classifier without restart |  |  |  |  |
|  |  | Rapidly alternate Magic Mouse and trackpad around 222/333 ms windows |  |  |  |  |
|  |  | Per-device Reverse / Don't reverse |  |  |  |  |
|  |  | Two identical VID/PID mice with different serials stay independent |  |  |  |  |
|  |  | Identical mice have distinct bounded labels in Settings and tray |  |  |  |  |
|  |  | Serial-less mouse: same-port reconnect keeps location rule |  |  |  |  |
|  |  | Serial-less mouse: moving ports shows a new identity without changing its sibling |  |  |  |  |
|  |  | Legacy vendor/product rule is shown as inherited and remains shared |  |  |  |  |
|  |  | Pause 15 minutes / Resume now |  |  |  |  |
|  |  | Missing then granted permissions |  |  |  |  |
|  |  | Start at Login after reboot |  |  |  |  |
|  |  | Sleep/wake: hidden app re-arms live tap or restarts one stopped tap |  |  |  |  |
|  |  | Sleep/wake preserves Magic Mouse/trackpad classification |  |  |  |  |
|  |  | Cmd-W, Cmd-Q, Dock Quit, tray Quit |  |  |  |  |
|  |  | Hide window, launch app again: one icon and focused existing window |  |  |  |  |
|  |  | Menu, device submenu, Option-click console |  |  |  |  |
|  |  | Debug filter/clear; Save Panel cancel/save/overwrite; Reveal in Finder |  |  |  |  |
|  |  | Debug Export menu: privacy trace and detailed CSV remain distinct |  |  |  |  |
|  |  | Privacy trace contains no wall time, PID, HID identity, app or window data | N/A | N/A |  |  |
|  |  | `trace-lab` replay, constant baseline and clutch threshold on exported trace | N/A | N/A |  |  |
|  |  | Benchmark Known/Unknown instructions produce separate result sessions |  |  |  |  |
|  |  | Benchmark Compact 12-case and Full 36-case matrices render without clipping |  |  |  |  |
|  |  | Benchmark 66 ms settle, switchbacks, overshoot, next-trial and CSV/Reveal workflow |  |  |  |  |
|  |  | Benchmark physical class: detent wheel |  |  |  |  |
|  |  | Benchmark physical class: free-spin wheel |  |  |  |  |
|  |  | Benchmark physical class: high-resolution wheel |  |  |  |  |
|  |  | Benchmark physical class: Magic Mouse |  |  |  |  |
|  |  | Benchmark physical class: built-in trackpad |  |  |  |  |
|  |  | Benchmark physical class: external trackpad |  |  |  |  |
|  |  | Benchmark CSV preserves the selected `physical_device` on every row | N/A | N/A |  |  |
|  |  | Observed event-rate p50/p95/max and five bins update per device kind |  |  |  |  |
|  |  | Manual tap-latency sample finds the active filter and labels interval min/avg/max |  |  |  |  |
|  |  | One latency outlier stays informational; two of five breached readings warn |  |  |  |  |
|  |  | Finder and System Settings app icon |  |  |  |  |
|  |  | Developer ID authority, hardened runtime, and secure timestamp | N/A | N/A |  |  |
|  |  | Notary result is Accepted and JSON audit log is reviewed | N/A | N/A |  |  |
|  |  | Stapled ticket validates; quarantined clean-Mac Gatekeeper launch passes |  |  |  |  |
|  |  | Developer-ID update preserves Accessibility and login item |  |  |  |  |
|  |  | Fresh release install to `/Applications` launches one process |  |  |  |  |
| 2026-07-11 | 26.6 | Update running `/Applications` app; config survives, release binary matches, new PID stays alive | N/A | N/A | Pass | Codex |
|  |  | Uninstall removes both startup registrations and preserves config |  |  |  |  |
|  |  | Reinstall after preserved-config uninstall restores settings |  |  |  |  |
|  |  | `--remove-user-data` removes only Auto Reverse config/locks/log |  |  |  |  |

Also verify high contrast, larger text, remote desktop with raw-input guard,
Notification Center, shake-to-locate and other system gestures remain intact,
and two simultaneously connected mice.

## Experimental dynamics gate

`scroll_dynamics` is not connected to live input yet. Before that changes, the
pure suite now proves:

- Off is exact same-call pass-through;
- every active preset produces immediate same-sign output;
- vertical and horizontal velocity, residual, momentum, rate and deadline state
  are independent;
- duplicate/long input intervals clamp to 1-50 ms;
- rate requires three observations and keeps only the latest eight;
- signed distance is conserved in both signs and two axes; every explicit
  cancellation is separately accounted instead of hidden as loss;
- direction change resets rate/momentum before opposite output;
- gaps over 150 ms create a new session without releasing stale tail;
- remaining momentum at or below 0.25 pt flushes immediately, then idle samples
  do not creep;
- click and new-physical-action triggers obey independent policy flags;
- continuous input bypasses dynamics without mutating discrete state;
- self-synthetic input is exact bypass and cannot mutate scheduler state;
- every tail sample carries generation, wake id, due-anchored 8 ms TTL, and
  synthetic tag; a 5 ms-late callback retains only 3 ms of posting lifetime;
- stale wake/sample races are discarded, scheduler is absent in idle, and
  dynamics/scheduler faults latch exact fail-open until explicit reset;
- fault reset preserves wake-id monotonicity, while a reset request in healthy
  state leaves the active wake untouched;
- screen-height scaling exists only as a recorded benchmark variant, with
  baseline remaining the default.

Still required before live integration and then on all six physical classes:

- the tail completes by its preset deadline plus the 8 ms scheduler budget;
- platform click/action hooks produce the expected pure cancellation trigger;
- the future platform timer/poster honors wake/sample validation and only
  writes marked `DeltaAxis1/2` events;
- physical fail-open and TTL behavior pass under induced stalls.
