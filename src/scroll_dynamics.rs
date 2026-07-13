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
    AxisStateSnapshot, CancellationReason, CancellationRecord, DISTANCE_EPSILON_POINTS,
    DynamicsEmission, DynamicsPhase, MAX_ABS_INPUT_POINTS, SESSION_GAP_US, STOP_THRESHOLD_POINTS,
    ScrollDynamics, SessionBoundary,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancellationTrigger {
    NewPhysicalAction,
    PointerClick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CancellationPolicy {
    pub cancel_on_new_physical_action: bool,
    pub cancel_on_pointer_click: bool,
}

impl CancellationPolicy {
    pub const NONE: Self = Self {
        cancel_on_new_physical_action: false,
        cancel_on_pointer_click: false,
    };

    pub const fn allows(self, trigger: CancellationTrigger) -> bool {
        match trigger {
            CancellationTrigger::NewPhysicalAction => self.cancel_on_new_physical_action,
            CancellationTrigger::PointerClick => self.cancel_on_pointer_click,
        }
    }
}

impl Default for CancellationPolicy {
    fn default() -> Self {
        Self {
            cancel_on_new_physical_action: true,
            cancel_on_pointer_click: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CancellationEmission {
    pub applied: bool,
    pub canceled_distance: ScrollVector,
    pub vertical_state: AxisStateSnapshot,
    pub horizontal_state: AxisStateSnapshot,
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
    cancellation_policy: CancellationPolicy,
}

impl ScrollDynamics2D {
    pub fn new(preset: SmoothPreset) -> Self {
        Self::with_cancellation_policy(preset, CancellationPolicy::default())
    }

    pub fn with_cancellation_policy(
        preset: SmoothPreset,
        cancellation_policy: CancellationPolicy,
    ) -> Self {
        Self {
            vertical: ScrollDynamics::new(preset),
            horizontal: ScrollDynamics::new(preset),
            cancellation_policy,
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

    /// Ends pending momentum for an external physical action when enabled by
    /// the explicit policy. Platform adapters decide which events map to the
    /// two public triggers.
    pub fn cancel(
        &mut self,
        timestamp_us: u64,
        trigger: CancellationTrigger,
    ) -> Result<CancellationEmission, DynamicsError> {
        if !self.cancellation_policy.allows(trigger) {
            return Ok(CancellationEmission {
                applied: false,
                canceled_distance: ScrollVector::ZERO,
                vertical_state: self.vertical.state(),
                horizontal_state: self.horizontal.state(),
            });
        }

        let reason = match trigger {
            CancellationTrigger::NewPhysicalAction => CancellationReason::NewPhysicalAction,
            CancellationTrigger::PointerClick => CancellationReason::PointerClick,
        };
        let mut vertical = self.vertical.clone();
        let mut horizontal = self.horizontal.clone();
        let vertical_canceled = vertical.cancel_external(timestamp_us, reason)?;
        let horizontal_canceled = horizontal.cancel_external(timestamp_us, reason)?;
        self.vertical = vertical;
        self.horizontal = horizontal;

        Ok(CancellationEmission {
            applied: true,
            canceled_distance: ScrollVector {
                vertical_points: vertical_canceled,
                horizontal_points: horizontal_canceled,
            },
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
    SessionGenerationOverflow,
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
            Self::SessionGenerationOverflow => {
                f.write_str("scroll dynamics session generation overflowed")
            }
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
        let first = dynamics
            .handle_event(
                0,
                ScrollVector {
                    vertical_points: 100.0,
                    horizontal_points: 0.0,
                },
                false,
            )
            .unwrap();
        assert_near(first.delta.vertical_points, 55.0);
        assert_eq!(first.delta.horizontal_points, 0.0);
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
            (7_000, 4.25, -1.5),
            (19_000, 2.0, -8.75),
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

    #[test]
    fn click_and_physical_action_cancellation_follow_explicit_policy() {
        let policy = CancellationPolicy {
            cancel_on_new_physical_action: true,
            cancel_on_pointer_click: false,
        };
        let mut dynamics =
            ScrollDynamics2D::with_cancellation_policy(SmoothPreset::Balanced, policy);
        let first = dynamics
            .handle_event(
                0,
                ScrollVector {
                    vertical_points: 100.0,
                    horizontal_points: -20.0,
                },
                false,
            )
            .unwrap();
        assert_near(first.delta.vertical_points, 55.0);
        assert_near(first.delta.horizontal_points, -11.0);
        let vertical_before = dynamics.vertical_state();
        let horizontal_before = dynamics.horizontal_state();

        let ignored = dynamics
            .cancel(1_000, CancellationTrigger::PointerClick)
            .unwrap();
        assert!(!ignored.applied);
        assert_eq!(dynamics.vertical_state(), vertical_before);
        assert_eq!(dynamics.horizontal_state(), horizontal_before);

        let canceled = dynamics
            .cancel(1_000, CancellationTrigger::NewPhysicalAction)
            .unwrap();
        assert!(canceled.applied);
        assert_near(canceled.canceled_distance.vertical_points, 45.0);
        assert_near(canceled.canceled_distance.horizontal_points, -9.0);
        assert_eq!(canceled.vertical_state.phase, DynamicsPhase::Idle);
        assert_eq!(canceled.horizontal_state.phase, DynamicsPhase::Idle);
        assert_eq!(
            canceled.vertical_state.last_cancellation.unwrap().reason,
            CancellationReason::NewPhysicalAction
        );

        let restarted = dynamics
            .handle_event(
                2_000,
                ScrollVector {
                    vertical_points: -5.0,
                    horizontal_points: 0.0,
                },
                false,
            )
            .unwrap();
        assert_eq!(restarted.vertical_state.session_generation, 2);
        assert_eq!(
            restarted.vertical_state.last_session_boundary,
            Some(SessionBoundary::InitialInput)
        );
    }

    #[test]
    fn default_policy_allows_pointer_click_cancellation() {
        let mut dynamics = ScrollDynamics2D::new(SmoothPreset::Fast);
        dynamics
            .handle_event(
                0,
                ScrollVector {
                    vertical_points: 40.0,
                    horizontal_points: 0.0,
                },
                false,
            )
            .unwrap();

        let canceled = dynamics
            .cancel(1, CancellationTrigger::PointerClick)
            .unwrap();
        assert!(canceled.applied);
        assert_eq!(
            canceled.vertical_state.last_cancellation.unwrap().reason,
            CancellationReason::PointerClick
        );
        assert_eq!(canceled.vertical_state.phase, DynamicsPhase::Idle);
    }
}
