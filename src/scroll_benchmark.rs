//! Pure ScrollTest-style target-acquisition state and reproducible matrices.

use std::error::Error;
use std::fmt;

pub const SETTLE_TIME_US: u64 = 66_000;
pub const MAX_BENCHMARK_CASES: usize = 128;
pub const MAX_DISTANCE_POINTS: u32 = 100_000;
pub const MAX_VIEWPORT_HEIGHT_POINTS: u32 = 1_200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetMode {
    Known,
    Unknown,
}

impl TargetMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Known => "known",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for TargetMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BenchmarkCase {
    pub distance_points: u32,
    pub viewport_height_points: u32,
    pub tolerance_points: u32,
}

impl BenchmarkCase {
    pub fn validate(self) -> Result<Self, BenchmarkError> {
        if self.distance_points == 0 || self.distance_points > MAX_DISTANCE_POINTS {
            return Err(BenchmarkError::InvalidDistance(self.distance_points));
        }
        if !(160..=MAX_VIEWPORT_HEIGHT_POINTS).contains(&self.viewport_height_points) {
            return Err(BenchmarkError::InvalidViewportHeight(
                self.viewport_height_points,
            ));
        }
        if self.tolerance_points == 0
            || self.tolerance_points.saturating_mul(2) >= self.viewport_height_points
            || self.tolerance_points >= self.distance_points
        {
            return Err(BenchmarkError::InvalidTolerance {
                tolerance: self.tolerance_points,
                distance: self.distance_points,
                viewport_height: self.viewport_height_points,
            });
        }
        Ok(self)
    }

    pub fn lower_bound(self) -> f64 {
        f64::from(self.distance_points - self.tolerance_points)
    }

