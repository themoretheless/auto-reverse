//! Pure, scalar-axis discrete-wheel dynamics.
//!
//! The engine owns no clock, thread, scheduler, CoreGraphics object, or
//! configuration store. Callers provide monotonic timestamps and decide when
//! to emit returned deltas. Live runtime integration remains deliberately off.

use std::error::Error;
use std::fmt;

pub const DISTANCE_EPSILON_POINTS: f64 = 1e-9;
pub const MAX_ABS_INPUT_POINTS: f64 = 1_000_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SmoothPreset {
    #[default]
    Off,
    Precise,
    Balanced,
    Fast,
}

impl SmoothPreset {
    pub const ALL: [Self; 4] = [Self::Off, Self::Precise, Self::Balanced, Self::Fast];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Precise => "precise",
            Self::Balanced => "balanced",
            Self::Fast => "fast",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Precise => "Precise",
            Self::Balanced => "Balanced",
            Self::Fast => "Fast",
        }
    }

    pub const fn goal(self) -> &'static str {
        match self {
            Self::Off => "Exact immediate pass-through",
            Self::Precise => "Longest control window for small corrections",
            Self::Balanced => "Middle response for general wheel use",
            Self::Fast => "Shortest response with the largest immediate share",
        }
    }

    pub const fn parameters(self) -> PresetParameters {
        match self {
            Self::Off => PresetParameters {
                immediate_per_mille: 1_000,
                tail_duration_us: 0,
            },
            Self::Precise => PresetParameters {
                immediate_per_mille: 350,
                tail_duration_us: 120_000,
            },
            Self::Balanced => PresetParameters {
                immediate_per_mille: 550,
                tail_duration_us: 90_000,
            },
            Self::Fast => PresetParameters {
                immediate_per_mille: 750,
                tail_duration_us: 60_000,
            },
        }
    }
}

impl fmt::Display for SmoothPreset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresetParameters {
    pub immediate_per_mille: u16,
    pub tail_duration_us: u64,
}

impl PresetParameters {
    pub fn validate(self) -> Result<Self, DynamicsError> {
        let valid = self.immediate_per_mille <= 1_000
            && if self.tail_duration_us == 0 {
                self.immediate_per_mille == 1_000
            } else {
                self.immediate_per_mille > 0 && self.immediate_per_mille < 1_000
            };
        if !valid {
            return Err(DynamicsError::InvalidParameters {
                immediate_per_mille: self.immediate_per_mille,
                tail_duration_us: self.tail_duration_us,
            });
        }
        Ok(self)
    }

