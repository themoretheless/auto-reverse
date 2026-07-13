//! One scalar-axis dynamics engine and signed-distance ledger.

use super::DynamicsError;
use super::preset::{PresetParameters, SmoothPreset};
use super::rate::{InputRateEstimator, NormalizedDelta, normalize_input_delta};

pub const DISTANCE_EPSILON_POINTS: f64 = 1e-9;
pub const MAX_ABS_INPUT_POINTS: f64 = 1_000_000.0;
pub const SESSION_GAP_US: u64 = 150_000;
pub const STOP_THRESHOLD_POINTS: f64 = 0.25;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DynamicsPhase {
    Idle,
    Active,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionBoundary {
    InitialInput,
    DirectionChange,
    LongGap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancellationReason {
    OppositeInput,
    LongGap,
    NewPhysicalAction,
    PointerClick,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CancellationRecord {
    pub timestamp_us: u64,
    pub reason: CancellationReason,
    pub canceled_points: f64,
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
    pub session_generation: u64,
    pub last_session_boundary: Option<SessionBoundary>,
    pub input_direction: i8,
    pub last_cancellation: Option<CancellationRecord>,
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
    canceled_points: f64,
    residual_points: f64,
}

impl DistanceLedger {
    fn accept(&mut self, input_points: f64) {
        self.accepted_points += input_points;
    }

    fn reconcile(&mut self, emitted_now: f64, momentum_points: f64) {
        self.emitted_points += emitted_now;
        self.residual_points =
            self.accepted_points - self.emitted_points - self.canceled_points - momentum_points;
    }

    fn take_idle_correction(&mut self) -> f64 {
        let correction = self.residual_points;
        self.emitted_points += correction;
        let final_error = self.accepted_points - self.emitted_points - self.canceled_points;
        self.reset();
        correction + final_error
    }

    fn cancel_all(&mut self, momentum_points: f64) -> f64 {
        let canceled_before = self.canceled_points;
        self.canceled_points += momentum_points + self.residual_points;
        self.residual_points = 0.0;
        self.canceled_points += self.accepted_points - self.emitted_points - self.canceled_points;
        let newly_canceled = self.canceled_points - canceled_before;
        self.reset();
        newly_canceled
    }

    fn reset(&mut self) {
        self.accepted_points = 0.0;
        self.emitted_points = 0.0;
        self.canceled_points = 0.0;
        self.residual_points = 0.0;
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
    input_direction: i8,
    session_generation: u64,
    last_session_boundary: Option<SessionBoundary>,
    last_cancellation: Option<CancellationRecord>,
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
            input_direction: 0,
            session_generation: 0,
            last_session_boundary: None,
            last_cancellation: None,
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
            session_generation: self.session_generation,
            last_session_boundary: self.last_session_boundary,
            input_direction: self.input_direction,
            last_cancellation: self.last_cancellation,
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

        let direction = input_direction(input_points);
        let boundary = self.session_boundary(timestamp_us, direction);
        let input_delta = if input_points == 0.0 || boundary.is_some() {
            None
        } else {
            self.last_input_timestamp_us
                .map(|previous| normalize_input_delta(previous, timestamp_us))
                .transpose()?
        };

        if let Some(boundary) = boundary {
            match boundary {
                SessionBoundary::InitialInput => {}
                SessionBoundary::DirectionChange => {
                    self.cancel_pending(timestamp_us, CancellationReason::OppositeInput);
                }
                SessionBoundary::LongGap => {
                    self.cancel_pending(timestamp_us, CancellationReason::LongGap);
                }
            }
            self.begin_session(boundary);
        }

        let due_tail = self.advance_tail(timestamp_us);
        self.last_timestamp_us = Some(timestamp_us);

        if input_points == 0.0 {
            return Ok(self.finish_emission(due_tail));
        }

        self.ledger.accept(input_points);
        self.last_input_timestamp_us = Some(timestamp_us);
        self.last_input_delta = input_delta;
        self.input_direction = direction;
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
            }
            None => {
                self.tail = Some(ActiveTail {
                    pending_points: tail_delta,
                    last_sample_us: timestamp_us,
                    deadline_us: new_deadline,
                });
            }
        }
        if self
            .tail
            .is_some_and(|tail| tail.pending_points.abs() <= STOP_THRESHOLD_POINTS)
        {
            self.tail = None;
        }

        Ok(self.finish_emission(due_tail + immediate))
    }

    pub fn sample(&mut self, timestamp_us: u64) -> Result<DynamicsEmission, DynamicsError> {
        self.validate_timestamp(timestamp_us)?;
        let delta_points = self.advance_tail(timestamp_us);
        self.last_timestamp_us = Some(timestamp_us);
        Ok(self.finish_emission(delta_points))
    }

    pub(super) fn cancel_external(
        &mut self,
        timestamp_us: u64,
        reason: CancellationReason,
    ) -> Result<f64, DynamicsError> {
        self.validate_timestamp(timestamp_us)?;
        let canceled_points = self.cancel_pending(timestamp_us, reason);
        self.reset_input_tracking();
        self.last_timestamp_us = Some(timestamp_us);
        Ok(canceled_points)
    }

    fn session_boundary(&self, timestamp_us: u64, direction: i8) -> Option<SessionBoundary> {
        if direction == 0 {
            return None;
        }
        let Some(previous_input_us) = self.last_input_timestamp_us else {
            return Some(SessionBoundary::InitialInput);
        };
        if timestamp_us - previous_input_us > SESSION_GAP_US {
            Some(SessionBoundary::LongGap)
        } else if self.input_direction != 0 && direction != self.input_direction {
            Some(SessionBoundary::DirectionChange)
        } else {
            None
        }
    }

    fn begin_session(&mut self, boundary: SessionBoundary) {
        self.reset_input_tracking();
        self.session_generation = self.session_generation.saturating_add(1);
        self.last_session_boundary = Some(boundary);
    }

    fn reset_input_tracking(&mut self) {
        self.last_input_timestamp_us = None;
        self.last_input_delta = None;
        self.rate_estimator.clear();
        self.velocity_points_per_second = None;
        self.input_direction = 0;
    }

    fn cancel_pending(&mut self, timestamp_us: u64, reason: CancellationReason) -> f64 {
        let momentum_points = self.tail.take().map_or(0.0, |tail| tail.pending_points);
        let canceled_points = self.ledger.cancel_all(momentum_points);
        self.last_cancellation = Some(CancellationRecord {
            timestamp_us,
            reason,
            canceled_points,
        });
        canceled_points
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
        if tail.pending_points.abs() <= STOP_THRESHOLD_POINTS {
            return emitted + tail.pending_points;
        }
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

fn input_direction(input_points: f64) -> i8 {
    if input_points > 0.0 {
        1
    } else if input_points < 0.0 {
        -1
    } else {
        0
    }
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
    fn distance_ledger_covers_fractional_sequences_in_both_signs_for_every_preset() {
        let sequence = [
            (0, 0.1),
            (3_000, 17.25),
            (11_000, 4.75),
            (27_000, 0.03),
            (49_000, 2.0),
        ];
        for preset in SmoothPreset::ALL {
            for sign in [-1.0, 1.0] {
                let mut dynamics = ScrollDynamics::new(preset);
                let mut input_total = 0.0;
                let mut output_total = 0.0;
                for (timestamp_us, input_points) in sequence {
                    let input_points = input_points * sign;
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
    }

    #[test]
    fn velocity_waits_for_bounded_recent_rate_estimate() {
        let mut dynamics = ScrollDynamics::new(SmoothPreset::Balanced);
        for timestamp_us in [0, 10_000, 20_000] {
            dynamics.handle_input(timestamp_us, 2.0).unwrap();
            assert_eq!(dynamics.state().velocity_points_per_second, None);
        }
        dynamics.handle_input(120_000, 2.0).unwrap();

        let state = dynamics.state();
        assert_eq!(state.observed_rate_millihz, Some(100_000));
        assert_eq!(state.velocity_points_per_second, Some(200.0));
        assert_eq!(
            state.last_input_delta,
            Some(normalize_input_delta(20_000, 120_000).unwrap())
        );
        assert_eq!(state.last_input_delta.unwrap().clamp(), DeltaClamp::Maximum);
    }

    #[test]
    fn opposite_input_cancels_momentum_and_resets_rate_window() {
        let mut dynamics = ScrollDynamics::new(SmoothPreset::Balanced);
        let mut input_total = 0.0;
        let mut output_total = 0.0;
        for timestamp_us in [0, 10_000, 20_000, 30_000] {
            input_total += 10.0;
            output_total += dynamics
                .handle_input(timestamp_us, 10.0)
                .unwrap()
                .delta_points;
        }
        assert_eq!(dynamics.state().observed_rate_millihz, Some(100_000));

        input_total -= 10.0;
        output_total += dynamics.handle_input(40_000, -10.0).unwrap().delta_points;
        let state = dynamics.state();
        let cancellation = state.last_cancellation.unwrap();
        assert_eq!(cancellation.reason, CancellationReason::OppositeInput);
        assert!(cancellation.canceled_points > 0.0);
        assert_eq!(
            state.last_session_boundary,
            Some(SessionBoundary::DirectionChange)
        );
        assert_eq!(state.session_generation, 2);
        assert_eq!(state.rate_interval_count, 0);
        assert_eq!(state.velocity_points_per_second, None);
        assert!(state.momentum_points < 0.0);

        output_total += dynamics.sample(200_000).unwrap().delta_points;
        assert_near(output_total + cancellation.canceled_points, input_total);
    }

    #[test]
    fn long_gap_starts_a_new_session_without_releasing_stale_tail() {
        let mut dynamics = ScrollDynamics::new(SmoothPreset::Balanced);
        for timestamp_us in [0, 10_000, 20_000, 30_000] {
            dynamics.handle_input(timestamp_us, 10.0).unwrap();
        }
        let output = dynamics.handle_input(200_000, 10.0).unwrap();
        let state = dynamics.state();

        assert_near(output.delta_points, 5.5);
        assert_eq!(state.last_session_boundary, Some(SessionBoundary::LongGap));
        assert_eq!(state.session_generation, 2);
        assert_eq!(state.rate_interval_count, 0);
        assert_eq!(state.last_input_delta, None);
        assert_eq!(
            state.last_cancellation.unwrap().reason,
            CancellationReason::LongGap
        );
    }

    #[test]
    fn stop_threshold_flushes_subpixel_remainder_without_later_creep() {
        let mut dynamics = ScrollDynamics::new(SmoothPreset::Balanced);
        let first = dynamics.handle_input(0, 1.0).unwrap();
        let stopped = dynamics.sample(45_000).unwrap();

        assert_near(first.delta_points + stopped.delta_points, 1.0);
        assert_eq!(stopped.phase, DynamicsPhase::Idle);
        assert_eq!(stopped.pending_points, 0.0);
        assert_eq!(dynamics.sample(60_000).unwrap().delta_points, 0.0);
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
