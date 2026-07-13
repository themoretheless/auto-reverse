//! One scalar-axis dynamics engine and signed-distance ledger.

use super::DynamicsError;
use super::preset::{PresetParameters, SmoothPreset};
use super::rate::{InputRateEstimator, NormalizedDelta, normalize_input_delta};

pub const DISTANCE_EPSILON_POINTS: f64 = 1e-9;
pub const MAX_ABS_INPUT_POINTS: f64 = 1_000_000.0;

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
pub struct AxisStateSnapshot {
    pub phase: DynamicsPhase,
    pub velocity_points_per_second: Option<f64>,
    pub residual_points: f64,
    pub momentum_points: f64,
    pub observed_rate_millihz: Option<u64>,
    pub rate_interval_count: usize,
    pub last_input_delta: Option<NormalizedDelta>,
    pub deadline_us: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ActiveTail {
    pending_points: f64,
    last_sample_us: u64,
    deadline_us: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct DistanceLedger {
    accepted_points: f64,
    emitted_points: f64,
    residual_points: f64,
}

impl DistanceLedger {
    fn accept(&mut self, input_points: f64) {
        self.accepted_points += input_points;
    }

    fn reconcile(&mut self, emitted_now: f64, momentum_points: f64) {
        self.emitted_points += emitted_now;
        self.residual_points = self.accepted_points - self.emitted_points - momentum_points;
    }

    fn take_idle_correction(&mut self) -> f64 {
        let correction = self.residual_points;
        self.emitted_points += correction;
        let final_error = self.accepted_points - self.emitted_points;
        self.accepted_points = 0.0;
        self.emitted_points = 0.0;
        self.residual_points = 0.0;
        correction + final_error
    }
}

#[derive(Debug, Clone)]
pub struct ScrollDynamics {
    preset: SmoothPreset,
    parameters: PresetParameters,
    last_timestamp_us: Option<u64>,
    last_input_timestamp_us: Option<u64>,
    last_input_delta: Option<NormalizedDelta>,
    rate_estimator: InputRateEstimator,
    velocity_points_per_second: Option<f64>,
    tail: Option<ActiveTail>,
    ledger: DistanceLedger,
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
            last_input_timestamp_us: None,
            last_input_delta: None,
            rate_estimator: InputRateEstimator::default(),
            velocity_points_per_second: None,
            tail: None,
            ledger: DistanceLedger::default(),
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

    pub fn state(&self) -> AxisStateSnapshot {
        let estimate = self.rate_estimator.estimate();
        let (momentum_points, deadline_us) = self.tail.map_or((0.0, None), |tail| {
            (tail.pending_points, Some(tail.deadline_us))
        });
        AxisStateSnapshot {
            phase: self.phase(),
            velocity_points_per_second: self.velocity_points_per_second,
            residual_points: self.ledger.residual_points,
            momentum_points,
            observed_rate_millihz: estimate.map(|estimate| estimate.millihertz),
            rate_interval_count: self.rate_estimator.interval_count(),
            last_input_delta: self.last_input_delta,
            deadline_us,
        }
    }

    /// Accepts one normalized discrete-wheel delta for this axis. The returned
    /// delta is due immediately at `timestamp_us`.
    pub fn handle_input(
        &mut self,
        timestamp_us: u64,
        input_points: f64,
    ) -> Result<DynamicsEmission, DynamicsError> {
        self.validate_timestamp(timestamp_us)?;
        validate_input_points(input_points)?;

        let input_delta = if input_points == 0.0 {
            None
        } else {
            self.last_input_timestamp_us
                .map(|previous| normalize_input_delta(previous, timestamp_us))
                .transpose()?
        };
        let new_deadline = if self.preset != SmoothPreset::Off && input_points != 0.0 {
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

        if input_points == 0.0 {
            return Ok(self.finish_emission(due_tail));
        }

        self.ledger.accept(input_points);
        self.last_input_timestamp_us = Some(timestamp_us);
        self.last_input_delta = input_delta;
        if let Some(delta) = input_delta {
            self.rate_estimator.observe(delta);
        }
        self.velocity_points_per_second = self
            .rate_estimator
            .estimate()
            .map(|estimate| input_points * estimate.millihertz as f64 / 1_000.0);

        if self.preset == SmoothPreset::Off {
            return Ok(self.finish_emission(due_tail + input_points));
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

        Ok(self.finish_emission(due_tail + immediate))
    }

    pub fn sample(&mut self, timestamp_us: u64) -> Result<DynamicsEmission, DynamicsError> {
        self.validate_timestamp(timestamp_us)?;
        let delta_points = self.advance_tail(timestamp_us);
        self.last_timestamp_us = Some(timestamp_us);
        Ok(self.finish_emission(delta_points))
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

    fn finish_emission(&mut self, mut delta_points: f64) -> DynamicsEmission {
        let momentum_points = self.tail.map_or(0.0, |tail| tail.pending_points);
        self.ledger.reconcile(delta_points, momentum_points);
        if self.tail.is_none() {
            delta_points += self.ledger.take_idle_correction();
        }

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

pub(super) fn validate_input_points(input_points: f64) -> Result<(), DynamicsError> {
    if !input_points.is_finite() {
        return Err(DynamicsError::NonFiniteInput);
    }
    if input_points.abs() > MAX_ABS_INPUT_POINTS {
        return Err(DynamicsError::InputTooLarge(input_points));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scroll_dynamics::DeltaClamp;

    fn assert_near(left: f64, right: f64) {
        assert!((left - right).abs() <= DISTANCE_EPSILON_POINTS);
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
    fn distance_ledger_covers_mixed_sign_fractional_sequences_for_every_preset() {
        let sequence = [
            (0, 0.1),
            (3_000, 17.25),
            (11_000, -4.75),
            (27_000, -0.03),
            (49_000, 2.0),
        ];
        for preset in SmoothPreset::ALL {
            let mut dynamics = ScrollDynamics::new(preset);
            let mut input_total = 0.0;
            let mut output_total = 0.0;
            for (timestamp_us, input_points) in sequence {
                input_total += input_points;
                output_total += dynamics
                    .handle_input(timestamp_us, input_points)
                    .unwrap()
                    .delta_points;
            }
            output_total += dynamics.sample(500_000).unwrap().delta_points;
            assert_near(output_total, input_total);
            assert_eq!(dynamics.state().residual_points, 0.0);
            assert_eq!(dynamics.state().momentum_points, 0.0);
        }
    }

    #[test]
    fn velocity_waits_for_bounded_recent_rate_estimate() {
        let mut dynamics = ScrollDynamics::new(SmoothPreset::Balanced);
        for timestamp_us in [0, 10_000, 20_000] {
            dynamics.handle_input(timestamp_us, 2.0).unwrap();
            assert_eq!(dynamics.state().velocity_points_per_second, None);
        }
        dynamics.handle_input(1_000_020_000, 2.0).unwrap();

        let state = dynamics.state();
        assert_eq!(state.observed_rate_millihz, Some(100_000));
        assert_eq!(state.velocity_points_per_second, Some(200.0));
        assert_eq!(
            state.last_input_delta,
            Some(normalize_input_delta(20_000, 1_000_020_000).unwrap())
        );
        assert_eq!(state.last_input_delta.unwrap().clamp(), DeltaClamp::Maximum);
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
    fn overflowing_deadline_is_rejected_without_mutation() {
        let mut dynamics = ScrollDynamics::new(SmoothPreset::Fast);
        assert!(matches!(
            dynamics.handle_input(u64::MAX, 10.0),
            Err(DynamicsError::TimestampOverflow { .. })
        ));
        assert_eq!(dynamics.phase(), DynamicsPhase::Idle);
        assert!(dynamics.handle_input(0, 10.0).is_ok());
    }
}
