//! Pure fail-open scheduler contract for the experimental wheel dynamics.
//!
//! This module owns no clock, timer, thread, CoreGraphics event, or runtime
//! opt-in. A future macOS adapter may hold the returned wake token and call
//! [`ScrollScheduler::poll`] when that token is due.

use std::error::Error;
use std::fmt;

use crate::scroll_dynamics::{
    CancellationPolicy, DynamicsError, DynamicsRoute, ScrollDynamics2D, ScrollVector, SmoothPreset,
};

mod schedule;

pub use schedule::{
    SAMPLE_INTERVAL_US, SCHEDULED_SAMPLE_TTL_US, SampleDiscardReason, SampleDisposition,
    ScheduleAxis, ScheduledEventTag, ScheduledSample, SchedulerError, SessionGeneration, WakeToken,
};
use schedule::{TailSchedule, sample_expiry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerInputRoute {
    DiscreteDynamics,
    ContinuousBypass,
    SelfSyntheticBypass,
    FailOpenTriggered,
    FailOpenLatched,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SchedulerInputEmission {
    pub delta: ScrollVector,
    pub route: SchedulerInputRoute,
    pub wake: Option<WakeToken>,
    pub fault: Option<SchedulerFault>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SchedulerFault {
    Dynamics(DynamicsError),
    Scheduler(SchedulerError),
}

impl fmt::Display for SchedulerFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dynamics(error) => write!(f, "dynamics failed: {error}"),
            Self::Scheduler(error) => write!(f, "scheduler failed: {error}"),
        }
    }
}

impl Error for SchedulerFault {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeDiscardReason {
    SchedulerIdle,
    Superseded,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PollOutcome {
    Waiting {
        wake: WakeToken,
    },
    Discarded {
        reason: WakeDiscardReason,
        current_wake: Option<WakeToken>,
    },
    Sample {
        sample: ScheduledSample,
        next_wake: Option<WakeToken>,
    },
    Faulted {
        fault: SchedulerFault,
    },
}

#[derive(Debug, Clone)]
pub struct ScrollScheduler {
    preset: SmoothPreset,
    cancellation_policy: CancellationPolicy,
    dynamics: ScrollDynamics2D,
    schedule: TailSchedule,
    fault: Option<SchedulerFault>,
}

impl ScrollScheduler {
    pub fn new(preset: SmoothPreset) -> Self {
        Self::with_cancellation_policy(preset, CancellationPolicy::default())
    }

    pub fn with_cancellation_policy(
        preset: SmoothPreset,
        cancellation_policy: CancellationPolicy,
    ) -> Self {
        Self {
            preset,
            cancellation_policy,
            dynamics: ScrollDynamics2D::with_cancellation_policy(preset, cancellation_policy),
            schedule: TailSchedule::default(),
            fault: None,
        }
    }

    /// Handles one normalized input without exposing an error return. Any
    /// internal fault resets pending state, latches fail-open mode, and returns
    /// the original event unchanged.
    pub fn handle_event(
        &mut self,
        timestamp_us: u64,
        original: ScrollVector,
        continuous: bool,
        self_synthetic: bool,
    ) -> SchedulerInputEmission {
        if self_synthetic {
            return self.bypass(original, SchedulerInputRoute::SelfSyntheticBypass);
        }
        if let Some(fault) = self.fault {
            return SchedulerInputEmission {
                delta: original,
                route: SchedulerInputRoute::FailOpenLatched,
                wake: None,
                fault: Some(fault),
            };
        }

        let mut dynamics = self.dynamics.clone();
        let output = match dynamics.handle_event(timestamp_us, original, continuous) {
            Ok(output) => output,
            Err(error) => return self.trip(original, SchedulerFault::Dynamics(error)),
        };

        if output.route == DynamicsRoute::ContinuousBypass {
            return self.bypass(original, SchedulerInputRoute::ContinuousBypass);
        }

        let mut schedule = self.schedule.clone();
        let wake = match schedule.sync(timestamp_us, output.vertical_state, output.horizontal_state)
        {
            Ok(wake) => wake,
            Err(error) => return self.trip(original, SchedulerFault::Scheduler(error)),
        };
        self.dynamics = dynamics;
        self.schedule = schedule;

        SchedulerInputEmission {
            delta: output.delta,
            route: SchedulerInputRoute::DiscreteDynamics,
            wake,
            fault: None,
        }
    }

