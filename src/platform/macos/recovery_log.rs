//! Process-local adapter around the pure bounded recovery audit.

use std::sync::{Mutex, OnceLock};

use crate::recovery_audit::{RecoveryAction, RecoveryAudit, RecoveryReason, RecoveryRecord};

static AUDIT: OnceLock<Mutex<RecoveryAudit>> = OnceLock::new();

fn audit() -> &'static Mutex<RecoveryAudit> {
    AUDIT.get_or_init(|| Mutex::new(RecoveryAudit::default()))
}

fn with_audit<T>(operation: impl FnOnce(&mut RecoveryAudit) -> T) -> T {
    let mut audit = audit()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    operation(&mut audit)
}

pub fn record_attempt(reason: RecoveryReason, action: RecoveryAction) -> RecoveryRecord {
    with_audit(|audit| audit.record_attempt(reason, action))
}

pub fn record_status(reason: RecoveryReason, action: RecoveryAction) -> RecoveryRecord {
    with_audit(|audit| audit.record_status(reason, action))
}

pub fn attempts(reason: RecoveryReason) -> u32 {
    with_audit(|audit| audit.attempts(reason))
}

pub fn snapshot() -> Vec<RecoveryRecord> {
    with_audit(|audit| audit.snapshot())
}

pub fn clear() {
    with_audit(RecoveryAudit::clear);
}
