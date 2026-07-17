//! Notification-led refresh scheduling with a rare timer backstop.

use std::time::{Duration, Instant};

pub const REFRESH_BACKSTOP_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RefreshRequest {
    pub permissions: bool,
    pub devices: bool,
}

#[derive(Debug)]
pub struct RefreshPolicy {
    seen_device_generation: u64,
    next_backstop: Instant,
}

impl RefreshPolicy {
    pub fn new(now: Instant, device_generation: u64) -> Self {
        Self {
            seen_device_generation: device_generation,
            next_backstop: now + REFRESH_BACKSTOP_INTERVAL,
        }
    }

    pub fn poll(
        &mut self,
        now: Instant,
        app_became_active: bool,
        did_wake: bool,
        device_generation: u64,
    ) -> RefreshRequest {
        let backstop_due = now >= self.next_backstop;
        if backstop_due {
            // Do not replay every missed interval after sleep or a stalled UI.
            self.next_backstop = now + REFRESH_BACKSTOP_INTERVAL;
        }

        let inventory_changed = device_generation != self.seen_device_generation;
        if inventory_changed {
            self.seen_device_generation = device_generation;
        }

        RefreshRequest {
            permissions: app_became_active || did_wake || backstop_due,
            devices: inventory_changed || app_became_active || did_wake || backstop_due,
        }
    }

    pub fn acknowledge_device_generation(&mut self, generation: u64) {
        self.seen_device_generation = generation;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifications_refresh_only_the_relevant_state() {
        let now = Instant::now();
        let mut policy = RefreshPolicy::new(now, 4);

        assert_eq!(
            policy.poll(now + Duration::from_secs(1), false, false, 5),
            RefreshRequest {
                permissions: false,
                devices: true,
            }
        );
        assert_eq!(
            policy.poll(now + Duration::from_secs(2), true, false, 5),
            RefreshRequest {
                permissions: true,
                devices: true,
            }
        );
    }

    #[test]
    fn late_backstop_coalesces_into_one_refresh() {
        let now = Instant::now();
        let mut policy = RefreshPolicy::new(now, 0);
        let late = now + REFRESH_BACKSTOP_INTERVAL * 4;

        assert_eq!(
            policy.poll(late, false, false, 0),
            RefreshRequest {
                permissions: true,
                devices: true,
            }
        );
        assert_eq!(
            policy.poll(late + Duration::from_secs(1), false, false, 0),
            RefreshRequest::default()
        );
    }
}