    /// Advances one exact wake lease. Old callbacks are discarded rather than
    /// sampling a newer session or a replacement lease.
    pub fn poll(&mut self, observed_at_us: u64, wake: WakeToken) -> PollOutcome {
        if let Some(fault) = self.fault {
            return PollOutcome::Faulted { fault };
        }
        let Some(current_wake) = self.schedule.wake() else {
            return PollOutcome::Discarded {
                reason: WakeDiscardReason::SchedulerIdle,
                current_wake: None,
            };
        };
        if wake != current_wake {
            return PollOutcome::Discarded {
                reason: WakeDiscardReason::Superseded,
                current_wake: Some(current_wake),
            };
        }
        if observed_at_us < wake.due_at_us {
            return PollOutcome::Waiting { wake };
        }

        let expires_at_us = match sample_expiry(wake.due_at_us) {
            Ok(expires_at_us) => expires_at_us,
            Err(error) => {
                self.latch_fault(SchedulerFault::Scheduler(error));
                return PollOutcome::Faulted {
                    fault: SchedulerFault::Scheduler(error),
                };
            }
        };
        if observed_at_us > expires_at_us {
            let error = SchedulerError::WakeExpired {
                due_at_us: wake.due_at_us,
                expires_at_us,
                observed_at_us,
            };
            self.latch_fault(SchedulerFault::Scheduler(error));
            return PollOutcome::Faulted {
                fault: SchedulerFault::Scheduler(error),
            };
        }

        let mut dynamics = self.dynamics.clone();
        let output = match dynamics.sample(observed_at_us) {
            Ok(output) => output,
            Err(error) => {
                self.latch_fault(SchedulerFault::Dynamics(error));
                return PollOutcome::Faulted {
                    fault: SchedulerFault::Dynamics(error),
                };
            }
        };
        let sample = match ScheduledSample::new(
            output.delta,
            wake.generation,
            wake.id,
            wake.due_at_us,
            observed_at_us,
        ) {
            Ok(sample) => sample,
            Err(error) => {
                self.latch_fault(SchedulerFault::Scheduler(error));
                return PollOutcome::Faulted {
                    fault: SchedulerFault::Scheduler(error),
                };
            }
        };

        let mut schedule = self.schedule.clone();
        let next_wake = match schedule.sync(
            observed_at_us,
            output.vertical_state,
            output.horizontal_state,
        ) {
            Ok(wake) => wake,
            Err(error) => {
                self.latch_fault(SchedulerFault::Scheduler(error));
                return PollOutcome::Faulted {
                    fault: SchedulerFault::Scheduler(error),
                };
            }
        };
        self.dynamics = dynamics;
        self.schedule = schedule;

        PollOutcome::Sample { sample, next_wake }
    }

    pub fn wake(&self) -> Option<WakeToken> {
        self.schedule.wake()
    }

    pub fn fault(&self) -> Option<SchedulerFault> {
        self.fault
    }

    pub fn current_generation(&self) -> SessionGeneration {
        SessionGeneration::from_states(
            self.dynamics.vertical_state(),
            self.dynamics.horizontal_state(),
        )
    }

    pub fn sample_disposition(
        &self,
        sample: ScheduledSample,
        observed_at_us: u64,
    ) -> SampleDisposition {
        sample.disposition(observed_at_us, self.current_generation())
    }

    pub fn reset_after_fault(&mut self) -> bool {
        if self.fault.is_none() {
            return false;
        }
        self.dynamics =
            ScrollDynamics2D::with_cancellation_policy(self.preset, self.cancellation_policy);
        self.schedule.disarm();
        self.fault = None;
        true
    }

    fn bypass(&self, original: ScrollVector, route: SchedulerInputRoute) -> SchedulerInputEmission {
        SchedulerInputEmission {
            delta: original,
            route,
            wake: self.schedule.wake(),
            fault: self.fault,
        }
    }

    fn trip(&mut self, original: ScrollVector, fault: SchedulerFault) -> SchedulerInputEmission {
        self.latch_fault(fault);
        SchedulerInputEmission {
            delta: original,
            route: SchedulerInputRoute::FailOpenTriggered,
            wake: None,
            fault: Some(fault),
        }
    }

