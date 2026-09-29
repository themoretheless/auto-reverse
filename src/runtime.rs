//! Process-local runtime controls that intentionally do not persist to TOML.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

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
    // A pause is process-local and duration-based, so wall-clock time is the
    // wrong clock: NTP corrections or a manual clock change could otherwise
    // expire it early or extend it unexpectedly. `Instant` is monotonic and
    // makes the stored deadline immune to those adjustments.
    static PROCESS_EPOCH: OnceLock<Instant> = OnceLock::new();
    let elapsed = PROCESS_EPOCH.get_or_init(Instant::now).elapsed();
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
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
    fn zero_length_pause_is_not_reported() {
        let control = RuntimeControl::default();

        control.pause_for(Duration::ZERO);

        assert!(!control.is_paused());
    }

    #[test]
    fn expired_pause_is_not_reported() {
        let control = RuntimeControl::default();
        control.paused_until_ms.store(0, Ordering::Release);

        assert_eq!(control.remaining_pause(), None);
    }
}
