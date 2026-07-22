//! A small, bounded ring buffer of scroll-reversal decisions, feeding the
//! Debug Console window (handoff "1f"). Written from the CGEventTap callback
//! thread on every real scroll event; read from the GUI/main thread while
//! the console viewport is open.
//!
//! This module does not decide anything - it records the structured result of
//! `crate::scroll::transform_event` plus source metadata already available at
//! the `event_tap::handle_event` call site. The callback stores stable enums
//! and raw values; user-facing text is derived only when the console searches,
//! renders or exports a snapshot.
//!
//! Local-only: nothing in this module (or anywhere else in this project)
//! sends data over a network. The ring buffer lives in process memory only
//! (Export writes a local file the user explicitly asks for).

use std::borrow::Cow;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, TryLockError};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::config::ResolvedDeviceProfile;
use crate::device::{DeviceIdentity, DeviceKind, HardwareId};
pub use crate::device_attribution::AttributionStatus;
use crate::device_classifier::ClassificationEvidence;
use crate::device_source::HidSourceClass;
pub use crate::diagnostics::{Axis, DecisionCategory, DecisionReason};
use crate::input_policy::InputProvenance;

/// Matches the design handoff's "ring buffer holds the last 500" label.
pub const CAPACITY: usize = 500;

/// One structured scroll-reversal decision. Device names use `Arc<str>` so a
/// two-axis event shares one allocation, while all labels and explanations are
/// deferred until a consumer asks for them.
#[derive(Debug, Clone)]
pub struct DebugEvent {
    /// Milliseconds since UNIX_EPOCH - plain integer, cheap to store/sort,
    /// formatted to a wall-clock time only when the console renders a row.
    pub timestamp_ms: u128,
    /// Process-relative monotonic time used only to derive relative trace
    /// intervals. Unlike timestamp_ms, this cannot reveal wall-clock time.
    pub monotonic_us: u64,
    pub device_kind: DeviceKind,
    /// Raw IOHID product name, preserved unchanged for structured export.
    pub device_name: Option<Arc<str>>,
    /// Exact public HID identity kept only in process memory for the Devices
    /// tab's local test. Exporters intentionally have no column for it.
    pub identity: Option<Arc<DeviceIdentity>>,
    pub hardware: Option<HardwareId>,
    pub attribution_status: AttributionStatus,
    pub classification_evidence: ClassificationEvidence,
    pub input_provenance: InputProvenance,
    pub hid_source: HidSourceClass,
    pub profile: ResolvedDeviceProfile,
    pub source_pid: i64,
    pub synthetic: bool,
    pub continuous: bool,
    pub axis: Axis,
    pub raw_delta: i64,
    pub output_delta: i64,
    pub reason: DecisionReason,
}

impl DebugEvent {
    pub fn device_description(&self) -> String {
        device_description(self.device_kind, self.device_name.as_deref())
    }

