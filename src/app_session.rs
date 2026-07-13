//! Pure target-PID pinning contract for future per-application rules.
//!
//! App rules are not live yet. This module exists so their eventual adapter
//! cannot switch policy halfway through a direct-scroll or momentum session.

use std::time::Duration;

pub const APP_SESSION_IDLE_TIMEOUT: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollSessionPhase {
    Direct,
    Momentum,
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinnedAppTarget {
    pub target_pid: Option<i32>,
    pub generation: u64,
    pub started_new_session: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPinError {
    TimestampRegressed,
    GenerationOverflow,
}

#[derive(Debug, Clone, Copy)]
struct ActiveSession {
    target_pid: Option<i32>,
    generation: u64,
    last_event_at: Duration,
}

#[derive(Debug, Clone, Default)]
pub struct AppTargetSessionPin {
    active: Option<ActiveSession>,
    last_generation: u64,
}

impl AppTargetSessionPin {
    pub fn observe(
        &mut self,
        candidate_pid: Option<i32>,
        phase: ScrollSessionPhase,
        at: Duration,
    ) -> Result<Option<PinnedAppTarget>, SessionPinError> {
        let candidate_pid = candidate_pid.filter(|pid| *pid > 0);
        let Some(active) = self.active else {
            return if phase == ScrollSessionPhase::Direct {
                self.start_session(candidate_pid, at).map(Some)
            } else {
                Ok(None)
            };
        };

        if at < active.last_event_at {
            return Err(SessionPinError::TimestampRegressed);
        }
        let elapsed = at - active.last_event_at;
        if elapsed > APP_SESSION_IDLE_TIMEOUT {
            self.active = None;
            return if phase == ScrollSessionPhase::Direct {
                self.start_session(candidate_pid, at).map(Some)
            } else {
                Ok(None)
            };
        }

        let pinned = PinnedAppTarget {
            target_pid: active.target_pid,
            generation: active.generation,
            started_new_session: false,
        };
        if phase == ScrollSessionPhase::Ended {
            self.active = None;
        } else if let Some(active) = &mut self.active {
            active.last_event_at = at;
        }
        Ok(Some(pinned))
    }

    pub fn cancel(&mut self) {
        self.active = None;
    }

    fn start_session(
        &mut self,
        target_pid: Option<i32>,
        at: Duration,
    ) -> Result<PinnedAppTarget, SessionPinError> {
        let generation = self
            .last_generation
            .checked_add(1)
            .ok_or(SessionPinError::GenerationOverflow)?;
        self.last_generation = generation;
        self.active = Some(ActiveSession {
            target_pid,
            generation,
            last_event_at: at,
        });
        Ok(PinnedAppTarget {
            target_pid,
            generation,
            started_new_session: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn target_pid_cannot_change_inside_direct_or_momentum_session() {
        let mut pin = AppTargetSessionPin::default();

        let first = pin
            .observe(Some(100), ScrollSessionPhase::Direct, ms(0))
            .unwrap()
            .unwrap();
        let direct = pin
            .observe(Some(200), ScrollSessionPhase::Direct, ms(20))
            .unwrap()
            .unwrap();
        let momentum = pin
            .observe(Some(300), ScrollSessionPhase::Momentum, ms(40))
            .unwrap()
            .unwrap();

        assert_eq!(first.target_pid, Some(100));
        assert_eq!(direct.target_pid, Some(100));
        assert_eq!(momentum.target_pid, Some(100));
        assert_eq!(first.generation, momentum.generation);
        assert!(first.started_new_session);
        assert!(!momentum.started_new_session);
    }

    #[test]
    fn new_direct_input_after_idle_gap_can_pin_a_new_target() {
        let mut pin = AppTargetSessionPin::default();
        let first = pin
            .observe(Some(100), ScrollSessionPhase::Direct, ms(0))
            .unwrap()
            .unwrap();
        let second = pin
            .observe(
                Some(200),
                ScrollSessionPhase::Direct,
                APP_SESSION_IDLE_TIMEOUT + ms(1),
            )
            .unwrap()
            .unwrap();

        assert_eq!(second.target_pid, Some(200));
        assert!(second.generation > first.generation);
        assert!(second.started_new_session);
    }

    #[test]
    fn orphaned_or_stale_momentum_never_adopts_a_new_candidate() {
        let mut pin = AppTargetSessionPin::default();
        assert_eq!(
            pin.observe(Some(100), ScrollSessionPhase::Momentum, ms(0))
                .unwrap(),
            None
        );

        pin.observe(Some(100), ScrollSessionPhase::Direct, ms(10))
            .unwrap();
        assert_eq!(
            pin.observe(Some(200), ScrollSessionPhase::Momentum, ms(200))
                .unwrap(),
            None
        );
    }

    #[test]
    fn ended_and_cancelled_sessions_release_the_pin() {
        let mut pin = AppTargetSessionPin::default();
        pin.observe(Some(100), ScrollSessionPhase::Direct, ms(0))
            .unwrap();
        let ended = pin
            .observe(Some(200), ScrollSessionPhase::Ended, ms(10))
            .unwrap()
            .unwrap();
        assert_eq!(ended.target_pid, Some(100));
        assert_eq!(
            pin.observe(Some(200), ScrollSessionPhase::Momentum, ms(20))
                .unwrap(),
            None
        );

        pin.observe(Some(300), ScrollSessionPhase::Direct, ms(30))
            .unwrap();
        pin.cancel();
        assert_eq!(
            pin.observe(Some(400), ScrollSessionPhase::Momentum, ms(40))
                .unwrap(),
            None
        );
    }

    #[test]
    fn invalid_pid_is_pinned_as_unknown_and_time_cannot_move_backwards() {
        let mut pin = AppTargetSessionPin::default();
        let pinned = pin
            .observe(Some(0), ScrollSessionPhase::Direct, ms(10))
            .unwrap()
            .unwrap();
        assert_eq!(pinned.target_pid, None);
        assert_eq!(
            pin.observe(Some(100), ScrollSessionPhase::Direct, ms(9)),
            Err(SessionPinError::TimestampRegressed)
        );
    }
}
