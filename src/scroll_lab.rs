//! Pure transfer-function measurements over a validated [`ScrollTrace`].

use std::error::Error;
use std::fmt;

use crate::config::AppConfig;
use crate::diagnostics::Axis;
use crate::event_rate::{DeviceEventRate, EventRateSample, analyze_event_rates_with_gap};
use crate::scroll;
use crate::scroll_trace::{ReplayedSample, ScrollTrace, TraceSample};
pub use crate::statistics::Distribution;
use crate::statistics::distribution;

pub const DEFAULT_BASELINE_GAIN: u32 = 1;
pub const MAX_BASELINE_GAIN: u32 = 100;
pub const DEFAULT_CLUTCH_GAP_US: u64 = 150_000;
pub const MAX_CLUTCH_GAP_US: u64 = 60_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LabOptions {
    pub baseline_gain: u32,
    pub clutch_gap_us: u64,
}

impl Default for LabOptions {
    fn default() -> Self {
        Self {
            baseline_gain: DEFAULT_BASELINE_GAIN,
            clutch_gap_us: DEFAULT_CLUTCH_GAP_US,
        }
    }
}

impl LabOptions {
    pub fn validate(self) -> Result<Self, LabError> {
        if !(1..=MAX_BASELINE_GAIN).contains(&self.baseline_gain) {
            return Err(LabError::InvalidBaselineGain(self.baseline_gain));
        }
        if !(1..=MAX_CLUTCH_GAP_US).contains(&self.clutch_gap_us) {
            return Err(LabError::InvalidClutchGap(self.clutch_gap_us));
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AxisMetrics {
    pub sample_count: usize,
    pub input_signed_distance: i128,
    pub input_absolute_distance: u128,
    pub observed_signed_distance: i128,
    pub observed_absolute_distance: u128,
    pub replayed_signed_distance: i128,
    pub replayed_absolute_distance: u128,
    pub baseline_signed_distance: i128,
    pub baseline_absolute_distance: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferPoint {
    pub timestamp_us: u64,
    pub interval_us: u64,
    pub axis: Axis,
    pub input_delta: i64,
    pub observed_output_delta: i64,
    pub replayed_output_delta: i64,
    pub baseline_output_delta: i64,
    pub omitted_context: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabReport {
    pub schema_version: u32,
    pub sample_count: usize,
    pub discrete_sample_count: usize,
    pub continuous_sample_count: usize,
    pub duration_us: u64,
    pub session_count: usize,
    pub direction_change_count: usize,
    pub replay_match_count: usize,
    pub omitted_context_count: usize,
    pub magnitude: Distribution,
    pub intervals: Option<Distribution>,
    pub event_rates: Vec<DeviceEventRate>,
    pub baseline_gain: u32,
    pub clutch_gap_us: u64,
    pub vertical: AxisMetrics,
    pub horizontal: AxisMetrics,
    pub points: Vec<TransferPoint>,
}

impl LabReport {
    pub fn replay_match_percent(&self) -> f64 {
        self.replay_match_count as f64 * 100.0 / self.sample_count as f64
    }
}

pub fn analyze(
    trace: &ScrollTrace,
    config: &AppConfig,
    options: LabOptions,
) -> Result<LabReport, LabError> {
    let options = options.validate()?;
    let replay = trace.replay(config);
    let mut baseline_config = config.clone();
    baseline_config.discrete_scroll_step_size = 0;

    let unique_timestamps = unique_timestamps(trace.samples());
    let intervals = unique_timestamps
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .collect::<Vec<_>>();
    let duration_us = unique_timestamps.last().copied().unwrap_or(0);
    let session_count = 1 + intervals
        .iter()
        .filter(|interval| **interval > options.clutch_gap_us)
        .count();

    let mut vertical = AxisMetrics::default();
    let mut horizontal = AxisMetrics::default();
    let mut points = Vec::with_capacity(trace.samples().len());
    let mut previous_timestamp = 0;

    for (sample, replayed) in trace.samples().iter().zip(replay.samples()) {
        let baseline_output_delta =
            constant_gain_output(sample, &baseline_config, options.baseline_gain);
        let interval_us = sample.timestamp_us.saturating_sub(previous_timestamp);
        previous_timestamp = sample.timestamp_us;

        let metrics = match sample.axis {
            Axis::Vertical => &mut vertical,
            Axis::Horizontal => &mut horizontal,
        };
        update_axis_metrics(metrics, replayed, baseline_output_delta);
        points.push(TransferPoint {
            timestamp_us: sample.timestamp_us,
            interval_us,
            axis: sample.axis,
            input_delta: sample.input_delta,
            observed_output_delta: sample.observed_output_delta,
            replayed_output_delta: replayed.replayed_output_delta,
            baseline_output_delta,
            omitted_context: replayed.omitted_context,
        });
    }

    let magnitudes = trace
        .samples()
        .iter()
        .map(|sample| sample.input_delta.unsigned_abs())
        .collect::<Vec<_>>();

    Ok(LabReport {
        schema_version: trace.schema_version(),
        sample_count: trace.samples().len(),
        discrete_sample_count: trace
            .samples()
            .iter()
            .filter(|sample| !sample.continuous)
            .count(),
        continuous_sample_count: trace
            .samples()
            .iter()
            .filter(|sample| sample.continuous)
            .count(),
        duration_us,
        session_count,
        direction_change_count: direction_changes(trace.samples(), options.clutch_gap_us),
        replay_match_count: replay
            .samples()
            .iter()
            .filter(|sample| sample.matches_observed())
            .count(),
        omitted_context_count: replay
            .samples()
            .iter()
            .filter(|sample| sample.omitted_context)
            .count(),
        magnitude: distribution(magnitudes).expect("a validated trace is non-empty"),
        intervals: distribution(intervals),
        event_rates: analyze_event_rates_with_gap(
            &trace
                .samples()
                .iter()
                .map(|sample| EventRateSample {
                    timestamp_us: sample.timestamp_us,
                    device_kind: sample.device_kind,
                })
                .collect::<Vec<_>>(),
            options.clutch_gap_us,
        ),
        baseline_gain: options.baseline_gain,
        clutch_gap_us: options.clutch_gap_us,
        vertical,
        horizontal,
        points,
    })
}

fn unique_timestamps(samples: &[TraceSample]) -> Vec<u64> {
    let mut timestamps = Vec::with_capacity(samples.len());
    for sample in samples {
        if timestamps.last().copied() != Some(sample.timestamp_us) {
            timestamps.push(sample.timestamp_us);
        }
    }
    timestamps
}

fn constant_gain_output(sample: &TraceSample, config: &AppConfig, gain: u32) -> i64 {
    let decision = scroll::transform_event(config, sample.to_scroll_event());
    let (output, reversed) = match sample.axis {
        Axis::Vertical => (
            decision.transformed.delta_vertical,
            decision.vertical_reversed,
        ),
        Axis::Horizontal => (
            decision.transformed.delta_horizontal,
            decision.horizontal_reversed,
        ),
    };
    if !sample.continuous && reversed {
        output.saturating_mul(i64::from(gain))
    } else {
        output
    }
}

fn update_axis_metrics(
    metrics: &mut AxisMetrics,
    replayed: &ReplayedSample,
    baseline_output_delta: i64,
) {
    metrics.sample_count += 1;
    add_distance(
        &mut metrics.input_signed_distance,
        &mut metrics.input_absolute_distance,
        replayed.input_delta,
    );
    add_distance(
        &mut metrics.observed_signed_distance,
        &mut metrics.observed_absolute_distance,
        replayed.observed_output_delta,
    );
    add_distance(
        &mut metrics.replayed_signed_distance,
        &mut metrics.replayed_absolute_distance,
        replayed.replayed_output_delta,
    );
    add_distance(
        &mut metrics.baseline_signed_distance,
        &mut metrics.baseline_absolute_distance,
        baseline_output_delta,
    );
}

fn add_distance(signed: &mut i128, absolute: &mut u128, value: i64) {
    *signed += i128::from(value);
    *absolute += u128::from(value.unsigned_abs());
}

fn direction_changes(samples: &[TraceSample], clutch_gap_us: u64) -> usize {
    let mut vertical_sign = 0;
    let mut horizontal_sign = 0;
    let mut changes = 0;
    let mut previous_timestamp = None;

    for sample in samples {
        if previous_timestamp
            .is_some_and(|previous| sample.timestamp_us.saturating_sub(previous) > clutch_gap_us)
        {
            vertical_sign = 0;
            horizontal_sign = 0;
        }
        previous_timestamp = Some(sample.timestamp_us);

        let sign = sample.input_delta.signum();
        if sign == 0 {
            continue;
        }
        let previous = match sample.axis {
            Axis::Vertical => &mut vertical_sign,
            Axis::Horizontal => &mut horizontal_sign,
        };
        if *previous != 0 && *previous != sign {
            changes += 1;
        }
        *previous = sign;
    }
    changes
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabError {
    InvalidBaselineGain(u32),
    InvalidClutchGap(u64),
}

impl fmt::Display for LabError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBaselineGain(value) => write!(
                f,
                "baseline gain must be between 1 and {MAX_BASELINE_GAIN}, found {value}"
            ),
            Self::InvalidClutchGap(value) => write!(
                f,
                "clutch gap must be between 1 and {MAX_CLUTCH_GAP_US} microseconds, found {value}"
            ),
        }
    }
}

impl Error for LabError {}

#[cfg(test)]
mod tests {
    use crate::device::DeviceKind;
    use crate::diagnostics::DecisionReason;

    use super::*;

    fn sample(timestamp_us: u64, axis: Axis, delta: i64) -> TraceSample {
        TraceSample {
            timestamp_us,
            device_kind: DeviceKind::Mouse,
            continuous: false,
            axis,
            input_delta: delta,
            observed_output_delta: delta.saturating_mul(-3),
            decision_reason: DecisionReason::Reversed,
        }
    }

    #[test]
    fn report_covers_magnitude_intervals_direction_duration_and_clutching() {
        let trace = ScrollTrace::new(vec![
            sample(0, Axis::Vertical, 1),
            sample(10_000, Axis::Vertical, -3),
            sample(20_000, Axis::Horizontal, 2),
            sample(300_000, Axis::Vertical, -4),
        ])
        .unwrap();

        let report = analyze(&trace, &AppConfig::default(), LabOptions::default()).unwrap();

        assert_eq!(report.duration_us, 300_000);
        assert_eq!(report.session_count, 2);
        assert_eq!(report.direction_change_count, 1);
        assert_eq!(
            report.magnitude,
            Distribution {
                min: 1,
                p50: 2,
                p95: 4,
                max: 4,
            }
        );
        assert_eq!(report.intervals.unwrap().max, 280_000);
    }

    #[test]
    fn constant_gain_baseline_is_independent_of_input_magnitude() {
        let trace = ScrollTrace::new(vec![
            sample(0, Axis::Vertical, 1),
            sample(10, Axis::Vertical, 5),
        ])
        .unwrap();
        let options = LabOptions {
            baseline_gain: 2,
            ..LabOptions::default()
        };

        let report = analyze(&trace, &AppConfig::default(), options).unwrap();

        assert_eq!(report.points[0].baseline_output_delta, -2);
        assert_eq!(report.points[1].baseline_output_delta, -10);
        // Current policy applies discrete_scroll_step_size only to +/-1.
        assert_eq!(report.points[0].replayed_output_delta, -3);
        assert_eq!(report.points[1].replayed_output_delta, -5);
    }

    #[test]
    fn baseline_does_not_add_gain_to_continuous_input() {
        let mut continuous = sample(0, Axis::Vertical, 4);
        continuous.device_kind = DeviceKind::Trackpad;
        continuous.continuous = true;
        continuous.observed_output_delta = 4;
        continuous.decision_reason = DecisionReason::TrackpadNatural;
        let trace = ScrollTrace::new(vec![continuous]).unwrap();

        let report = analyze(
            &trace,
            &AppConfig::default(),
            LabOptions {
                baseline_gain: 10,
                ..LabOptions::default()
            },
        )
        .unwrap();

        assert_eq!(report.points[0].baseline_output_delta, 4);
        assert_eq!(report.continuous_sample_count, 1);
    }

    #[test]
    fn axis_metrics_use_wide_accumulators_and_keep_sign() {
        let trace = ScrollTrace::new(vec![
            sample(0, Axis::Vertical, i64::MAX),
            sample(1, Axis::Vertical, i64::MAX),
        ])
        .unwrap();

        let report = analyze(&trace, &AppConfig::default(), LabOptions::default()).unwrap();

        assert_eq!(
            report.vertical.input_signed_distance,
            i128::from(i64::MAX) * 2
        );
        assert_eq!(
            report.vertical.input_absolute_distance,
            u128::from(i64::MAX as u64) * 2
        );
    }

    #[test]
    fn options_are_bounded() {
        assert!(matches!(
            LabOptions {
                baseline_gain: 0,
                ..LabOptions::default()
            }
            .validate(),
            Err(LabError::InvalidBaselineGain(0))
        ));
        assert!(matches!(
            LabOptions {
                clutch_gap_us: 0,
                ..LabOptions::default()
            }
            .validate(),
            Err(LabError::InvalidClutchGap(0))
        ));
    }
}