    pub fn decision_text(&self) -> Cow<'static, str> {
        self.reason.display_text(self.device_kind)
    }

    pub fn category(&self) -> DecisionCategory {
        self.reason.category()
    }

    pub fn resolution_summary(&self) -> String {
        format!(
            "Snapshot: attribution {}, HID {}; classifier: {} -> {}; input: {}; profile: direction {} from {}, step {} from {}, preset {} from {}; final: {} ({})",
            self.attribution_status.code(),
            self.hid_source.label(),
            self.classification_evidence.label(),
            self.device_kind,
            self.input_provenance.label(),
            if self.profile.reverse.value {
                "reverse"
            } else {
                "natural"
            },
            self.profile.reverse.source.label(),
            self.profile.step_size.value,
            self.profile.step_size.source.label(),
            self.profile.smooth_preset.value.as_str(),
            self.profile.smooth_preset.source.label(),
            self.category().code(),
            self.reason.code(),
        )
    }

    pub fn matches_search(&self, needle: &str) -> bool {
        let needle = needle.trim();
        if needle.is_empty() {
            return true;
        }

        let device_description = self.device_description();
        let decision_text = self.decision_text();
        let hardware = self.hardware.map(|id| id.to_string()).unwrap_or_default();

        contains_case_insensitive(&device_description, needle)
            || self
                .device_name
                .as_deref()
                .is_some_and(|name| contains_case_insensitive(name, needle))
            || contains_case_insensitive(self.device_kind.as_str(), needle)
            || contains_case_insensitive(self.axis.code(), needle)
            || contains_case_insensitive(self.axis.label(), needle)
            || contains_case_insensitive(&decision_text, needle)
            || contains_case_insensitive(self.reason.code(), needle)
            || contains_case_insensitive(self.category().code(), needle)
            || contains_case_insensitive(&hardware, needle)
            || contains_case_insensitive(self.attribution_status.code(), needle)
            || contains_case_insensitive(self.classification_evidence.code(), needle)
            || contains_case_insensitive(self.input_provenance.code(), needle)
            || contains_case_insensitive(self.hid_source.code(), needle)
            || contains_case_insensitive(self.profile.reverse.source.code(), needle)
            || contains_case_insensitive(self.profile.step_size.source.code(), needle)
            || contains_case_insensitive(self.profile.smooth_preset.source.code(), needle)
            || contains_case_insensitive(self.profile.smooth_preset.value.as_str(), needle)
            || self.profile.reverse.value.to_string().contains(needle)
            || self.profile.step_size.value.to_string().contains(needle)
            || self.source_pid.to_string().contains(needle)
            || contains_case_insensitive(if self.synthetic { "true" } else { "false" }, needle)
            || contains_case_insensitive(
                if self.continuous {
                    "continuous"
                } else {
                    "discrete"
                },
                needle,
            )
            || self.raw_delta.to_string().contains(needle)
            || self.output_delta.to_string().contains(needle)
            || contains_case_insensitive(&self.resolution_summary(), needle)
    }
}

fn contains_case_insensitive(haystack: &str, needle: &str) -> bool {
    if needle.is_ascii() {
        let needle = needle.as_bytes();
        return haystack
            .as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle));
    }

    haystack.to_lowercase().contains(&needle.to_lowercase())
}

/// The actual bounded-eviction logic, factored out of the process-wide
/// static below so it can be unit-tested directly (a `static` singleton
/// shared across every `#[test]` in the binary would make tests interfere
/// with each other and with real ring-buffer traffic).
struct RingBuffer {
    events: VecDeque<DebugEvent>,
}

impl RingBuffer {
    fn new() -> Self {
        Self {
            events: VecDeque::with_capacity(CAPACITY),
        }
    }

    fn push(&mut self, event: DebugEvent) {
        if self.events.len() >= CAPACITY {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    fn snapshot(&self) -> Vec<DebugEvent> {
        self.events.iter().cloned().collect()
    }

    fn clear(&mut self) {
        self.events.clear();
    }
}

struct DebugLog {
    buffer: Mutex<RingBuffer>,
    dropped_records: AtomicU64,
}

impl DebugLog {
    fn new() -> Self {
        Self {
            buffer: Mutex::new(RingBuffer::new()),
            dropped_records: AtomicU64::new(0),
        }
    }

    /// Attempts one bounded write without waiting for a GUI reader. A
    /// poisoned mutex is still owned when `try_lock` reports it, so recovering
    /// that guard remains non-blocking and keeps diagnostics fail-open.
    fn try_push(&self, event: DebugEvent) -> bool {
        match self.buffer.try_lock() {
            Ok(mut guard) => {
                guard.push(event);
                true
            }
            Err(TryLockError::Poisoned(poisoned)) => {
                poisoned.into_inner().push(event);
                true
            }
            Err(TryLockError::WouldBlock) => {
                self.record_drop();
                false
            }
        }
    }

    fn snapshot(&self) -> Vec<DebugEvent> {
        let guard = match self.buffer.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.snapshot()
    }

    fn clear(&self) {
        let mut guard = match self.buffer.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.clear();
    }

    fn record_drop(&self) {
        let _ = self
            .dropped_records
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                count.checked_add(1)
            });
    }

    fn dropped_records(&self) -> u64 {
        self.dropped_records.load(Ordering::Relaxed)
    }

    fn take_dropped_records(&self) -> u64 {
        self.dropped_records.swap(0, Ordering::Relaxed)
    }
}

