//! A small, bounded ring buffer of scroll-reversal decisions, feeding the
//! Debug Console window (handoff "1f"). Written from the CGEventTap callback
//! thread on every real scroll event; read from the GUI/main thread while
//! the console viewport is open.
//!
//! This module does not decide anything - it only records what
//! `crate::scroll::transform_event` (via
//! `platform::macos::scroll_events::apply_config_in_place`) already decided,
//! plus the small amount of extra context (`config.enabled`,
//! `event.synthetic`, the raw-input guard, `should_reverse`) that is already
//! computed or trivially available at the `event_tap::handle_event` call
//! site. It intentionally does not reach back into `scroll.rs` to add new
//! fields - every string produced here is derived from data the call site
//! already has.
//!
//! Local-only: nothing in this module (or anywhere else in this project)
//! sends data over a network. The ring buffer lives in process memory only
//! (Export writes a local file the user explicitly asks for).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::device::DeviceKind;

/// Matches the design handoff's "ring buffer holds the last 500" label.
pub const CAPACITY: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Vertical,
    Horizontal,
}

impl Axis {
    pub fn label(self) -> &'static str {
        match self {
            Axis::Vertical => "Vertical",
            Axis::Horizontal => "Horizontal",
        }
    }
}

/// Coarse category `DebugEvent::decision` falls into - what the Debug
/// Console's filter tabs (All/Reversed/Passed/Ignored) group by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionCategory {
    Reversed,
    Passed,
    Ignored,
}

/// One recorded scroll-reversal decision. Deliberately a plain, owned,
/// `Copy`-free-but-cheap-to-clone struct with no formatting/allocation done
/// on the hot path beyond what's unavoidable (the two `String`s are built
/// once, outside the lock, before `push`).
#[derive(Debug, Clone)]
pub struct DebugEvent {
    /// Milliseconds since UNIX_EPOCH - plain integer, cheap to store/sort,
    /// formatted to a wall-clock time only when the console renders a row.
    pub timestamp_ms: u128,
    pub device_description: String,
    pub axis: Axis,
    pub raw_delta: i64,
    pub output_delta: i64,
    pub decision_text: String,
    pub category: DecisionCategory,
}

impl DebugEvent {
    pub fn matches_search(&self, needle: &str) -> bool {
        let needle = needle.trim().to_lowercase();
        if needle.is_empty() {
            return true;
        }

        self.device_description.to_lowercase().contains(&needle)
            || self.axis.label().to_lowercase().contains(&needle)
            || self.decision_text.to_lowercase().contains(&needle)
            || self.raw_delta.to_string().contains(&needle)
            || self.output_delta.to_string().contains(&needle)
    }
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

/// Human-readable device description, e.g. "Mouse wheel · Logitech" or
/// "Trackpad" or "Unknown device" - reuses `DeviceKind`'s existing labels
/// plus whatever device name the caller already resolved (see
/// `event_tap::handle_event`'s `hid::recent_wheel_device` /
/// `hid::cached_device_name` lookup), rather than this module doing its own
/// (expensive - `list_pointing_devices` opens an IOHIDManager) name lookup
/// on every single scroll event.
pub fn device_description(device_kind: DeviceKind, device_name: Option<&str>) -> String {
    let kind_label = match device_kind {
        DeviceKind::Mouse => "Mouse wheel",
        DeviceKind::Trackpad => "Trackpad",
        DeviceKind::MagicMouse => "Magic Mouse",
        DeviceKind::Unknown => "Unknown device",
    };
    match device_name {
        Some(name) => format!("{kind_label} · {name}"),
        None => kind_label.to_string(),
    }
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
            device_description: format!("Mouse wheel · Test {tag}"),
            axis: Axis::Vertical,
            raw_delta: 1,
            output_delta: -1,
            decision_text: "Reversed".to_string(),
            category: DecisionCategory::Reversed,
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
    fn search_matches_are_case_insensitive_substrings() {
        let event = sample_event(1);
        assert!(event.matches_search(""));
        assert!(event.matches_search("mouse"));
        assert!(event.matches_search("TEST"));
        assert!(event.matches_search("vertical"));
        assert!(event.matches_search("reversed"));
        assert!(event.matches_search("-1"));
        assert!(!event.matches_search("trackpad"));
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