    fn latch_fault(&mut self, fault: SchedulerFault) {
        self.dynamics =
            ScrollDynamics2D::with_cancellation_policy(self.preset, self.cancellation_policy);
        self.schedule.disarm();
        self.fault = Some(fault);
    }
}

impl Default for ScrollScheduler {
    fn default() -> Self {
        Self::new(SmoothPreset::Off)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vector(vertical_points: f64, horizontal_points: f64) -> ScrollVector {
        ScrollVector {
            vertical_points,
            horizontal_points,
        }
    }

    #[test]
    fn self_synthetic_event_is_exact_bypass_without_state_mutation() {
        let mut scheduler = ScrollScheduler::new(SmoothPreset::Balanced);
        let before = scheduler.current_generation();
        let original = vector(17.25, -3.5);

        let output = scheduler.handle_event(10, original, false, true);

        assert_eq!(output.delta, original);
        assert_eq!(output.route, SchedulerInputRoute::SelfSyntheticBypass);
        assert_eq!(scheduler.current_generation(), before);
        assert_eq!(scheduler.wake(), None);
    }

    #[test]
    fn bypass_events_do_not_postpone_or_replace_an_active_wake() {
        let mut scheduler = ScrollScheduler::new(SmoothPreset::Balanced);
        let first = scheduler.handle_event(0, vector(100.0, 0.0), false, false);
        let wake = first.wake.unwrap();

        let continuous = scheduler.handle_event(1_000, vector(-3.0, 2.0), true, false);
        assert_eq!(continuous.delta, vector(-3.0, 2.0));
        assert_eq!(continuous.route, SchedulerInputRoute::ContinuousBypass);
        assert_eq!(continuous.wake, Some(wake));

        let synthetic = scheduler.handle_event(1_500, vector(1.0, 0.0), false, true);
        assert_eq!(synthetic.delta, vector(1.0, 0.0));
        assert_eq!(synthetic.route, SchedulerInputRoute::SelfSyntheticBypass);
        assert_eq!(synthetic.wake, Some(wake));
        assert_eq!(scheduler.wake(), Some(wake));
    }

    #[test]
    fn every_sample_has_generation_ttl_and_synthetic_tag() {
        let mut scheduler = ScrollScheduler::new(SmoothPreset::Balanced);
        let input = scheduler.handle_event(0, vector(100.0, -20.0), false, false);
        let wake = input.wake.unwrap();
        let PollOutcome::Sample { sample, .. } = scheduler.poll(wake.due_at_us, wake) else {
            panic!("due wake must emit one scheduled sample");
        };

        assert_eq!(sample.generation, wake.generation);
        assert_eq!(sample.wake_id, wake.id);
        assert_eq!(
            sample.expires_at_us - sample.scheduled_at_us,
            SCHEDULED_SAMPLE_TTL_US
        );
        assert_eq!(sample.event_tag, ScheduledEventTag::AutoReverseSynthetic);
        assert_eq!(
            scheduler.sample_disposition(sample, sample.expires_at_us),
            SampleDisposition::Deliver
        );
        assert!(matches!(
            scheduler.sample_disposition(sample, sample.expires_at_us + 1),
            SampleDisposition::Discard(SampleDiscardReason::Expired { .. })
        ));
    }

    #[test]
    fn generation_change_discards_old_wake_and_held_sample() {
        let mut scheduler = ScrollScheduler::new(SmoothPreset::Balanced);
        let first = scheduler.handle_event(0, vector(100.0, 0.0), false, false);
        let first_wake = first.wake.unwrap();
        let PollOutcome::Sample {
            sample: held_sample,
            ..
        } = scheduler.poll(first_wake.due_at_us, first_wake)
        else {
            panic!("first wake must emit a sample");
        };
        let old_wake = scheduler.wake().unwrap();

        let reversed = scheduler.handle_event(3_000, vector(-10.0, 0.0), false, false);
        let current_wake = reversed.wake.unwrap();
        assert_ne!(first_wake.generation, current_wake.generation);
        assert_eq!(
            scheduler.poll(current_wake.due_at_us, old_wake),
            PollOutcome::Discarded {
                reason: WakeDiscardReason::Superseded,
                current_wake: Some(current_wake),
            }
        );
        assert!(matches!(
            scheduler.sample_disposition(held_sample, 3_000),
            SampleDisposition::Discard(SampleDiscardReason::StaleGeneration { .. })
        ));
    }

    #[test]
    fn late_callback_keeps_only_the_remaining_wake_ttl() {
        let mut scheduler = ScrollScheduler::new(SmoothPreset::Balanced);
        let input = scheduler.handle_event(0, vector(100.0, 0.0), false, false);
        let wake = input.wake.unwrap();
        let observed_at_us = wake.due_at_us + 5_000;
        let PollOutcome::Sample { sample, .. } = scheduler.poll(observed_at_us, wake) else {
            panic!("wake inside its budget must still emit a sample");
        };

        assert_eq!(sample.created_at_us, observed_at_us);
        assert_eq!(
            sample.expires_at_us,
            wake.due_at_us + SCHEDULED_SAMPLE_TTL_US
        );
        assert_eq!(sample.expires_at_us - sample.created_at_us, 3_000);
    }

    #[test]
    fn scheduler_is_armed_only_while_pending_output_exists() {
        let mut off = ScrollScheduler::new(SmoothPreset::Off);
        let output = off.handle_event(0, vector(10.0, -2.0), false, false);
        assert_eq!(output.delta, vector(10.0, -2.0));
        assert_eq!(output.wake, None);
        assert_eq!(off.wake(), None);

        let mut active = ScrollScheduler::new(SmoothPreset::Balanced);
        let output = active.handle_event(0, vector(100.0, 0.0), false, false);
        let mut wake = output.wake.unwrap();
        loop {
            match active.poll(wake.due_at_us, wake) {
                PollOutcome::Sample {
                    next_wake: Some(next),
                    ..
                } => wake = next,
                PollOutcome::Sample {
                    next_wake: None, ..
                } => break,
                other => panic!("unexpected scheduler result: {other:?}"),
            }
        }
        assert_eq!(active.wake(), None);
        assert_eq!(
            active.poll(wake.due_at_us, wake),
            PollOutcome::Discarded {
                reason: WakeDiscardReason::SchedulerIdle,
                current_wake: None,
            }
        );
    }

    #[test]
    fn dynamics_error_returns_original_and_latches_fail_open() {
        let mut scheduler = ScrollScheduler::new(SmoothPreset::Balanced);
        scheduler.handle_event(1_000, vector(10.0, 0.0), false, false);
        let original = vector(-7.0, 2.0);

        let failed = scheduler.handle_event(999, original, false, false);
        assert_eq!(failed.delta, original);
        assert_eq!(failed.route, SchedulerInputRoute::FailOpenTriggered);
        assert!(matches!(
            failed.fault,
            Some(SchedulerFault::Dynamics(
                DynamicsError::TimestampOutOfOrder { .. }
            ))
        ));
        assert_eq!(scheduler.wake(), None);

        let later = scheduler.handle_event(2_000, original, false, false);
        assert_eq!(later.delta, original);
        assert_eq!(later.route, SchedulerInputRoute::FailOpenLatched);
        assert_eq!(later.fault, failed.fault);
    }

    #[test]
    fn expired_wake_latches_fail_open_and_drops_pending_output() {
        let mut scheduler = ScrollScheduler::new(SmoothPreset::Balanced);
        let started_at_us = u64::MAX - 90_000;
        let output = scheduler.handle_event(started_at_us, vector(100.0, 0.0), false, false);
        let wake = output.wake.unwrap();
        let observed_at_us = wake.due_at_us + SCHEDULED_SAMPLE_TTL_US + 1;

        assert!(matches!(
            scheduler.poll(observed_at_us, wake),
            PollOutcome::Faulted {
                fault: SchedulerFault::Scheduler(SchedulerError::WakeExpired { .. })
            }
        ));
        assert_eq!(scheduler.wake(), None);

        let original = vector(4.0, -1.0);
        let later = scheduler.handle_event(0, original, false, false);
        assert_eq!(later.delta, original);
        assert_eq!(later.route, SchedulerInputRoute::FailOpenLatched);
    }

    #[test]
    fn explicit_reset_is_required_to_leave_fail_open_mode() {
        let mut scheduler = ScrollScheduler::new(SmoothPreset::Balanced);
        let old_wake = scheduler
            .handle_event(10, vector(10.0, 0.0), false, false)
            .wake
            .unwrap();
        scheduler.handle_event(9, vector(1.0, 0.0), false, false);
        assert!(scheduler.fault().is_some());

        assert!(scheduler.reset_after_fault());
        let recovered = scheduler.handle_event(0, vector(10.0, 0.0), false, false);
        assert_eq!(recovered.route, SchedulerInputRoute::DiscreteDynamics);
        assert!(recovered.wake.unwrap().id > old_wake.id);
    }

    #[test]
    fn reset_after_fault_is_a_noop_while_scheduler_is_healthy() {
        let mut scheduler = ScrollScheduler::new(SmoothPreset::Balanced);
        let wake = scheduler
            .handle_event(0, vector(10.0, 0.0), false, false)
            .wake
            .unwrap();

        assert!(!scheduler.reset_after_fault());
        assert_eq!(scheduler.wake(), Some(wake));
    }
}