/// Process-wide diagnostics state. `OnceLock` avoids plumbing a shared owner
/// through event-tap and GUI call sites that otherwise do not need one.
static DEBUG_LOG: OnceLock<DebugLog> = OnceLock::new();

fn debug_log() -> &'static DebugLog {
    DEBUG_LOG.get_or_init(DebugLog::new)
}

/// Hot-path entry point, called once per real scroll event from
/// `event_tap::handle_event`. This never waits for the GUI's snapshot lock: if
/// it is contended, this diagnostic record is dropped and counted. The caller
/// builds `event` completely before calling this; no formatting, filtering or
/// I/O happens here.
pub fn push(event: DebugEvent) {
    debug_log().try_push(event);
}

/// A cloned snapshot of the current buffer contents, oldest first. Cloning
/// out from under the lock (rather than handing back a guard) keeps the
/// lock's critical section short and lets the GUI thread filter/format the
/// snapshot without blocking the tap thread.
pub fn snapshot() -> Vec<DebugEvent> {
    debug_log().snapshot()
}

pub fn clear() {
    debug_log().clear();
}

/// Number of records omitted because the ring was busy. This is independent
/// of ring eviction: reaching [`CAPACITY`] still evicts the oldest record and
/// does not increment this counter.
pub fn dropped_records() -> u64 {
    debug_log().dropped_records()
}

/// Atomically returns and resets the contention-drop count. Clearing the ring
/// does not reset this value, so consumers can choose their own reporting
/// interval without losing data unexpectedly.
pub fn take_dropped_records() -> u64 {
    debug_log().take_dropped_records()
}

/// Human-readable device description, built outside the event-tap callback.
/// The raw name remains untouched in `DebugEvent`; whitespace normalization
/// applies only to this UI-facing representation.
pub fn device_description(device_kind: DeviceKind, device_name: Option<&str>) -> String {
    let kind_label = device_kind_label(device_kind);
    match device_name.and_then(normalized_device_name) {
        Some(name) => format!("{kind_label} · {name}"),
        None => kind_label.to_string(),
    }
}

fn device_kind_label(device_kind: DeviceKind) -> &'static str {
    match device_kind {
        DeviceKind::Mouse => "Mouse wheel",
        DeviceKind::Trackpad => "Trackpad",
        DeviceKind::MagicMouse => "Magic Mouse",
        DeviceKind::Unknown => "Unknown device",
    }
}

fn normalized_device_name(name: &str) -> Option<String> {
    let normalized = name.split_whitespace().collect::<Vec<_>>().join(" ");
    (!normalized.is_empty()).then_some(normalized)
}

pub fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

static MONOTONIC_ORIGIN: OnceLock<Instant> = OnceLock::new();

