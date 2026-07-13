//! Pure wake-lease and scheduled-sample safety vocabulary.

use std::error::Error;
use std::fmt;

use crate::scroll_dynamics::{AxisStateSnapshot, DynamicsPhase, ScrollVector};

pub const SAMPLE_INTERVAL_US: u64 = 2_000;
pub const SCHEDULED_SAMPLE_TTL_US: u64 = 8_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SessionGeneration {
    pub vertical: u64,
    pub horizontal: u64,
}

impl SessionGeneration {
    pub(crate) fn from_states(vertical: AxisStateSnapshot, horizontal: AxisStateSnapshot) -> Self {
        Self {
            vertical: vertical.session_generation,
            horizontal: horizontal.session_generation,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WakeToken {
    pub id: u64,
    pub due_at_us: u64,
    pub generation: SessionGeneration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduledEventTag {
    AutoReverseSynthetic,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScheduledSample {
    pub delta: ScrollVector,
    pub generation: SessionGeneration,
    pub wake_id: u64,
    pub scheduled_at_us: u64,
    pub created_at_us: u64,
    pub expires_at_us: u64,
    pub event_tag: ScheduledEventTag,
}

impl ScheduledSample {
    pub(crate) fn new(
        delta: ScrollVector,
        generation: SessionGeneration,
        wake_id: u64,
        scheduled_at_us: u64,
        created_at_us: u64,
    ) -> Result<Self, SchedulerError> {
        let expires_at_us = sample_expiry(scheduled_at_us)?;
        Ok(Self {
            delta,
            generation,
            wake_id,
            scheduled_at_us,
            created_at_us,
            expires_at_us,
            event_tag: ScheduledEventTag::AutoReverseSynthetic,
        })
    }

    pub fn disposition(
        self,
        now_us: u64,
        current_generation: SessionGeneration,
    ) -> SampleDisposition {
        if self.generation != current_generation {
            return SampleDisposition::Discard(SampleDiscardReason::StaleGeneration {
                sample: self.generation,
                current: current_generation,
            });
        }
        if now_us > self.expires_at_us {
            return SampleDisposition::Discard(SampleDiscardReason::Expired {
                expires_at_us: self.expires_at_us,
                observed_at_us: now_us,
            });
        }
        SampleDisposition::Deliver
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleDisposition {
    Deliver,
    Discard(SampleDiscardReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleDiscardReason {
    StaleGeneration {
        sample: SessionGeneration,
        current: SessionGeneration,
    },
    Expired {
        expires_at_us: u64,
        observed_at_us: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleAxis {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerError {
    MissingDeadline(ScheduleAxis),
    DeadlineNotFuture {
        axis: ScheduleAxis,
        deadline_us: u64,
        observed_at_us: u64,
    },
    WakeIdOverflow,
    SampleExpiryOverflow {
        scheduled_at_us: u64,
        ttl_us: u64,
    },
    WakeExpired {
        due_at_us: u64,
        expires_at_us: u64,
        observed_at_us: u64,
    },
}

impl fmt::Display for SchedulerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDeadline(axis) => {
                write!(f, "active {} axis has no scheduler deadline", axis.label())
            }
            Self::DeadlineNotFuture {
                axis,
                deadline_us,
                observed_at_us,
            } => write!(
                f,
                "{} axis deadline {deadline_us} is not after scheduler time {observed_at_us}",
                axis.label()
            ),
            Self::WakeIdOverflow => f.write_str("scheduler wake id overflowed"),
            Self::SampleExpiryOverflow {
                scheduled_at_us,
                ttl_us,
            } => write!(
                f,
                "scheduled sample expiry overflows: scheduled={scheduled_at_us}, ttl={ttl_us}"
            ),
            Self::WakeExpired {
                due_at_us,
                expires_at_us,
                observed_at_us,
            } => write!(
                f,
                "scheduler wake due at {due_at_us} expired at {expires_at_us} before callback {observed_at_us}"
            ),
        }
    }
}

impl Error for SchedulerError {}

pub(crate) fn sample_expiry(scheduled_at_us: u64) -> Result<u64, SchedulerError> {
    scheduled_at_us.checked_add(SCHEDULED_SAMPLE_TTL_US).ok_or(
        SchedulerError::SampleExpiryOverflow {
            scheduled_at_us,
            ttl_us: SCHEDULED_SAMPLE_TTL_US,
        },
    )
}

impl ScheduleAxis {
    const fn label(self) -> &'static str {
        match self {
            Self::Vertical => "vertical",
            Self::Horizontal => "horizontal",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TailSchedule {
    wake: Option<WakeToken>,
    last_wake_id: u64,
}

impl TailSchedule {
    pub(crate) fn wake(&self) -> Option<WakeToken> {
        self.wake
    }

    pub(crate) fn disarm(&mut self) {
        self.wake = None;
    }

    pub(crate) fn sync(
        &mut self,
        now_us: u64,
        vertical: AxisStateSnapshot,
        horizontal: AxisStateSnapshot,
    ) -> Result<Option<WakeToken>, SchedulerError> {
        let vertical_deadline = active_deadline(now_us, ScheduleAxis::Vertical, vertical)?;
        let horizontal_deadline = active_deadline(now_us, ScheduleAxis::Horizontal, horizontal)?;
        let deadline_us = match (vertical_deadline, horizontal_deadline) {
            (Some(vertical), Some(horizontal)) => vertical.min(horizontal),
            (Some(deadline), None) | (None, Some(deadline)) => deadline,
            (None, None) => {
                self.wake = None;
                return Ok(None);
            }
        };

        let id = self
            .last_wake_id
            .checked_add(1)
            .ok_or(SchedulerError::WakeIdOverflow)?;
        let due_at_us = now_us
            .checked_add(SAMPLE_INTERVAL_US)
            .map_or(deadline_us, |interval_due| interval_due.min(deadline_us));
        let wake = WakeToken {
            id,
            due_at_us,
            generation: SessionGeneration::from_states(vertical, horizontal),
        };
        self.last_wake_id = id;
        self.wake = Some(wake);
        Ok(Some(wake))
    }
}

fn active_deadline(
    now_us: u64,
    axis: ScheduleAxis,
    state: AxisStateSnapshot,
) -> Result<Option<u64>, SchedulerError> {
    if state.phase == DynamicsPhase::Idle {
        return Ok(None);
    }
    let deadline_us = state
        .deadline_us
        .ok_or(SchedulerError::MissingDeadline(axis))?;
    if deadline_us <= now_us {
        return Err(SchedulerError::DeadlineNotFuture {
            axis,
            deadline_us,
            observed_at_us: now_us,
        });
    }
    Ok(Some(deadline_us))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_ttl_is_anchored_to_scheduled_time_not_callback_time() {
        let sample = ScheduledSample::new(
            ScrollVector::ZERO,
            SessionGeneration::default(),
            1,
            10_000,
            15_000,
        )
        .unwrap();

        assert_eq!(sample.scheduled_at_us, 10_000);
        assert_eq!(sample.created_at_us, 15_000);
        assert_eq!(sample.expires_at_us, 18_000);
    }

    #[test]
    fn scheduled_time_overflow_is_an_explicit_scheduler_error() {
        assert!(matches!(
            sample_expiry(u64::MAX - SCHEDULED_SAMPLE_TTL_US + 1),
            Err(SchedulerError::SampleExpiryOverflow { .. })
        ));
    }
}
