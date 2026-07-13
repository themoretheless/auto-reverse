# Scroll Trace Contract

Auto Reverse privacy traces are small, versioned TOML files for deterministic
policy replay and transfer-function measurements. They are generated locally
from the Debug Console's existing 500-row ring buffer and are never uploaded.

## Create A Trace

1. Open the Debug Console from the menu bar.
2. Scroll through the scenario to compare.
3. Choose Export -> Privacy trace.
4. Select a local destination in the native Save Panel.

The current Debug Console filter is applied before export. Detailed CSV remains
available as a separate Export menu item for support diagnostics.

## Analyze A Trace

```bash
cargo run -- trace-lab /path/to/scroll-trace.toml
cargo run -- trace-lab /path/to/scroll-trace.toml \
  --baseline-gain 2 \
  --clutch-gap-ms 150
```

`trace-lab` uses the current config when it exists and defaults without creating
a config otherwise. It reports:

- input magnitude min/p50/p95/max;
- event interval min/p50/p95/max;
- duration and clutch-session count;
- direction changes;
- observed-versus-replayed matches;
- vertical and horizontal signed/absolute distances;
- a constant-gain baseline beside the current policy.

Percentiles use the nearest-rank definition. Direction-change state resets
when the configured clutch gap starts a new session.

Baseline gain applies only to reversed discrete-wheel axes. Continuous
Trackpad/Magic Mouse samples are never amplified by the baseline.

## Schema Version 1

```toml
schema_version = 1

[[samples]]
timestamp_us = 0
device_kind = "mouse"
continuous = false
axis = "vertical"
input_delta = 1
observed_output_delta = -3
decision_reason = "reversed"

[[samples]]
timestamp_us = 8000
device_kind = "mouse"
continuous = false
axis = "vertical"
input_delta = 4
observed_output_delta = -4
decision_reason = "reversed"
```

Fields:

- `timestamp_us`: monotonic microseconds relative to the first exported row;
- `device_kind`: `mouse`, `trackpad`, `magic-mouse`, or `unknown`;
- `continuous`: the normalized source class observed by the runtime;
- `axis`: `vertical` or `horizontal`;
- `input_delta`: normalized input on that axis;
- `observed_output_delta`: output produced during capture;
- `decision_reason`: stable code shared with Debug Console diagnostics.

## Bounds And Validation

- Maximum serialized/imported size: 1 MiB.
- Maximum sample count: 10,000. The current GUI source holds at most 500 rows.
- A trace must contain at least one sample.
- The first timestamp must be zero.
- Timestamps must be nondecreasing.
- Unknown fields and unsupported schema versions are rejected.
- CLI reading is capped before the whole input can be allocated.

## Privacy Boundary

Privacy traces contain no:

- wall-clock timestamp;
- process ID;
- application or window name;
- HID product name;
- vendor/product ID;
- serial number or connection location;
- pointer coordinates, touch positions, key data, or arbitrary HID payload.

`synthetic_event` and `raw_input_guard` can be replayed from their decision code
using nonidentifying sentinel values. `temporarily_paused` and
`device_rule_reversed`/`device_rule_disabled` require process-local state or
physical identity that is intentionally omitted; the lab counts those rows as
requiring omitted context instead of pretending the replay is exact.

## Ownership

- `src/diagnostics.rs`: stable axis/reason vocabulary.
- `src/scroll_trace.rs`: schema, bounds, TOML and deterministic replay.
- `src/scroll_lab.rs`: distributions, sessions, direction changes and baseline.
- `src/ui/debug_console/export.rs`: Debug Console projection and atomic export.
- `src/main.rs`: bounded file loading and text report orchestration.

Changing fields requires a schema-version decision, fixtures for the previous
version, and an update to this document before release.
