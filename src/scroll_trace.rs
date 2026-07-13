//! Versioned, privacy-bounded scroll traces and deterministic pure replay.
//!
//! A trace deliberately contains no process IDs, application/window names,
//! HID names, vendor/product IDs, serials, locations, or absolute wall-clock
//! time. It is suitable for local algorithm comparison, not device identity
//! debugging.

use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::config::AppConfig;
use crate::device::DeviceKind;
use crate::diagnostics::{Axis, DecisionReason};
use crate::input::ScrollEvent;
use crate::scroll;

pub const TRACE_SCHEMA_VERSION: u32 = 1;
pub const MAX_TRACE_SAMPLES: usize = 10_000;
pub const MAX_TRACE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceSample {
    /// Microseconds since the first exported sample, never wall-clock time.
    pub timestamp_us: u64,
    pub device_kind: DeviceKind,
    pub continuous: bool,
    pub axis: Axis,
    pub input_delta: i64,
    pub observed_output_delta: i64,
    pub decision_reason: DecisionReason,
}

impl TraceSample {
    pub(crate) fn to_scroll_event(&self) -> ScrollEvent {
        let (delta_vertical, delta_horizontal) = match self.axis {
            Axis::Vertical => (self.input_delta, 0),
            Axis::Horizontal => (0, self.input_delta),
        };

        ScrollEvent {
            device_kind: self.device_kind,
            delta_vertical,
            delta_horizontal,
            continuous: self.continuous,
            // These two privacy-sensitive contexts are reproducible from the
            // stable reason without storing a real PID or injected-event tag.
            synthetic: self.decision_reason == DecisionReason::SyntheticEvent,
            source_pid: i64::from(self.decision_reason == DecisionReason::RawInputGuard),
            identity: None,
        }
    }

