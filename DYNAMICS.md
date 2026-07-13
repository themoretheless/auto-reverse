# Discrete-Wheel Dynamics Contract

This document makes "smooth scrolling" measurable before Auto Reverse changes
live input. The current implementation is a pure experimental model only:
`CGEventTap` does not call it, no scheduler exists, and all installed scrolling
behavior remains the existing raw/reverse policy.

## Scope

- Applies only to normalized scalar-axis samples from a discrete wheel.
- Never applies to continuous Trackpad or Magic Mouse events.
- Owns no CoreGraphics object, timer, thread, config file, or wall clock.
- Receives monotonic microsecond timestamps from a caller and returns deltas
  that are due at that timestamp.
- Uses unit gain in every current preset. A preset changes response timing, not
  total signed distance.

## Measurable Product Contract

For one input sample and one axis:

1. **Off pass-through:** output equals input in the same call, pending distance
   is zero, and the engine remains idle.
2. **Immediate response:** every active preset emits a non-zero same-sign share
   in `handle_input`; pure-engine first-output latency is therefore zero.
3. **Bounded completion:** sampling at or after the preset deadline emits all
   remaining distance and returns the state to idle.
4. **Signed-distance conservation:** immediate plus tail output equals input
   within `1e-9` logical points for the modeled impulse.
5. **No idle creep:** after completion, later samples emit zero.
6. **Adapter budget:** a future scheduler may wake late by at most its 8 ms tail
   budget; it may not silently extend the preset curve.

Repeated input, direction changes, long gaps, cancellation, per-axis state,
bounded `dt`, and scheduler failure are handled explicitly in R16-R30. The
model is not release-enabled until those invariants and the physical matrix
pass.

## Presets

| Preset | Immediate share | Tail deadline | Product goal |
| --- | ---: | ---: | --- |
| Off | 100% | 0 ms | Exact immediate pass-through |
| Precise | 35% | 120 ms | Longest correction window for precise stops |
| Balanced | 55% | 90 ms | Middle response for general wheel use |
| Fast | 75% | 60 ms | Largest immediate response and shortest tail |

These are versioned experimental parameters, not claims copied from the cited
papers or competitor defaults. Changes require benchmark evidence and tests.

## Latency Budget

The pure `latency_budget` module defines separate engineering budgets:

| Stage | Average | Interval tail |
| --- | ---: | ---: |
| Event-tap callback | 1 ms | 8 ms |
| Future scheduler wake | 2 ms | 8 ms |

The 8 ms tail target leaves roughly half a 60 Hz frame for the rest of the
input-to-display path. It is an internal target, not a universal human
perception threshold.

Diagnostics retain the latest five manually requested callback readings and
wait for at least three. A tail warning requires two interval maxima above the
budget; one maximum outlier is reported as isolated and never warns. Repeated
average readings are assessed separately because Apple documents the min/max
interval but does not define the average accumulation window. Sampling stays
manual because `CGGetEventTapList` resets min/max when read.

## Physical Matrix

Every benchmark session records one explicit physical class:

- detent wheel;
- free-spin wheel;
- high-resolution wheel;
- Magic Mouse;
- built-in trackpad;
- external trackpad.

This is test metadata, not a promise that macOS exposes those identities on
every event. CSV stores a stable `physical_device` value alongside target mode,
distance, viewport, tolerance, movement time, switchbacks, overshoot, and event
count.

## Ownership

- `src/latency_budget.rs`: budgets, bounded interval history, warning policy.
- `src/scroll_dynamics.rs`: presets and pure scalar-axis state machine.
- `src/scroll_benchmark.rs`: physical class vocabulary and trial results.
- `src/ui/debug_console.rs`: manual callback samples and status presentation.
- `src/ui/scroll_benchmark.rs`: physical-class picker and CSV export.

The future `platform/macos/scroll_scheduler.rs` may consume pure emissions but
must not own the curve, preset resolution, or product policy.