    fn immediate_ratio(self) -> f64 {
        f64::from(self.immediate_per_mille) / 1_000.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DynamicsPhase {
    Idle,
    Active,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DynamicsEmission {
    pub delta_points: f64,
    pub pending_points: f64,
    pub phase: DynamicsPhase,
    pub deadline_us: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ActiveTail {
    pending_points: f64,
    last_sample_us: u64,
    deadline_us: u64,
}

#[derive(Debug, Clone)]
pub struct ScrollDynamics {
    preset: SmoothPreset,
    parameters: PresetParameters,
    last_timestamp_us: Option<u64>,
    tail: Option<ActiveTail>,
}

impl ScrollDynamics {
    pub fn new(preset: SmoothPreset) -> Self {
        let parameters = preset
            .parameters()
            .validate()
            .expect("built-in smooth presets must remain valid");
        Self {
            preset,
            parameters,
            last_timestamp_us: None,
            tail: None,
        }
    }

    pub fn preset(&self) -> SmoothPreset {
        self.preset
    }

    pub fn phase(&self) -> DynamicsPhase {
        if self.tail.is_some() {
            DynamicsPhase::Active
        } else {
            DynamicsPhase::Idle
        }
    }

    /// Accepts one already-normalized discrete-wheel delta for one axis.
    /// The returned delta is due immediately at `timestamp_us`.
    pub fn handle_input(
        &mut self,
        timestamp_us: u64,
        input_points: f64,
    ) -> Result<DynamicsEmission, DynamicsError> {
        self.validate_timestamp(timestamp_us)?;
        if !input_points.is_finite() {
            return Err(DynamicsError::NonFiniteInput);
        }
        if input_points.abs() > MAX_ABS_INPUT_POINTS {
            return Err(DynamicsError::InputTooLarge(input_points));
        }

        let new_deadline =
            if self.preset != SmoothPreset::Off && input_points.abs() > DISTANCE_EPSILON_POINTS {
                Some(
                    timestamp_us
                        .checked_add(self.parameters.tail_duration_us)
                        .ok_or(DynamicsError::TimestampOverflow {
                            timestamp_us,
                            duration_us: self.parameters.tail_duration_us,
                        })?,
                )
            } else {
                None
            };

        let due_tail = self.advance_tail(timestamp_us);
        self.last_timestamp_us = Some(timestamp_us);

        if input_points.abs() <= DISTANCE_EPSILON_POINTS {
            return Ok(self.emission(due_tail));
        }
        if self.preset == SmoothPreset::Off {
            return Ok(self.emission(due_tail + input_points));
        }

        let immediate = input_points * self.parameters.immediate_ratio();
        let tail_delta = input_points - immediate;
        let new_deadline = new_deadline.expect("active non-zero input has a checked deadline");

        match &mut self.tail {
            Some(tail) => {
                tail.pending_points += tail_delta;
                tail.last_sample_us = timestamp_us;
                tail.deadline_us = tail.deadline_us.max(new_deadline);
                if tail.pending_points.abs() <= DISTANCE_EPSILON_POINTS {
                    self.tail = None;
                }
            }
            None => {
                self.tail = Some(ActiveTail {
                    pending_points: tail_delta,
                    last_sample_us: timestamp_us,
                    deadline_us: new_deadline,
                });
            }
        }

        Ok(self.emission(due_tail + immediate))
    }

    /// Advances the pure state to a caller-supplied timestamp. A future
    /// platform scheduler may call this; the domain engine never wakes itself.
    pub fn sample(&mut self, timestamp_us: u64) -> Result<DynamicsEmission, DynamicsError> {
        self.validate_timestamp(timestamp_us)?;
        let delta_points = self.advance_tail(timestamp_us);
        self.last_timestamp_us = Some(timestamp_us);
        Ok(self.emission(delta_points))
    }

    fn validate_timestamp(&self, timestamp_us: u64) -> Result<(), DynamicsError> {
        if let Some(previous) = self.last_timestamp_us
            && timestamp_us < previous
        {
            return Err(DynamicsError::TimestampOutOfOrder {
                previous,
                current: timestamp_us,
            });
        }
        Ok(())
    }

    fn advance_tail(&mut self, timestamp_us: u64) -> f64 {
        let Some(mut tail) = self.tail.take() else {
            return 0.0;
        };
        if timestamp_us <= tail.last_sample_us {
            self.tail = Some(tail);
            return 0.0;
        }
        if timestamp_us >= tail.deadline_us {
            return tail.pending_points;
        }

        let elapsed_us = timestamp_us - tail.last_sample_us;
        let remaining_us = tail.deadline_us - tail.last_sample_us;
        let fraction = elapsed_us as f64 / remaining_us as f64;
        let emitted = tail.pending_points * fraction;
        tail.pending_points -= emitted;
        tail.last_sample_us = timestamp_us;
        if tail.pending_points.abs() > DISTANCE_EPSILON_POINTS {
            self.tail = Some(tail);
        }
        emitted
    }

    fn emission(&self, delta_points: f64) -> DynamicsEmission {
        let (pending_points, deadline_us) = self.tail.map_or((0.0, None), |tail| {
            (tail.pending_points, Some(tail.deadline_us))
        });
        DynamicsEmission {
            delta_points,
            pending_points,
            phase: self.phase(),
            deadline_us,
        }
    }
}

impl Default for ScrollDynamics {
    fn default() -> Self {
        Self::new(SmoothPreset::Off)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DynamicsError {
    InvalidParameters {
        immediate_per_mille: u16,
        tail_duration_us: u64,
    },
    NonFiniteInput,
    InputTooLarge(f64),
    TimestampOutOfOrder {
        previous: u64,
        current: u64,
    },
    TimestampOverflow {
        timestamp_us: u64,
        duration_us: u64,
    },
}

impl fmt::Display for DynamicsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidParameters {
                immediate_per_mille,
                tail_duration_us,
            } => write!(
                f,
                "invalid dynamics parameters: immediate={immediate_per_mille}/1000, tail={tail_duration_us} us"
            ),
            Self::NonFiniteInput => f.write_str("scroll dynamics input must be finite"),
            Self::InputTooLarge(value) => write!(
                f,
                "scroll dynamics input magnitude {value} exceeds {MAX_ABS_INPUT_POINTS} points"
            ),
            Self::TimestampOutOfOrder { previous, current } => write!(
                f,
                "scroll dynamics timestamp moved backwards from {previous} to {current} microseconds"
            ),
            Self::TimestampOverflow {
                timestamp_us,
                duration_us,
            } => write!(
                f,
                "scroll dynamics deadline overflows: timestamp={timestamp_us}, duration={duration_us}"
            ),
        }
    }
}

impl Error for DynamicsError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_near(left: f64, right: f64) {
        assert!((left - right).abs() <= DISTANCE_EPSILON_POINTS);
    }

    #[test]
    fn presets_have_stable_testable_parameters_and_goals() {
        let expected = [
            (SmoothPreset::Off, "off", 1_000, 0),
            (SmoothPreset::Precise, "precise", 350, 120_000),
            (SmoothPreset::Balanced, "balanced", 550, 90_000),
            (SmoothPreset::Fast, "fast", 750, 60_000),
        ];
        assert_eq!(SmoothPreset::ALL.len(), expected.len());
        for (preset, key, immediate_per_mille, tail_duration_us) in expected {
            let parameters = preset.parameters().validate().unwrap();
            assert_eq!(preset.as_str(), key);
            assert_eq!(parameters.immediate_per_mille, immediate_per_mille);
            assert_eq!(parameters.tail_duration_us, tail_duration_us);
            assert!(!preset.goal().is_empty());
        }
    }

    #[test]
    fn off_is_exact_immediate_pass_through() {
        let mut dynamics = ScrollDynamics::new(SmoothPreset::Off);
        let output = dynamics.handle_input(10, -42.5).unwrap();
        assert_eq!(output.delta_points, -42.5);
        assert_eq!(output.pending_points, 0.0);
        assert_eq!(output.phase, DynamicsPhase::Idle);
        assert_eq!(dynamics.sample(20).unwrap().delta_points, 0.0);
    }

    #[test]
    fn every_active_preset_emits_immediately_and_finishes_by_its_deadline() {
        for preset in [
            SmoothPreset::Precise,
            SmoothPreset::Balanced,
            SmoothPreset::Fast,
        ] {
            let mut dynamics = ScrollDynamics::new(preset);
            let first = dynamics.handle_input(1_000, 100.0).unwrap();
            assert!(first.delta_points > 0.0);
            assert!(first.delta_points < 100.0);
            assert_eq!(first.phase, DynamicsPhase::Active);

            let deadline = first.deadline_us.unwrap();
            let final_output = dynamics.sample(deadline).unwrap();
            assert_near(first.delta_points + final_output.delta_points, 100.0);
            assert_eq!(final_output.phase, DynamicsPhase::Idle);
            assert_eq!(final_output.pending_points, 0.0);
            assert_eq!(dynamics.sample(deadline + 1).unwrap().delta_points, 0.0);
        }
    }

    #[test]
    fn intermediate_sampling_conserves_signed_distance() {
        let mut dynamics = ScrollDynamics::new(SmoothPreset::Balanced);
        let first = dynamics.handle_input(0, -80.0).unwrap();
        let middle = dynamics.sample(45_000).unwrap();
        let final_output = dynamics.sample(90_000).unwrap();

        assert!(first.delta_points < 0.0);
        assert!(middle.delta_points < 0.0);
        assert!(final_output.delta_points < 0.0);
        assert_near(
            first.delta_points + middle.delta_points + final_output.delta_points,
            -80.0,
        );
    }

    #[test]
    fn invalid_input_does_not_advance_timestamp_or_create_tail() {
        let mut dynamics = ScrollDynamics::new(SmoothPreset::Balanced);
        assert_eq!(
            dynamics.handle_input(20, f64::NAN),
            Err(DynamicsError::NonFiniteInput)
        );
        assert!(dynamics.handle_input(10, 10.0).is_ok());
        assert!(matches!(
            dynamics.sample(9),
            Err(DynamicsError::TimestampOutOfOrder { .. })
        ));
    }

    #[test]
    fn overflowing_deadline_is_rejected_without_panicking() {
        let mut dynamics = ScrollDynamics::new(SmoothPreset::Fast);
        assert!(matches!(
            dynamics.handle_input(u64::MAX, 10.0),
            Err(DynamicsError::TimestampOverflow { .. })
        ));
        assert_eq!(dynamics.phase(), DynamicsPhase::Idle);
        assert!(dynamics.handle_input(0, 10.0).is_ok());
    }
}
