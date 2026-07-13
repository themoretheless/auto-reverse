# Scroll Benchmark Contract

Auto Reverse includes a local ScrollTest-style target-acquisition harness for
measuring scrolling speed and accuracy without changing the live reversal
policy. The design follows the metrics and known/unknown target distinction in
[ScrollTest](https://arxiv.org/abs/2210.00735).

## Open The Harness

1. Open **Debug Console** from the menu-bar item.
2. Choose **Benchmark...**.
3. Select a target condition and matrix.
4. Start the session, then start each trial with the pointer inside the test
   viewport.

For a reproducible direct entry during development, run `cargo run -- benchmark`.

The benchmark receives the scroll stream delivered to its egui viewport. Point
events stay in points; line events use a documented 40-point adapter; page
events use the active logical viewport height. The app's live event tap remains
the owner of reversal behavior.

## Target Conditions

- **Known target** shows the target distance before the trial and numbers the
  document rows.
- **Unknown target** hides the target position until the target marker enters
  the viewport.

One session has one condition. Results and CSV rows always retain that condition
instead of aggregating known and unknown strategies together.

## Matrices

The matrices are deterministic Cartesian products, not one demo case:

| Preset | Distances (pt) | Viewport heights (pt) | Tolerances (pt) | Trials |
| --- | --- | --- | --- | --- |
| Compact | 240, 960, 2880 | 240, 360 | 12, 32 | 12 |
| Full | 160, 480, 1440, 4320 | 240, 360, 480 | 8, 20, 40 | 36 |

The target band is centered in a test surface whose physical egui height equals
the case's logical viewport height. Some full-matrix targets begin onscreen;
others require short or long scrolling.

## Trial Completion

Timing starts when **Start trial** is pressed. A trial completes only when:

- the target center is inside `distance +/- tolerance`;
- at least one effective document movement occurred; and
- no movement arrives for 66 milliseconds.

Document position is clamped at its origin. The pure state machine rejects
non-finite deltas, timestamps that move backwards, and input after completion.

## Metrics

- **Movement time**: start click through the successful 66 ms settled state.
- **Switchbacks**: direction reversals after the first movement beyond the far
  edge of the target band.
- **Maximum overshoot**: greatest distance beyond that far edge, in points.
- **Event count**: effective document movements accepted by the trial.

The session summary shows mean movement time, mean switchbacks, and largest
overshoot. CSV keeps every trial, condition, case dimension, metric, and event
count. Export uses the native Save Panel and an atomic local replacement.

## Observed Input Metrics

Debug Console also derives an arrival-rate distribution for each observed
device class from its bounded ring buffer:

- p50, p95, and maximum event rate;
- counts in `<30`, `30-60`, `60-120`, `120-240`, and `240+` Hz bins;
- identical timestamps from two axes count as one arrival;
- gaps over 150 ms are session boundaries, not very-low device rates.

These are rates observed by Auto Reverse after macOS scheduling. They are never
presented as a device's advertised polling rate.

## Event Tap Latency

**Sample now** uses Apple's public
[`CGGetEventTapList`](https://developer.apple.com/documentation/coregraphics/cggeteventtaplist(_:_:_:))
to show min/average/max microsecond latency for this process's active scroll
filter. Sampling is manual because CoreGraphics resets each listed tap's min
and max to its average after the read. The UI therefore calls it an interval
snapshot and never polls it.

## Ownership

- `src/scroll_benchmark.rs`: pure matrix validation and trial state machine.
- `src/event_rate.rs`: pure per-device observed-rate distributions.
- `src/statistics.rs`: shared nearest-rank integer distributions.
- `src/ui/scroll_benchmark.rs`: viewport, input-unit adapter, rendering and CSV.
- `src/ui/local_export.rs`: shared atomic local write and CSV escaping.
- `src/platform/macos/tap_metrics.rs`: bounded CoreGraphics adapter.
- `src/ui/debug_console.rs`: diagnostics presentation and explicit sampling.

The remaining release gate is physical/manual QA across real wheel, trackpad,
and Magic Mouse sessions in light and dark mode. Automated tests cover matrix
bounds, completion timing, switchbacks, overshoot, idle-gap exclusion, CSV,
tap selection, and invalid latency values.