    pub fn upper_bound(self) -> f64 {
        f64::from(self.distance_points + self.tolerance_points)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkMatrix {
    cases: Vec<BenchmarkCase>,
}

impl BenchmarkMatrix {
    pub fn compact() -> Self {
        Self::from_axes(&[240, 960, 2_880], &[240, 360], &[12, 32])
            .expect("the built-in compact benchmark matrix is valid")
    }

    pub fn full() -> Self {
        Self::from_axes(&[160, 480, 1_440, 4_320], &[240, 360, 480], &[8, 20, 40])
            .expect("the built-in full benchmark matrix is valid")
    }

    pub fn from_axes(
        distances: &[u32],
        viewport_heights: &[u32],
        tolerances: &[u32],
    ) -> Result<Self, BenchmarkError> {
        if distances.is_empty() || viewport_heights.is_empty() || tolerances.is_empty() {
            return Err(BenchmarkError::EmptyMatrixAxis);
        }
        let count = distances
            .len()
            .checked_mul(viewport_heights.len())
            .and_then(|count| count.checked_mul(tolerances.len()))
            .ok_or(BenchmarkError::TooManyCases(usize::MAX))?;
        if count > MAX_BENCHMARK_CASES {
            return Err(BenchmarkError::TooManyCases(count));
        }

        let mut cases = Vec::with_capacity(count);
        for &distance_points in distances {
            for &viewport_height_points in viewport_heights {
                for &tolerance_points in tolerances {
                    cases.push(
                        BenchmarkCase {
                            distance_points,
                            viewport_height_points,
                            tolerance_points,
                        }
                        .validate()?,
                    );
                }
            }
        }
        Ok(Self { cases })
    }

    pub fn cases(&self) -> &[BenchmarkCase] {
        &self.cases
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrialResult {
    pub target_mode: TargetMode,
    pub case: BenchmarkCase,
    pub movement_time_us: u64,
    pub switchback_count: usize,
    pub maximum_overshoot_points: f64,
    pub event_count: usize,
}

#[derive(Debug, Clone)]
pub struct BenchmarkTrial {
    target_mode: TargetMode,
    case: BenchmarkCase,
    started_us: u64,
    last_timestamp_us: u64,
    last_movement_us: u64,
    position_points: f64,
    last_direction: i8,
    has_overshot: bool,
    switchback_count: usize,
    maximum_overshoot_points: f64,
    event_count: usize,
    result: Option<TrialResult>,
}

impl BenchmarkTrial {
    pub fn new(
        target_mode: TargetMode,
        case: BenchmarkCase,
        started_us: u64,
    ) -> Result<Self, BenchmarkError> {
        Ok(Self {
            target_mode,
            case: case.validate()?,
            started_us,
            last_timestamp_us: started_us,
            last_movement_us: started_us,
            position_points: 0.0,
            last_direction: 0,
            has_overshot: false,
            switchback_count: 0,
            maximum_overshoot_points: 0.0,
            event_count: 0,
            result: None,
        })
    }

    /// Applies document movement in logical points. Positive values move
    /// toward the target; position is clamped at the document origin.
    pub fn apply_delta(
        &mut self,
        timestamp_us: u64,
        document_delta_points: f64,
    ) -> Result<(), BenchmarkError> {
        self.check_timestamp(timestamp_us)?;
        if self.result.is_some() {
            return Err(BenchmarkError::TrialFinished);
        }
        if !document_delta_points.is_finite() {
            return Err(BenchmarkError::NonFiniteDelta);
        }
        if document_delta_points.abs() <= f64::EPSILON {
            return Ok(());
        }

        let previous_position = self.position_points;
        self.position_points = (self.position_points + document_delta_points).max(0.0);
        let applied_delta = self.position_points - previous_position;
        if applied_delta.abs() <= f64::EPSILON {
            return Ok(());
        }

        let direction = if applied_delta.is_sign_positive() {
            1
        } else {
            -1
        };
        if self.has_overshot && self.last_direction != 0 && self.last_direction != direction {
            self.switchback_count += 1;
        }
        self.last_direction = direction;
        self.last_movement_us = timestamp_us;
        self.event_count += 1;

        let overshoot = (self.position_points - self.case.upper_bound()).max(0.0);
        if overshoot > 0.0 {
            self.has_overshot = true;
            self.maximum_overshoot_points = self.maximum_overshoot_points.max(overshoot);
        }
        Ok(())
    }

    pub fn finish_if_settled(
        &mut self,
        timestamp_us: u64,
    ) -> Result<Option<TrialResult>, BenchmarkError> {
        self.check_timestamp(timestamp_us)?;
        if let Some(result) = self.result {
            return Ok(Some(result));
        }
        if self.event_count == 0
            || !self.target_is_in_tolerance()
            || timestamp_us.saturating_sub(self.last_movement_us) < SETTLE_TIME_US
        {
            return Ok(None);
        }

        let result = TrialResult {
            target_mode: self.target_mode,
            case: self.case,
            movement_time_us: timestamp_us.saturating_sub(self.started_us),
            switchback_count: self.switchback_count,
            maximum_overshoot_points: self.maximum_overshoot_points,
            event_count: self.event_count,
        };
        self.result = Some(result);
        Ok(Some(result))
    }

    pub fn case(&self) -> BenchmarkCase {
        self.case
    }

    pub fn target_mode(&self) -> TargetMode {
        self.target_mode
    }

    pub fn position_points(&self) -> f64 {
        self.position_points
    }

    pub fn target_is_in_tolerance(&self) -> bool {
        (self.case.lower_bound()..=self.case.upper_bound()).contains(&self.position_points)
    }

    pub fn switchback_count(&self) -> usize {
        self.switchback_count
    }

    pub fn maximum_overshoot_points(&self) -> f64 {
        self.maximum_overshoot_points
    }

    fn check_timestamp(&mut self, timestamp_us: u64) -> Result<(), BenchmarkError> {
        if timestamp_us < self.last_timestamp_us {
            return Err(BenchmarkError::TimestampOutOfOrder {
                previous: self.last_timestamp_us,
                current: timestamp_us,
            });
        }
        self.last_timestamp_us = timestamp_us;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BenchmarkError {
    EmptyMatrixAxis,
    TooManyCases(usize),
    InvalidDistance(u32),
    InvalidViewportHeight(u32),
    InvalidTolerance {
        tolerance: u32,
        distance: u32,
        viewport_height: u32,
    },
    TimestampOutOfOrder {
        previous: u64,
        current: u64,
    },
    NonFiniteDelta,
    TrialFinished,
}

impl fmt::Display for BenchmarkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyMatrixAxis => f.write_str("benchmark matrix axes cannot be empty"),
            Self::TooManyCases(count) => write!(
                f,
                "benchmark matrix has {count} cases; the limit is {MAX_BENCHMARK_CASES}"
            ),
            Self::InvalidDistance(value) => write!(
                f,
                "distance must be between 1 and {MAX_DISTANCE_POINTS} points, found {value}"
            ),
            Self::InvalidViewportHeight(value) => write!(
                f,
                "viewport height must be between 160 and {MAX_VIEWPORT_HEIGHT_POINTS} points, found {value}"
            ),
            Self::InvalidTolerance {
                tolerance,
                distance,
                viewport_height,
            } => write!(
                f,
                "tolerance {tolerance} must be positive and smaller than distance {distance} and half viewport height {viewport_height}"
            ),
            Self::TimestampOutOfOrder { previous, current } => write!(
                f,
                "benchmark timestamp moved backwards from {previous} to {current} microseconds"
            ),
            Self::NonFiniteDelta => f.write_str("benchmark delta must be finite"),
            Self::TrialFinished => f.write_str("benchmark trial is already finished"),
        }
    }
}

impl Error for BenchmarkError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn case() -> BenchmarkCase {
        BenchmarkCase {
            distance_points: 100,
            viewport_height_points: 240,
            tolerance_points: 10,
        }
    }

    #[test]
    fn compact_matrix_crosses_multiple_distances_viewports_and_tolerances() {
        let matrix = BenchmarkMatrix::compact();
        assert_eq!(matrix.cases().len(), 12);
        assert_eq!(matrix.cases()[0].distance_points, 240);
        assert_eq!(matrix.cases()[11].distance_points, 2_880);
        assert!(
            matrix
                .cases()
                .iter()
                .any(|case| case.viewport_height_points == 360)
        );
        assert!(
            matrix
                .cases()
                .iter()
                .any(|case| case.tolerance_points == 32)
        );
    }

    #[test]
    fn target_must_settle_for_sixty_six_milliseconds() {
        let mut trial = BenchmarkTrial::new(TargetMode::Known, case(), 1_000).unwrap();
        trial.apply_delta(10_000, 100.0).unwrap();

        assert_eq!(trial.finish_if_settled(75_999).unwrap(), None);
        let result = trial.finish_if_settled(76_000).unwrap().unwrap();
        assert_eq!(result.movement_time_us, 75_000);
        assert_eq!(result.switchback_count, 0);
        assert_eq!(result.maximum_overshoot_points, 0.0);
    }

    #[test]
    fn switchbacks_only_start_after_overshooting_target_frame() {
        let mut trial = BenchmarkTrial::new(TargetMode::Unknown, case(), 0).unwrap();
        trial.apply_delta(1, 40.0).unwrap();
        trial.apply_delta(2, -10.0).unwrap();
        assert_eq!(trial.switchback_count(), 0);

        trial.apply_delta(3, 90.0).unwrap();
        assert_eq!(trial.maximum_overshoot_points(), 10.0);
        trial.apply_delta(4, -20.0).unwrap();
        assert_eq!(trial.switchback_count(), 1);
        trial.apply_delta(5, 5.0).unwrap();
        assert_eq!(trial.switchback_count(), 2);
    }

    #[test]
    fn origin_clamping_does_not_create_phantom_events() {
        let mut trial = BenchmarkTrial::new(TargetMode::Known, case(), 0).unwrap();
        trial.apply_delta(1, -50.0).unwrap();
        assert_eq!(trial.position_points(), 0.0);
        assert_eq!(trial.finish_if_settled(SETTLE_TIME_US + 1).unwrap(), None);
    }

    #[test]
    fn invalid_matrices_and_timestamps_are_rejected() {
        assert!(matches!(
            BenchmarkMatrix::from_axes(&[], &[240], &[10]),
            Err(BenchmarkError::EmptyMatrixAxis)
        ));
        assert!(matches!(
            BenchmarkCase {
                tolerance_points: 120,
                ..case()
            }
            .validate(),
            Err(BenchmarkError::InvalidTolerance { .. })
        ));

        let mut trial = BenchmarkTrial::new(TargetMode::Known, case(), 10).unwrap();
        assert!(matches!(
            trial.apply_delta(9, 1.0),
            Err(BenchmarkError::TimestampOutOfOrder { .. })
        ));
    }
}
