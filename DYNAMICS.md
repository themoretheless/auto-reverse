# Discrete-Wheel Dynamics Contract

This document makes "smooth scrolling" measurable before Auto Reverse changes
live input. The current implementation is a pure experimental model only:
`CGEventTap` does not call it, the scheduler contract owns no platform timer,
and all installed scrolling behavior remains the existing raw/reverse policy.

## Scope

- Applies only to normalized discrete-wheel input.
- `ScrollDynamics2D` routes every continuous event through exact pass-through;
  Trackpad, Magic Mouse, and other continuous streams cannot mutate dynamics.
- Vertical and horizontal axes own separate scalar engines, rate windows,
  velocity, residual ledger, momentum, timestamp, and deadline.
- Owns no CoreGraphics object, timer, thread, config file, or wall clock.
- Receives monotonic microsecond timestamps from a caller and returns deltas
  that are due at that timestamp.
- Uses unit gain in every current preset. A preset changes response timing, not
  total signed distance.

## Measurable Product Contract

For input samples on either axis:

1. **Off pass-through:** output equals input in the same call, pending distance
   is zero, and the engine remains idle.
2. **Immediate response:** every active preset emits a non-zero same-sign share
   in `handle_input`; pure-engine first-output latency is therefore zero.
3. **Bounded completion:** sampling at or after the preset deadline emits all
   remaining distance and returns the state to idle.
4. **Signed-distance conservation:** immediate plus tail output equals the sum
   of signed input within `1e-9` logical points when no explicit cancellation
   occurs. With cancellation, the ledger keeps the explicit equation
   `input = emitted + pending + residual + canceled` instead of silently
   losing distance.
5. **No idle creep:** after completion, later samples emit zero.
6. **Continuous bypass:** continuous input is returned exactly and leaves both
   discrete states byte-for-byte equivalent at the public snapshot boundary.
7. **Adapter budget:** a future platform timer may wake late by at most the
   sample's 8 ms TTL; it may not silently extend the preset curve.

Direction resets, opposite-input cancellation, long-gap sessions, stop
threshold, and physical-action cancellation are implemented in the pure model.
Pure scheduler tagging, idle lifecycle, and fail-open behavior are implemented.
Runtime opt-in and physical acceptance remain required before release enablement.

## Axis State

Each scalar engine exposes a diagnostic snapshot:

- **velocity** is signed input distance multiplied by a robust recent-rate
  estimate; it stays unavailable until enough intervals exist;
- **momentum** is signed distance already accepted and scheduled for the tail;
- **residual** is the separate signed conservation correction after accepted,
  emitted, and momentum distance are reconciled;
- **deadline** bounds the active tail.
- **session generation** increments on initial input, direction change, and a
  long-gap restart;
- **last cancellation** records reason, timestamp, and signed canceled distance.

The two-axis facade applies a discrete event transactionally: if either cloned
axis rejects it, neither live axis state advances.

## Time And Input Rate

Only caller-supplied monotonic timestamps are accepted. Input intervals are
normalized to `1-50 ms`; duplicate timestamps clamp upward and sleep/debugger
stalls clamp downward before they can affect rate or velocity. Absolute tail
deadlines still complete pending distance instead of integrating an unbounded
stall.

Rate estimation uses the median of a fixed eight-interval ring and returns no
estimate before three observations. It uses only delivery intervals observed by
the engine, never firmware metadata or one isolated interval. Raw and clamped
`dt` remain visible in `AxisStateSnapshot` for diagnostics.

## Session And Cancellation Policy

- A direction change cancels the old signed momentum before the opposite tick
  is processed, clears rate/velocity history, and starts a new generation.
- A raw input gap over 150 ms starts a new session. Pending output is canceled
  before the new event, so a stale tail cannot jump after sleep or debugging.
- When remaining momentum reaches `0.25` logical points or less, it is flushed
  into the current sample and the axis becomes idle. Distance is not dropped,
  and later samples cannot produce one-pixel creep.
- `CancellationPolicy` independently enables new-physical-action and pointer-
  click cancellation. Both are enabled by default; `NONE` is an explicit
  opt-out for controlled experiments.
