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
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::device::{DeviceKind, HardwareId};

/// Matches the design handoff's "ring buffer holds the last 500" label.
pub const CAPACITY: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Vertical,
    Horizontal,
}

impl Axis {
    pub fn code(self) -> &'static str {
        match self {
            Axis::Vertical => "vertical",
            Axis::Horizontal => "horizontal",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Axis::Vertical => "Vertical",
            Axis::Horizontal => "Horizontal",
        }
    }
}

/// Coarse category `DebugEvent::reason` falls into - what the Debug
/// Console's filter tabs (All/Reversed/Passed/Ignored) group by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionCategory {
    Reversed,
    Passed,
    Ignored,
}

impl DecisionCategory {
    pub fn code(self) -> &'static str {
        match self {
            Self::Reversed => "reversed",
            Self::Passed => "passed",
            Self::Ignored => "ignored",
        }
    }
}

/// Stable, machine-readable reason for one axis decision. These variants are
/// stored by the event-tap callback and exported as `reason_code`; changing UI
/// wording no longer changes the recorded data or allocates on the hot path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionReason {
    ScrollReversalOff,
    TemporarilyPaused,
    SyntheticEvent,
    RawInputGuard,
    Reversed,
    UnknownDeviceNotReversed,
    DeviceRuleDisabled,
    TrackpadNatural,
    DeviceReversalOff,
    AxisDisabled,
}

impl DecisionReason {
    pub fn code(self) -> &'static str {
        match self {
            Self::ScrollReversalOff => "scroll_reversal_off",
            Self::TemporarilyPaused => "temporarily_paused",
            Self::SyntheticEvent => "synthetic_event",
            Self::RawInputGuard => "raw_input_guard",
            Self::Reversed => "reversed",
            Self::UnknownDeviceNotReversed => "unknown_device_not_reversed",
            Self::DeviceRuleDisabled => "device_rule_disabled",
            Self::TrackpadNatural => "trackpad_natural",
            Self::DeviceReversalOff => "device_reversal_off",
            Self::AxisDisabled => "axis_disabled",
        }
    }

    pub fn category(self) -> DecisionCategory {
        match self {
            Self::Reversed => DecisionCategory::Reversed,
            Self::TrackpadNatural | Self::AxisDisabled => DecisionCategory::Passed,
            Self::ScrollReversalOff
            | Self::TemporarilyPaused
            | Self::SyntheticEvent
            | Self::RawInputGuard
            | Self::UnknownDeviceNotReversed
            | Self::DeviceRuleDisabled
            | Self::DeviceReversalOff => DecisionCategory::Ignored,
        }
    }

    pub fn display_text(self, device_kind: DeviceKind) -> Cow<'static, str> {
        match self {
            Self::ScrollReversalOff => Cow::Borrowed("Ignored – scroll reversal is off"),
            Self::TemporarilyPaused => Cow::Borrowed("Ignored - temporarily paused"),
            Self::SyntheticEvent => Cow::Borrowed("Ignored – synthetic event"),
            Self::RawInputGuard => Cow::Borrowed("Ignored – raw input guard (remote desktop)"),
            Self::Reversed => Cow::Borrowed("Reversed"),
            Self::UnknownDeviceNotReversed => {
                Cow::Borrowed("Ignored – unknown devices not reversed")
            }
            Self::DeviceRuleDisabled => {
                Cow::Borrowed("Ignored – this device has a Don't reverse rule")
            }
            Self::TrackpadNatural => Cow::Borrowed("Passed through – trackpad natural"),
            Self::DeviceReversalOff => Cow::Owned(format!(
                "Ignored – {} reversal is off",
                reversal_kind_label(device_kind)
            )),
            Self::AxisDisabled => Cow::Borrowed("Passed through"),
        }
    }
}

/// One structured scroll-reversal decision. Device names use `Arc<str>` so a
/// two-axis event shares one allocation, while all labels and explanations are
/// deferred until a consumer asks for them.
#[derive(Debug, Clone)]
pub struct DebugEvent {
    /// Milliseconds since UNIX_EPOCH - plain integer, cheap to store/sort,
    /// formatted to a wall-clock time only when the console renders a row.
    pub timestamp_ms: u128,
    pub device_kind: DeviceKind,
    /// Raw IOHID product name, preserved unchanged for structured export.
    pub device_name: Option<Arc<str>>,
    pub hardware: Option<HardwareId>,
    pub source_pid: i64,
    pub synthetic: bool,
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
            || self.source_pid.to_string().contains(needle)
            || contains_case_insensitive(if self.synthetic { "true" } else { "false" }, needle)
            || self.raw_delta.to_string().contains(needle)
            || self.output_delta.to_string().contains(needle)
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

/// Process-wide ring buffer. `OnceLock` rather than plumbing an `Arc`
/// through `event_tap`/`hid` call sites that don't otherwise need shared
/// state - the console window and the tap callback both just call
/// `buffer()`.
static BUFFER: OnceLock<Arc<Mutex<RingBuffer>>> = OnceLock::new();

fn buffer() -> &'static Arc<Mutex<RingBuffer>> {
    BUFFER.get_or_init(|| Arc::new(Mutex::new(RingBuffer::new())))
}

/// Hot-path entry point, called once per real scroll event from
/// `event_tap::handle_event`. The critical section is a plain push plus a
/// pop-front-if-over-capacity - no string formatting, no filtering, no I/O
/// happens while the lock is held; the caller builds `event` completely
/// before calling this.
pub fn push(event: DebugEvent) {
    let buf = buffer();
    let mut guard = match buf.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.push(event);
}

/// A cloned snapshot of the current buffer contents, oldest first. Cloning
/// out from under the lock (rather than handing back a guard) keeps the
/// lock's critical section short and lets the GUI thread filter/format the
/// snapshot without blocking the tap thread.
pub fn snapshot() -> Vec<DebugEvent> {
    let buf = buffer();
    let guard = match buf.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.snapshot()
}

pub fn clear() {
    let buf = buffer();
    let mut guard = match buf.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.clear();
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

fn reversal_kind_label(device_kind: DeviceKind) -> &'static str {
    match device_kind {
        DeviceKind::Mouse => "mouse",
        DeviceKind::Trackpad => "trackpad",
        DeviceKind::MagicMouse => "Magic Mouse",
        DeviceKind::Unknown => "unknown device",
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_event(tag: u32) -> DebugEvent {
        DebugEvent {
            timestamp_ms: tag as u128,
            device_kind: DeviceKind::Mouse,
            device_name: Some(Arc::from(format!("Test {tag}"))),
            hardware: Some(HardwareId {
                vendor_id: 0x1234,
                product_id: 0x5678,
            }),
            source_pid: 0,
            synthetic: false,
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
        assert!(event.matches_search("false"));
        assert!(event.matches_search("-1"));
        assert!(!event.matches_search("trackpad"));

        let sourced_event = DebugEvent {
            source_pid: 4242,
            synthetic: true,
            reason: DecisionReason::RawInputGuard,
            ..sample_event(2)
        };
        assert!(sourced_event.matches_search("4242"));
        assert!(sourced_event.matches_search("TRUE"));
        assert!(sourced_event.matches_search("raw_input_guard"));
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
