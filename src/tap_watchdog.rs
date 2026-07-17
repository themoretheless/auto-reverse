//! Pure bounded recovery policy for a public CGEventTap enabled-state probe.

use std::time::{Duration, Instant};

pub const WATCHDOG_SAMPLE_INTERVAL: Duration = Duration::from_secs(1);
pub const WATCHDOG_MAX_ATTEMPTS: u8 = 3;
const UNHEALTHY_HYSTERESIS: u8 = 2;
const HEALTHY_RESET_HYSTERESIS: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapObservation {
    Enabled,
    Disabled,
    NotInstalled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchdogAction {
    Rearm,
    Restart,
}

#[derive(Debug, Default)]
pub struct TapWatchdog {
    next_sample: Option<Instant>,
    unhealthy_samples: u8,
    healthy_samples: u8,
    attempts: u8,
    exhausted: bool,
}

impl TapWatchdog {
    pub fn observe(
        &mut self,
        now: Instant,
        observation: TapObservation,
        restart_allowed: bool,
    ) -> Option<WatchdogAction> {
        if self.next_sample.is_some_and(|next| now < next) {
            return None;
        }
        self.next_sample = Some(now + WATCHDOG_SAMPLE_INTERVAL);

        if observation == TapObservation::Enabled {
            self.unhealthy_samples = 0;
            self.healthy_samples = self.healthy_samples.saturating_add(1);
            if self.healthy_samples >= HEALTHY_RESET_HYSTERESIS {
                self.attempts = 0;
                self.exhausted = false;
            }
            return None;
        }

        self.healthy_samples = 0;
        self.unhealthy_samples = self.unhealthy_samples.saturating_add(1);
        if self.unhealthy_samples < UNHEALTHY_HYSTERESIS {
            return None;
        }

        let action = match observation {
            TapObservation::Disabled => Some(WatchdogAction::Rearm),
            TapObservation::NotInstalled if restart_allowed => Some(WatchdogAction::Restart),
            TapObservation::NotInstalled | TapObservation::Enabled => None,
        };
        let action = action?;
        if self.attempts >= WATCHDOG_MAX_ATTEMPTS {
            self.exhausted = true;
            return None;
        }

        self.attempts += 1;
        self.unhealthy_samples = 0;
        Some(action)
    }

    /// Pauses sampling while the feature or its permission is unavailable.
    /// The attempt budget is retained so a permission flap cannot create an
    /// unbounded restart loop.
    pub fn suspend(&mut self) {
        self.next_sample = None;
        self.unhealthy_samples = 0;
        self.healthy_samples = 0;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn attempts(&self) -> u8 {
        self.attempts
    }

    pub fn exhausted(&self) -> bool {
        self.exhausted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_tap_needs_hysteresis_before_rearm() {
        let now = Instant::now();
        let mut watchdog = TapWatchdog::default();

        assert_eq!(watchdog.observe(now, TapObservation::Disabled, false), None);
        assert_eq!(
            watchdog.observe(
                now + WATCHDOG_SAMPLE_INTERVAL,
                TapObservation::Disabled,
                false
            ),
            Some(WatchdogAction::Rearm)
        );
    }

    #[test]
    fn restart_attempts_are_bounded_until_sustained_health() {
        let now = Instant::now();
        let mut watchdog = TapWatchdog::default();
        let mut actions = 0;
        for index in 0..10 {
            if watchdog
                .observe(
                    now + WATCHDOG_SAMPLE_INTERVAL * index,
                    TapObservation::NotInstalled,
                    true,
                )
                .is_some()
            {
                actions += 1;
            }
        }
        assert_eq!(actions, usize::from(WATCHDOG_MAX_ATTEMPTS));
        assert!(watchdog.exhausted());

        for index in 10..13 {
            watchdog.observe(
                now + WATCHDOG_SAMPLE_INTERVAL * index,
                TapObservation::Enabled,
                false,
            );
        }
        assert_eq!(watchdog.attempts(), 0);
        assert!(!watchdog.exhausted());
    }

    #[test]
    fn missing_port_does_not_spend_budget_before_restart_is_possible() {
        let now = Instant::now();
        let mut watchdog = TapWatchdog::default();
        watchdog.observe(now, TapObservation::NotInstalled, false);
        watchdog.observe(
            now + WATCHDOG_SAMPLE_INTERVAL,
            TapObservation::NotInstalled,
            false,
        );

        assert_eq!(watchdog.attempts(), 0);
        assert_eq!(
            watchdog.observe(
                now + WATCHDOG_SAMPLE_INTERVAL * 2,
                TapObservation::NotInstalled,
                true,
            ),
            Some(WatchdogAction::Restart)
        );
    }
}
