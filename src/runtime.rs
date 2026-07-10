//! Process-local runtime controls that intentionally do not persist to TOML.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const DEFAULT_PAUSE_DURATION: Duration = Duration::from_secs(15 * 60);

/// Shared temporary-pause state for the UI, tray and event-tap callback.
/// Reads are lock-free because every scroll event checks this value.
#[derive(Debug, Default)]
pub struct RuntimeControl {
    paused_until_ms: AtomicU64,
}

impl RuntimeControl {
    pub fn pause_for(&self, duration: Duration) {
        let duration_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
        self.paused_until_ms
            .store(now_millis().saturating_add(duration_ms), Ordering::Release);
    }

    pub fn resume(&self) {
        self.paused_until_ms.store(0, Ordering::Release);
    }

    pub fn remaining_pause(&self) -> Option<Duration> {
        let until = self.paused_until_ms.load(Ordering::Acquire);
        let remaining_ms = until.saturating_sub(now_millis());
        (remaining_ms > 0).then(|| Duration::from_millis(remaining_ms))
    }

    pub fn is_paused(&self) -> bool {
        self.remaining_pause().is_some()
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_and_resume_are_process_local_and_immediate() {
        let control = RuntimeControl::default();
        assert!(!control.is_paused());

        control.pause_for(Duration::from_secs(60));
        assert!(control.is_paused());
        assert!(
            control
                .remaining_pause()
                .is_some_and(|value| value.as_secs() <= 60)
        );

        control.resume();
        assert!(!control.is_paused());
    }

    #[test]
    fn expired_pause_is_not_reported() {
        let control = RuntimeControl::default();
        control.paused_until_ms.store(1, Ordering::Release);

        assert_eq!(control.remaining_pause(), None);
    }
}