pub fn now_monotonic_micros() -> u64 {
    let elapsed = MONOTONIC_ORIGIN.get_or_init(Instant::now).elapsed();
    u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use std::sync::mpsc;
    use std::thread;

    fn sample_event(tag: u32) -> DebugEvent {
        DebugEvent {
            timestamp_ms: tag as u128,
            monotonic_us: u64::from(tag),
            device_kind: DeviceKind::Mouse,
            device_name: Some(Arc::from(format!("Test {tag}"))),
            identity: None,
            hardware: Some(HardwareId {
                vendor_id: 0x1234,
                product_id: 0x5678,
            }),
            attribution_status: AttributionStatus::HighConfidence,
            classification_evidence: ClassificationEvidence::DiscreteWheel,
            input_provenance: InputProvenance::Hardware,
            hid_source: HidSourceClass::Physical,
            profile: AppConfig::default().resolve_device_profile(DeviceKind::Mouse, None),
            source_pid: 0,
            synthetic: false,
            continuous: false,
            axis: Axis::Vertical,
            raw_delta: 1,
            output_delta: -1,
            reason: DecisionReason::Reversed,
        }
    }

    #[test]
    fn ring_buffer_evicts_oldest_once_over_capacity() {
        let mut buffer = RingBuffer::new();
        for tag in 0..(CAPACITY as u32 + 5) {
            buffer.push(sample_event(tag));
        }

        let snapshot = buffer.snapshot();

        assert_eq!(snapshot.len(), CAPACITY);
        // The first 5 pushed events (tags 0..5) should have been evicted;
        // the oldest surviving entry is tag 5.
        assert_eq!(snapshot.first().unwrap().timestamp_ms, 5);
        assert_eq!(snapshot.last().unwrap().timestamp_ms, CAPACITY as u128 + 4);
    }

    #[test]
    fn ring_buffer_clear_empties_it() {
        let mut buffer = RingBuffer::new();
        buffer.push(sample_event(1));
        buffer.push(sample_event(2));

        buffer.clear();

        assert!(buffer.snapshot().is_empty());
    }

    #[test]
    fn ring_buffer_preserves_insertion_order() {
        let mut buffer = RingBuffer::new();
        buffer.push(sample_event(1));
        buffer.push(sample_event(2));
        buffer.push(sample_event(3));

        let snapshot = buffer.snapshot();

        assert_eq!(
            snapshot.iter().map(|e| e.timestamp_ms).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn debug_log_drops_without_waiting_when_buffer_is_contended() {
        let log = Arc::new(DebugLog::new());
        let holder_log = Arc::clone(&log);
        let (locked_tx, locked_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let holder = thread::spawn(move || {
            let _guard = holder_log.buffer.lock().unwrap();
            locked_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });

        locked_rx.recv().unwrap();
        assert!(!log.try_push(sample_event(1)));
        assert_eq!(log.dropped_records(), 1);

        release_tx.send(()).unwrap();
        holder.join().unwrap();

        assert!(log.try_push(sample_event(2)));
        assert_eq!(log.snapshot().len(), 1);
        assert_eq!(log.take_dropped_records(), 1);
        assert_eq!(log.dropped_records(), 0);
    }

    #[test]
    fn debug_log_recovers_a_poisoned_buffer_without_panicking() {
        let log = Arc::new(DebugLog::new());
        let poison_log = Arc::clone(&log);
        let poisoner = thread::spawn(move || {
            let _guard = poison_log.buffer.lock().unwrap();
            panic!("poison diagnostics mutex");
        });

        assert!(poisoner.join().is_err());
        assert!(log.try_push(sample_event(7)));
        assert_eq!(log.dropped_records(), 0);
        assert_eq!(log.snapshot()[0].timestamp_ms, 7);

        log.clear();
        assert!(log.snapshot().is_empty());
    }

    #[test]
    fn dropped_record_count_saturates() {
        let log = DebugLog::new();
        log.dropped_records.store(u64::MAX, Ordering::Relaxed);

        log.record_drop();

        assert_eq!(log.dropped_records(), u64::MAX);
    }

    #[test]
    fn device_description_includes_name_when_known() {
        assert_eq!(
            device_description(DeviceKind::Mouse, Some("Logitech MX Master")),
            "Mouse wheel · Logitech MX Master"
        );
    }

    #[test]
    fn device_description_falls_back_to_kind_label_when_name_unknown() {
        assert_eq!(device_description(DeviceKind::Trackpad, None), "Trackpad");
        assert_eq!(
            device_description(DeviceKind::Unknown, None),
            "Unknown device"
        );
    }

    #[test]
    fn device_description_normalizes_only_the_ui_copy() {
        let raw = "  MX\nMaster\t3S  ";
        assert_eq!(
            device_description(DeviceKind::Mouse, Some(raw)),
            "Mouse wheel · MX Master 3S"
        );

        let event = DebugEvent {
            device_name: Some(Arc::from(raw)),
            ..sample_event(1)
        };
        assert_eq!(event.device_name.as_deref(), Some(raw));
    }

    #[test]
    fn every_decision_reason_has_a_stable_code_and_category() {
        let cases = [
            (
                DecisionReason::ScrollReversalOff,
                "scroll_reversal_off",
                DecisionCategory::Ignored,
            ),
            (
                DecisionReason::TemporarilyPaused,
                "temporarily_paused",
                DecisionCategory::Ignored,
            ),
            (
                DecisionReason::SyntheticEvent,
                "synthetic_event",
                DecisionCategory::Ignored,
            ),
            (
                DecisionReason::VirtualHidSource,
                "virtual_hid_source",
                DecisionCategory::Ignored,
            ),
            (
                DecisionReason::UnknownHidSource,
                "unknown_hid_source",
                DecisionCategory::Ignored,
            ),
            (
                DecisionReason::RawInputGuard,
                "raw_input_guard",
                DecisionCategory::Ignored,
            ),
            (
                DecisionReason::Reversed,
                "reversed",
                DecisionCategory::Reversed,
            ),
            (
                DecisionReason::DeviceRuleReversed,
                "device_rule_reversed",
                DecisionCategory::Reversed,
            ),
            (
                DecisionReason::UnknownDeviceNotReversed,
                "unknown_device_not_reversed",
                DecisionCategory::Ignored,
            ),
            (
                DecisionReason::DeviceRuleDisabled,
                "device_rule_disabled",
                DecisionCategory::Ignored,
            ),
            (
                DecisionReason::TrackpadNatural,
                "trackpad_natural",
                DecisionCategory::Passed,
            ),
            (
                DecisionReason::DeviceReversalOff,
                "device_reversal_off",
                DecisionCategory::Ignored,
            ),
            (
                DecisionReason::AxisDisabled,
                "axis_disabled",
                DecisionCategory::Passed,
            ),
        ];

        for (reason, code, category) in cases {
            assert_eq!(reason.code(), code);
            assert_eq!(reason.category(), category);
        }
    }

    #[test]
    fn device_reversal_off_text_is_formatted_lazily_from_kind() {
        assert_eq!(
            DecisionReason::DeviceReversalOff.display_text(DeviceKind::Mouse),
            "Ignored – mouse reversal is off"
        );
        assert_eq!(
            DecisionReason::DeviceReversalOff.display_text(DeviceKind::MagicMouse),
            "Ignored – Magic Mouse reversal is off"
        );
    }

    #[test]
    fn search_matches_are_case_insensitive_substrings() {
        let event = sample_event(1);
        assert!(event.matches_search(""));
        assert!(event.matches_search("mouse"));
        assert!(event.matches_search("TEST"));
        assert!(event.matches_search("vertical"));
        assert!(event.matches_search("reversed"));
        assert!(event.matches_search("1234"));
        assert!(event.matches_search("high"));
        assert!(event.matches_search("discrete_wheel"));
        assert!(event.matches_search("mouse_kind"));
        assert!(event.matches_search("mouse setting"));
        assert!(event.matches_search("off"));
        assert!(event.matches_search("false"));
        assert!(event.matches_search("-1"));
        assert!(!event.matches_search("trackpad"));

        let sourced_event = DebugEvent {
            source_pid: 4242,
            synthetic: true,
            input_provenance: InputProvenance::SelfSynthetic,
            reason: DecisionReason::RawInputGuard,
            ..sample_event(2)
        };
        assert!(sourced_event.matches_search("4242"));
        assert!(sourced_event.matches_search("TRUE"));
        assert!(sourced_event.matches_search("raw_input_guard"));
    }

    #[test]
    fn resolution_summary_covers_snapshot_classifier_profile_and_final_decision() {
        let summary = sample_event(1).resolution_summary();

        for required in [
            "Snapshot: attribution high",
            "classifier: discrete wheel -> mouse",
            "input: Hardware",
            "direction reverse from mouse setting",
            "preset off from global default",
            "final: reversed (reversed)",
        ] {
            assert!(
                summary.contains(required),
                "missing {required:?} in {summary:?}"
            );
        }
    }

    #[test]
    fn process_wide_push_is_visible_in_a_later_snapshot() {
        // Exercises the actual static-backed public API (push/snapshot),
        // not just the RingBuffer struct in isolation. This static is
        // process-wide and this module has no other test touching it, so a
        // simple "what I pushed is present" check (not an absolute length,
        // in case a parallel test elsewhere in the binary somehow shared
        // it) is enough without needing a `clear()` race with anything.
        let marker = now_millis().max(1) + 1;
        push(DebugEvent {
            timestamp_ms: marker,
            ..sample_event(0)
        });

        assert!(snapshot().iter().any(|e| e.timestamp_ms == marker));
    }
}