- External cancellation affects both axes transactionally and returns the
  signed canceled distance for diagnostics.

## Scheduler Safety Contract

- `ScrollScheduler` is a pure caller-driven facade. It owns no clock, timer,
  thread, CoreGraphics event, or config flag.
- A wake exists only while either axis has pending distance. Completing the
  last tail removes the wake; polling an idle scheduler cannot emit output.
- Every wake has a unique wake id and the current vertical/horizontal session
  generations. A replaced callback is discarded even if direction did not
  change; a generation mismatch cannot cross into a new session.
- Every produced `ScheduledSample` repeats that generation and wake id, expires
  8 ms after the planned wake (not 8 ms after a late callback), and requires
  `AutoReverseSynthetic` provenance. Samples are validated again immediately
  before a future platform post.
- macOS synthetic events use the public 64-bit `kCGEventSourceUserData` field
  with the `AUTORVRS` marker. Normalization maps the marker to
  `ScrollEvent.synthetic`, which the existing pure reversal policy ignores.
- Any dynamics or scheduler error clears pending state and the active wake,
  returns the current physical delta exactly, and latches fail-open until an
  explicit reset. Fault recovery cannot happen silently inside a callback.
- A callback arriving after its due-anchored TTL is a scheduler fault: stale
  momentum is not sampled or posted, and later physical input remains exact
  fail-open until reset.
- Session and wake counters use checked arithmetic; overflow is an error, not
  a token reuse. Disarm/fault reset preserves the wake-id counter, and
  `reset_after_fault` is a no-op while healthy so it cannot cancel a live tail.

The platform posting adapter and timer remain deliberately absent. Therefore
this contract changes no live scrolling behavior.

## Presets

| Preset | Immediate share | Tail deadline | Product goal |
| --- | ---: | ---: | --- |
| Off | 100% | 0 ms | Exact immediate pass-through |
| Precise | 35% | 120 ms | Longest correction window for precise stops |
| Balanced | 55% | 90 ms | Middle response for general wheel use |
| Fast | 75% | 60 ms | Largest immediate response and shortest tail |

These are versioned experimental parameters, not claims copied from the cited
papers or competitor defaults. Changes require benchmark evidence and tests.

## Benchmark-Only Height Hypothesis

`BenchmarkTransfer::Baseline` is the default and leaves movement unchanged.
An explicitly constructed benchmark trial may instead select
`ScreenHeightHypothesis`, which multiplies input by
`case.viewport_height_points / 360`. The controlled test viewport is a proxy
for comparing heights, not a claim about the user's physical display.

The selected transfer is stored in `TrialResult` and CSV. No runtime config,
settings control, event-tap path, or dynamics preset imports this hypothesis.
Promoting it beyond the benchmark requires comparative physical evidence.

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
transfer hypothesis, distance, viewport, tolerance, movement time, switchbacks,
overshoot, and event count.

## Ownership

- `src/latency_budget.rs`: budgets, bounded reading history, warning policy.
- `src/scroll_dynamics.rs`: continuous bypass and transactional two-axis facade.
- `src/scroll_dynamics/axis.rs`: scalar state, momentum, residual, velocity and
  signed-distance ledger.
- `src/scroll_dynamics/rate.rs`: bounded `dt` and fixed recent-rate window.
- `src/scroll_dynamics/preset.rs`: stable preset vocabulary and parameters.
- `src/scroll_scheduler.rs`: fail-open orchestration and caller-driven polling.
- `src/scroll_scheduler/schedule.rs`: wake ids, generation pairs, sample TTL,
  synthetic provenance, and stale-sample disposition.
- `src/scroll_benchmark.rs`: physical class vocabulary and trial results.
- `src/ui/debug_console.rs`: manual callback samples and status presentation.
- `src/ui/scroll_benchmark.rs`: physical-class picker and CSV export.

The future `platform/macos/scroll_scheduler.rs` may consume pure wake tokens and
samples but must not own the curve, preset resolution, or product policy.
