//! Pure discrete-wheel dynamics facade.
//!
//! The facade owns two independent scalar-axis engines and rejects dynamics
//! for every continuous event. It owns no clock, thread, scheduler,
//! CoreGraphics object, or configuration store.

use std::error::Error;
use std::fmt;

mod axis;
mod preset;
mod rate;

pub use axis::{
    AxisStateSnapshot, DISTANCE_EPSILON_POINTS, DynamicsEmission, DynamicsPhase,
    MAX_ABS_INPUT_POINTS, ScrollDynamics,
};
pub use preset::{PresetParameters, SmoothPreset};
pub use rate::{
    DeltaClamp, InputRateEstimator, MAX_INPUT_DT_US, MIN_INPUT_DT_US, MIN_RATE_INTERVALS,
    NormalizedDelta, RATE_WINDOW_CAPACITY, RateEstimate, normalize_input_delta,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollVector {
    pub vertical_points: f64,
    pub horizontal_points: f64,
}

impl ScrollVector {
    pub const ZERO: Self = Self {
        vertical_points: 0.0,
        horizontal_points: 0.0,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DynamicsRoute {
    DiscreteDynamics,
    ContinuousBypass,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TwoAxisEmission {
    pub delta: ScrollVector,
    pub route: DynamicsRoute,
    pub vertical_state: AxisStateSnapshot,
    pub horizontal_state: AxisStateSnapshot,
}

#[derive(Debug, Clone)]
pub struct ScrollDynamics2D {
    vertical: ScrollDynamics,
    horizontal: ScrollDynamics,
}

impl ScrollDynamics2D {
    pub fn new(preset: SmoothPreset) -> Self {
        Self {
            vertical: ScrollDynamics::new(preset),
            horizontal: ScrollDynamics::new(preset),
        }
    }

    /// Routes one already-normalized scroll event. Continuous input is exact
    /// pass-through and cannot mutate either discrete-wheel axis state.
    pub fn handle_event(
        &mut self,
        timestamp_us: u64,
        input: ScrollVector,
        continuous: bool,
    ) -> Result<TwoAxisEmission, DynamicsError> {
        axis::validate_input_points(input.vertical_points)?;
        axis::validate_input_points(input.horizontal_points)?;

        if continuous {
            return Ok(TwoAxisEmission {
                delta: input,
                route: DynamicsRoute::ContinuousBypass,
                vertical_state: self.vertical.state(),
                horizontal_state: self.horizontal.state(),
            });
        }

        // Keep a two-axis event transactional if a future axis-specific rule
        // introduces an error after the other axis has advanced.
        let mut vertical = self.vertical.clone();
        let mut horizontal = self.horizontal.clone();
        let vertical_output = vertical.handle_input(timestamp_us, input.vertical_points)?;
        let horizontal_output = horizontal.handle_input(timestamp_us, input.horizontal_points)?;
        self.vertical = vertical;
        self.horizontal = horizontal;

        Ok(TwoAxisEmission {
            delta: ScrollVector {
                vertical_points: vertical_output.delta_points,
                horizontal_points: horizontal_output.delta_points,
            },
            route: DynamicsRoute::DiscreteDynamics,
            vertical_state: self.vertical.state(),
            horizontal_state: self.horizontal.state(),
        })
    }

    /// Advances pending output at a caller-provided timestamp. A future
    /// scheduler may call this method; the pure facade never wakes itself.
    pub fn sample(&mut self, timestamp_us: u64) -> Result<TwoAxisEmission, DynamicsError> {
        let mut vertical = self.vertical.clone();
        let mut horizontal = self.horizontal.clone();
        let vertical_output = vertical.sample(timestamp_us)?;
        let horizontal_output = horizontal.sample(timestamp_us)?;
        self.vertical = vertical;
        self.horizontal = horizontal;

        Ok(TwoAxisEmission {
            delta: ScrollVector {
                vertical_points: vertical_output.delta_points,
                horizontal_points: horizontal_output.delta_points,
            },
            route: DynamicsRoute::DiscreteDynamics,
            vertical_state: self.vertical.state(),
            horizontal_state: self.horizontal.state(),
        })
    }

    pub fn vertical_state(&self) -> AxisStateSnapshot {
        self.vertical.state()
    }

    pub fn horizontal_state(&self) -> AxisStateSnapshot {
        self.horizontal.state()
    }
}

impl Default for ScrollDynamics2D {
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
    fn continuous_input_is_exact_bypass_and_does_not_mutate_discrete_state() {
        let mut dynamics = ScrollDynamics2D::new(SmoothPreset::Balanced);
        dynamics
            .handle_event(
                0,
                ScrollVector {
                    vertical_points: 100.0,
                    horizontal_points: 0.0,
                },
                false,
            )
            .unwrap();
        let vertical_before = dynamics.vertical_state();
        let horizontal_before = dynamics.horizontal_state();

        let output = dynamics
            .handle_event(
                50_000,
                ScrollVector {
                    vertical_points: -3.25,
                    horizontal_points: 4.5,
                },
                true,
            )
            .unwrap();

        assert_eq!(output.route, DynamicsRoute::ContinuousBypass);
        assert_eq!(output.delta.vertical_points, -3.25);
        assert_eq!(output.delta.horizontal_points, 4.5);
        assert_eq!(dynamics.vertical_state(), vertical_before);
        assert_eq!(dynamics.horizontal_state(), horizontal_before);
    }

    #[test]
    fn rejected_two_axis_event_leaves_both_states_unchanged() {
        let mut dynamics = ScrollDynamics2D::new(SmoothPreset::Fast);
        dynamics
            .handle_event(
                0,
                ScrollVector {
                    vertical_points: 12.0,
                    horizontal_points: 3.0,
                },
                false,
            )
            .unwrap();
        let vertical_before = dynamics.vertical_state();
        let horizontal_before = dynamics.horizontal_state();

        assert_eq!(
            dynamics.handle_event(
                5_000,
                ScrollVector {
                    vertical_points: 1.0,
                    horizontal_points: f64::NAN,
                },
                false,
            ),
            Err(DynamicsError::NonFiniteInput)
        );
        assert_eq!(dynamics.vertical_state(), vertical_before);
        assert_eq!(dynamics.horizontal_state(), horizontal_before);
    }

    #[test]
    fn vertical_and_horizontal_rate_velocity_residual_and_momentum_are_independent() {
        let mut dynamics = ScrollDynamics2D::new(SmoothPreset::Balanced);
        for timestamp_us in [0, 10_000, 20_000, 30_000] {
            dynamics
                .handle_event(
                    timestamp_us,
                    ScrollVector {
                        vertical_points: 10.0,
                        horizontal_points: 0.0,
                    },
                    false,
                )
                .unwrap();
        }
        for timestamp_us in [40_000, 60_000, 80_000, 100_000] {
            dynamics
                .handle_event(
                    timestamp_us,
                    ScrollVector {
                        vertical_points: 0.0,
                        horizontal_points: -2.0,
                    },
                    false,
                )
                .unwrap();
        }

        let vertical = dynamics.vertical_state();
        let horizontal = dynamics.horizontal_state();
        assert_eq!(vertical.observed_rate_millihz, Some(100_000));
        assert_eq!(horizontal.observed_rate_millihz, Some(50_000));
        assert_eq!(vertical.velocity_points_per_second, Some(1_000.0));
        assert_eq!(horizontal.velocity_points_per_second, Some(-100.0));
        assert_ne!(vertical.momentum_points, horizontal.momentum_points);
        assert!(vertical.residual_points.abs() <= DISTANCE_EPSILON_POINTS);
        assert!(horizontal.residual_points.abs() <= DISTANCE_EPSILON_POINTS);
    }

    #[test]
    fn two_axis_distance_is_conserved_across_repeated_inputs() {
        let mut dynamics = ScrollDynamics2D::new(SmoothPreset::Precise);
        let inputs = [
            (0, 12.5, -3.0),
            (7_000, 4.25, 1.5),
            (19_000, -2.0, 8.75),
            (44_000, 0.125, -0.375),
        ];
        let mut input_total = ScrollVector::ZERO;
        let mut output_total = ScrollVector::ZERO;
        for (timestamp_us, vertical_points, horizontal_points) in inputs {
            input_total.vertical_points += vertical_points;
            input_total.horizontal_points += horizontal_points;
            let output = dynamics
                .handle_event(
                    timestamp_us,
                    ScrollVector {
                        vertical_points,
                        horizontal_points,
                    },
                    false,
                )
                .unwrap();
            output_total.vertical_points += output.delta.vertical_points;
            output_total.horizontal_points += output.delta.horizontal_points;
        }
        let final_output = dynamics.sample(500_000).unwrap();
        output_total.vertical_points += final_output.delta.vertical_points;
        output_total.horizontal_points += final_output.delta.horizontal_points;

        assert_near(output_total.vertical_points, input_total.vertical_points);
        assert_near(
            output_total.horizontal_points,
            input_total.horizontal_points,
        );
        assert_eq!(dynamics.vertical_state().phase, DynamicsPhase::Idle);
        assert_eq!(dynamics.horizontal_state().phase, DynamicsPhase::Idle);
    }
}
