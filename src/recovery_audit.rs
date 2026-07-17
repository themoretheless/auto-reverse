//! Pure, privacy-bounded audit vocabulary for runtime recovery attempts.

use std::collections::{BTreeMap, VecDeque};

pub const RECOVERY_AUDIT_CAPACITY: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RecoveryReason {
    Wake,
    TapTimeout,
    TapUserInput,
    WatchdogDisabled,
    PermissionLoss,
}

impl RecoveryReason {
    pub const ALL: [Self; 5] = [
        Self::Wake,
        Self::TapTimeout,
        Self::TapUserInput,
        Self::WatchdogDisabled,
        Self::PermissionLoss,
    ];

    pub const fn code(self) -> &'static str {
        match self {
            Self::Wake => "wake",
            Self::TapTimeout => "tap_timeout",
            Self::TapUserInput => "tap_user_input",
            Self::WatchdogDisabled => "watchdog_disabled",
            Self::PermissionLoss => "permission_loss",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Wake => "Wake",
            Self::TapTimeout => "Tap timeout",
            Self::TapUserInput => "Tap disabled by input",
            Self::WatchdogDisabled => "Watchdog",
            Self::PermissionLoss => "Permission loss",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAction {
    Detected,
    Rearmed,
    RestartRequested,
    Suspended,
    Restored,
    Failed,
    Exhausted,
    UserRetry,
}

impl RecoveryAction {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Detected => "detected",
            Self::Rearmed => "rearmed",
            Self::RestartRequested => "restart_requested",
            Self::Suspended => "suspended",
            Self::Restored => "restored",
            Self::Failed => "failed",
            Self::Exhausted => "exhausted",
            Self::UserRetry => "user_retry",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Detected => "Detected",
            Self::Rearmed => "Re-armed",
            Self::RestartRequested => "Restart requested",
            Self::Suspended => "Suspended",
            Self::Restored => "Restored",
            Self::Failed => "Failed",
            Self::Exhausted => "Exhausted",
            Self::UserRetry => "User retry",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryRecord {
    pub sequence: u64,
    pub reason: RecoveryReason,
    pub action: RecoveryAction,
    /// Attempt number within this reason only. Status-only records retain the
    /// latest number and therefore never make a different reason look retried.
    pub attempt: u32,
}

#[derive(Debug)]
pub struct RecoveryAudit {
    records: VecDeque<RecoveryRecord>,
    attempts: BTreeMap<RecoveryReason, u32>,
    next_sequence: u64,
}

impl Default for RecoveryAudit {
    fn default() -> Self {
        Self {
            records: VecDeque::with_capacity(RECOVERY_AUDIT_CAPACITY),
            attempts: BTreeMap::new(),
            next_sequence: 1,
        }
    }
}

impl RecoveryAudit {
    pub fn record_attempt(
        &mut self,
        reason: RecoveryReason,
        action: RecoveryAction,
    ) -> RecoveryRecord {
        let attempt = {
            let attempt = self
                .attempts
                .entry(reason)
                .and_modify(|attempt| *attempt = attempt.saturating_add(1))
                .or_insert(1);
            *attempt
        };
        self.push(reason, action, attempt)
    }

    pub fn record_status(
        &mut self,
        reason: RecoveryReason,
        action: RecoveryAction,
    ) -> RecoveryRecord {
        self.push(reason, action, self.attempts(reason))
    }

    pub fn attempts(&self, reason: RecoveryReason) -> u32 {
        self.attempts.get(&reason).copied().unwrap_or(0)
    }

    pub fn snapshot(&self) -> Vec<RecoveryRecord> {
        self.records.iter().copied().collect()
    }

    pub fn clear(&mut self) {
        self.records.clear();
        self.attempts.clear();
        self.next_sequence = 1;
    }

    fn push(
        &mut self,
        reason: RecoveryReason,
        action: RecoveryAction,
        attempt: u32,
    ) -> RecoveryRecord {
        let record = RecoveryRecord {
            sequence: self.next_sequence,
            reason,
            action,
            attempt,
        };
        self.next_sequence = self.next_sequence.saturating_add(1);
        if self.records.len() >= RECOVERY_AUDIT_CAPACITY {
            self.records.pop_front();
        }
        self.records.push_back(record);
        record
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attempts_are_counted_independently_by_reason() {
        let mut audit = RecoveryAudit::default();
        audit.record_attempt(RecoveryReason::Wake, RecoveryAction::Rearmed);
        audit.record_attempt(RecoveryReason::TapTimeout, RecoveryAction::Rearmed);
        audit.record_attempt(RecoveryReason::Wake, RecoveryAction::RestartRequested);

        assert_eq!(audit.attempts(RecoveryReason::Wake), 2);
        assert_eq!(audit.attempts(RecoveryReason::TapTimeout), 1);
        assert_eq!(audit.attempts(RecoveryReason::PermissionLoss), 0);
    }

    #[test]
    fn status_records_retain_but_do_not_increment_attempt() {
        let mut audit = RecoveryAudit::default();
        audit.record_attempt(RecoveryReason::PermissionLoss, RecoveryAction::Suspended);
        let restored =
            audit.record_status(RecoveryReason::PermissionLoss, RecoveryAction::Restored);

        assert_eq!(restored.attempt, 1);
        assert_eq!(audit.attempts(RecoveryReason::PermissionLoss), 1);
    }

    #[test]
    fn ring_is_bounded_and_clear_resets_episode_counts() {
        let mut audit = RecoveryAudit::default();
        for _ in 0..RECOVERY_AUDIT_CAPACITY + 5 {
            audit.record_attempt(RecoveryReason::Wake, RecoveryAction::Failed);
        }
        let snapshot = audit.snapshot();
        assert_eq!(snapshot.len(), RECOVERY_AUDIT_CAPACITY);
        assert_eq!(snapshot[0].sequence, 6);

        audit.clear();
        assert!(audit.snapshot().is_empty());
        assert_eq!(audit.attempts(RecoveryReason::Wake), 0);
    }
}