    pub fn requires_omitted_context(&self) -> bool {
        matches!(
            self.decision_reason,
            DecisionReason::TemporarilyPaused
                | DecisionReason::DeviceRuleReversed
                | DecisionReason::DeviceRuleDisabled
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScrollTrace {
    schema_version: u32,
    samples: Vec<TraceSample>,
}

impl ScrollTrace {
    pub fn new(samples: Vec<TraceSample>) -> Result<Self, TraceError> {
        let trace = Self {
            schema_version: TRACE_SCHEMA_VERSION,
            samples,
        };
        trace.validate()?;
        Ok(trace)
    }

    pub fn from_toml(contents: &str) -> Result<Self, TraceError> {
        if contents.len() > MAX_TRACE_BYTES {
            return Err(TraceError::TooManyBytes {
                actual: contents.len(),
                maximum: MAX_TRACE_BYTES,
            });
        }
        let trace: Self = toml::from_str(contents).map_err(TraceError::Parse)?;
        trace.validate()?;
        Ok(trace)
    }

    pub fn to_toml(&self) -> Result<String, TraceError> {
        self.validate()?;
        let serialized = toml::to_string_pretty(self).map_err(TraceError::Serialize)?;
        if serialized.len() > MAX_TRACE_BYTES {
            return Err(TraceError::TooManyBytes {
                actual: serialized.len(),
                maximum: MAX_TRACE_BYTES,
            });
        }
        Ok(serialized)
    }

    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn samples(&self) -> &[TraceSample] {
        &self.samples
    }

    pub fn replay(&self, config: &AppConfig) -> TraceReplay {
        let samples = self
            .samples
            .iter()
            .map(|sample| {
                let decision = scroll::transform_event(config, sample.to_scroll_event());
                let replayed_output_delta = match sample.axis {
                    Axis::Vertical => decision.transformed.delta_vertical,
                    Axis::Horizontal => decision.transformed.delta_horizontal,
                };
                let axis_reversed = match sample.axis {
                    Axis::Vertical => decision.vertical_reversed,
                    Axis::Horizontal => decision.horizontal_reversed,
                };

                ReplayedSample {
                    timestamp_us: sample.timestamp_us,
                    device_kind: sample.device_kind,
                    continuous: sample.continuous,
                    axis: sample.axis,
                    input_delta: sample.input_delta,
                    observed_output_delta: sample.observed_output_delta,
                    replayed_output_delta,
                    observed_reason: sample.decision_reason,
                    axis_reversed,
                    step_size_applied: decision.step_size_applied,
                    omitted_context: sample.requires_omitted_context(),
                }
            })
            .collect();
        TraceReplay { samples }
    }

    fn validate(&self) -> Result<(), TraceError> {
        if self.schema_version != TRACE_SCHEMA_VERSION {
            return Err(TraceError::UnsupportedVersion {
                found: self.schema_version,
                supported: TRACE_SCHEMA_VERSION,
            });
        }
        if self.samples.is_empty() {
            return Err(TraceError::Empty);
        }
        if self.samples.len() > MAX_TRACE_SAMPLES {
            return Err(TraceError::TooManySamples {
                actual: self.samples.len(),
                maximum: MAX_TRACE_SAMPLES,
            });
        }
        if self.samples[0].timestamp_us != 0 {
            return Err(TraceError::NonZeroOrigin(self.samples[0].timestamp_us));
        }
        for (index, pair) in self.samples.windows(2).enumerate() {
            if pair[1].timestamp_us < pair[0].timestamp_us {
                return Err(TraceError::TimestampOutOfOrder {
                    index: index + 1,
                    previous: pair[0].timestamp_us,
                    current: pair[1].timestamp_us,
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayedSample {
    pub timestamp_us: u64,
    pub device_kind: DeviceKind,
    pub continuous: bool,
    pub axis: Axis,
    pub input_delta: i64,
    pub observed_output_delta: i64,
    pub replayed_output_delta: i64,
    pub observed_reason: DecisionReason,
    pub axis_reversed: bool,
    pub step_size_applied: bool,
    pub omitted_context: bool,
}

impl ReplayedSample {
    pub fn matches_observed(&self) -> bool {
        self.observed_output_delta == self.replayed_output_delta
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceReplay {
    samples: Vec<ReplayedSample>,
}

impl TraceReplay {
    pub fn samples(&self) -> &[ReplayedSample] {
        &self.samples
    }
}

#[derive(Debug)]
pub enum TraceError {
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
    UnsupportedVersion {
        found: u32,
        supported: u32,
    },
    Empty,
    TooManySamples {
        actual: usize,
        maximum: usize,
    },
    TooManyBytes {
        actual: usize,
        maximum: usize,
    },
    NonZeroOrigin(u64),
    TimestampOutOfOrder {
        index: usize,
        previous: u64,
        current: u64,
    },
}

impl fmt::Display for TraceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "could not parse trace TOML: {error}"),
            Self::Serialize(error) => write!(f, "could not serialize trace TOML: {error}"),
            Self::UnsupportedVersion { found, supported } => write!(
                f,
                "trace schema version {found} is unsupported; this build supports {supported}"
            ),
            Self::Empty => write!(f, "trace has no samples"),
            Self::TooManySamples { actual, maximum } => write!(
                f,
                "trace has {actual} samples; the safety limit is {maximum}"
            ),
            Self::TooManyBytes { actual, maximum } => {
                write!(f, "trace is {actual} bytes; the safety limit is {maximum}")
            }
            Self::NonZeroOrigin(timestamp) => write!(
                f,
                "the first trace timestamp must be 0 microseconds, found {timestamp}"
            ),
            Self::TimestampOutOfOrder {
                index,
                previous,
                current,
            } => write!(
                f,
                "trace timestamp at sample {index} moved backwards from {previous} to {current} microseconds"
            ),
        }
    }
}

impl Error for TraceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            Self::Serialize(error) => Some(error),
            Self::UnsupportedVersion { .. }
            | Self::Empty
            | Self::TooManySamples { .. }
            | Self::TooManyBytes { .. }
            | Self::NonZeroOrigin(_)
            | Self::TimestampOutOfOrder { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(timestamp_us: u64, input_delta: i64) -> TraceSample {
        TraceSample {
            timestamp_us,
            device_kind: DeviceKind::Mouse,
            continuous: false,
            axis: Axis::Vertical,
            input_delta,
            observed_output_delta: input_delta.saturating_mul(-3),
            decision_reason: DecisionReason::Reversed,
        }
    }

    #[test]
    fn trace_round_trips_without_private_identity_fields() {
        let trace = ScrollTrace::new(vec![sample(0, 1), sample(8_000, 2)]).unwrap();

        let serialized = trace.to_toml().unwrap();
        let decoded = ScrollTrace::from_toml(&serialized).unwrap();

        assert_eq!(decoded, trace);
        for forbidden in [
            "source_pid",
            "device_name",
            "vendor_id",
            "product_id",
            "serial_number",
            "location_id",
            "app_name",
            "window_title",
        ] {
            assert!(!serialized.contains(forbidden));
        }
    }

    #[test]
    fn unknown_fields_are_rejected_to_keep_the_privacy_contract_exact() {
        let toml = "schema_version = 1\n\
                    [[samples]]\n\
                    timestamp_us = 0\n\
                    device_kind = \"mouse\"\n\
                    continuous = false\n\
                    axis = \"vertical\"\n\
                    input_delta = 1\n\
                    observed_output_delta = -1\n\
                    decision_reason = \"reversed\"\n\
                    device_name = \"private\"\n";

        assert!(ScrollTrace::from_toml(toml).is_err());
    }

    #[test]
    fn validation_rejects_empty_oversized_and_non_relative_traces() {
        assert!(matches!(ScrollTrace::new(vec![]), Err(TraceError::Empty)));
        assert!(matches!(
            ScrollTrace::new(vec![sample(0, 1); MAX_TRACE_SAMPLES + 1]),
            Err(TraceError::TooManySamples { .. })
        ));
        assert!(matches!(
            ScrollTrace::new(vec![sample(1, 1)]),
            Err(TraceError::NonZeroOrigin(1))
        ));
    }

    #[test]
    fn validation_rejects_timestamps_that_move_backwards() {
        assert!(matches!(
            ScrollTrace::new(vec![sample(0, 1), sample(10, 1), sample(9, 1)]),
            Err(TraceError::TimestampOutOfOrder { index: 2, .. })
        ));
    }

    #[test]
    fn parser_rejects_unsupported_versions_and_oversized_input() {
        let unsupported = "schema_version = 2\n\
                           [[samples]]\n\
                           timestamp_us = 0\n\
                           device_kind = \"mouse\"\n\
                           continuous = false\n\
                           axis = \"vertical\"\n\
                           input_delta = 1\n\
                           observed_output_delta = -1\n\
                           decision_reason = \"reversed\"\n";
        assert!(matches!(
            ScrollTrace::from_toml(unsupported),
            Err(TraceError::UnsupportedVersion {
                found: 2,
                supported: TRACE_SCHEMA_VERSION
            })
        ));

        let oversized = "x".repeat(MAX_TRACE_BYTES + 1);
        assert!(matches!(
            ScrollTrace::from_toml(&oversized),
            Err(TraceError::TooManyBytes { .. })
        ));
    }

    #[test]
    fn replay_is_deterministic_for_the_same_trace_and_config() {
        let trace = ScrollTrace::new(vec![sample(0, 1), sample(8_000, 2)]).unwrap();
        let config = AppConfig::default();

        let first = trace.replay(&config);
        let second = trace.replay(&config);

        assert_eq!(first, second);
        assert_eq!(first.samples()[0].replayed_output_delta, -3);
        assert_eq!(first.samples()[1].replayed_output_delta, -2);
    }

    #[test]
    fn synthetic_and_injected_context_use_nonidentifying_sentinels() {
        let mut synthetic = sample(0, 1);
        synthetic.observed_output_delta = 1;
        synthetic.decision_reason = DecisionReason::SyntheticEvent;
        let mut injected = sample(1, 1);
        injected.observed_output_delta = 1;
        injected.decision_reason = DecisionReason::RawInputGuard;
        let trace = ScrollTrace::new(vec![synthetic, injected]).unwrap();
        let config = AppConfig {
            reverse_only_raw_input: true,
            ..AppConfig::default()
        };

        let replay = trace.replay(&config);

        assert!(
            replay
                .samples()
                .iter()
                .all(ReplayedSample::matches_observed)
        );
    }

    #[test]
    fn identity_and_runtime_only_decisions_are_marked_context_limited() {
        let mut paused = sample(0, 1);
        paused.decision_reason = DecisionReason::TemporarilyPaused;
        let mut ruled = sample(1, 1);
        ruled.decision_reason = DecisionReason::DeviceRuleDisabled;
        let mut rule_reversed = sample(2, 1);
        rule_reversed.decision_reason = DecisionReason::DeviceRuleReversed;
        let trace = ScrollTrace::new(vec![paused, ruled, rule_reversed]).unwrap();

        let replay = trace.replay(&AppConfig::default());

        assert!(replay.samples().iter().all(|sample| sample.omitted_context));
    }
}
