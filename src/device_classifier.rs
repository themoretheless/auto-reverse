//! Pure scroll-source classification.
//!
//! The platform layer supplies a connected-device inventory plus observations
//! (continuous scroll, momentum phase, and whether a recent gesture had at
//! least two touching fingers). Keeping the policy here makes the heuristic
//! deterministic and testable without a live event tap or OS framework types.

use std::time::{Duration, Instant};

use crate::device::DeviceKind;

pub const CLASSIFIER_DESCRIPTION: &str = "discrete wheel = mouse; an exclusive connected trackpad or Magic Mouse wins; when both are connected, recent one-finger/two-finger gesture evidence and public scroll phases pin the source, while ambiguity falls back to trackpad";

const TRACKPAD_TOUCH_WINDOW: Duration = Duration::from_millis(222);
const SOURCE_RESET_WINDOW: Duration = Duration::from_millis(333);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MomentumPhase {
    None,
    Began,
    Continued,
    Ended,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollPhase {
    None,
    Began,
    Changed,
    Ended,
    Cancelled,
    MayBegin,
    Unknown,
}

/// Public-hardware evidence available before classifying a continuous scroll.
///
/// IOHID cannot attribute an individual continuous event, but it can answer a
/// simpler and decisive question: whether only one supported continuous
/// source is currently connected. The timing heuristic is needed only for
/// [`Self::Both`]. Unknown inventory deliberately falls back to trackpad so a
/// failed optional probe never reverses a built-in trackpad unexpectedly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ContinuousSourceHint {
    TrackpadOnly,
    MagicMouseOnly,
    Both,
    #[default]
    Unknown,
}

impl ContinuousSourceHint {
    pub const fn from_presence(trackpad: bool, magic_mouse: bool) -> Self {
        match (trackpad, magic_mouse) {
            (true, false) => Self::TrackpadOnly,
            (false, true) => Self::MagicMouseOnly,
            (true, true) => Self::Both,
            (false, false) => Self::Unknown,
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::TrackpadOnly => "trackpad only",
            Self::MagicMouseOnly => "Magic Mouse only",
            Self::Both => "trackpad and Magic Mouse",
            Self::Unknown => "no recognized continuous device",
        }
    }

    const fn exclusive_kind(self) -> Option<DeviceKind> {
        match self {
            Self::TrackpadOnly => Some(DeviceKind::Trackpad),
            Self::MagicMouseOnly => Some(DeviceKind::MagicMouse),
            Self::Both | Self::Unknown => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassificationEvidence {
    DiscreteWheel,
    ExclusiveTrackpadInventory,
    ExclusiveMagicMouseInventory,
    UnknownInventoryFallback,
    RecentTwoFingerGesture,
    RecentSingleFingerGesture,
    DirectGestureContinuation,
    MomentumContinuation,
    RecentSourceContinuation,
    AmbiguousTrackpadFallback,
}

impl ClassificationEvidence {
    pub const fn code(self) -> &'static str {
        match self {
            Self::DiscreteWheel => "discrete_wheel",
            Self::ExclusiveTrackpadInventory => "exclusive_trackpad_inventory",
            Self::ExclusiveMagicMouseInventory => "exclusive_magic_mouse_inventory",
            Self::UnknownInventoryFallback => "unknown_inventory_fallback",
            Self::RecentTwoFingerGesture => "recent_two_finger_gesture",
            Self::RecentSingleFingerGesture => "recent_single_finger_gesture",
            Self::DirectGestureContinuation => "direct_gesture_continuation",
            Self::MomentumContinuation => "momentum_continuation",
            Self::RecentSourceContinuation => "recent_source_continuation",
            Self::AmbiguousTrackpadFallback => "ambiguous_trackpad_fallback",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::DiscreteWheel => "discrete wheel",
            Self::ExclusiveTrackpadInventory => "exclusive trackpad inventory",
            Self::ExclusiveMagicMouseInventory => "exclusive Magic Mouse inventory",
            Self::UnknownInventoryFallback => "unknown inventory fallback",
            Self::RecentTwoFingerGesture => "recent two-finger gesture",
            Self::RecentSingleFingerGesture => "recent one-finger gesture",
            Self::DirectGestureContinuation => "pinned direct gesture source",
            Self::MomentumContinuation => "pinned momentum source",
            Self::RecentSourceContinuation => "recent continuous source",
            Self::AmbiguousTrackpadFallback => "ambiguous input; trackpad fallback",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassifiedDevice {
    pub kind: DeviceKind,
    pub evidence: ClassificationEvidence,
}

/// Stateful public-API heuristic for separating two continuous sources.
///
/// A two-finger AppKit gesture immediately preceding a scroll identifies a
/// trackpad; a positive one-finger observation identifies Magic Mouse-like
/// input. Public direct-scroll phases and momentum pin that positive source.
/// Missing observations never prove Magic Mouse: ambiguous input falls back
/// to trackpad so an optional monitor failure cannot reverse the built-in
/// trackpad unexpectedly.
#[derive(Debug, Clone)]
pub struct GestureSourceClassifier {
    source_hint: ContinuousSourceHint,
    last_gesture_observation: Option<(Instant, DeviceKind)>,
    gesture_observation_pending: bool,
    last_continuous_kind: DeviceKind,
    last_continuous_at: Option<Instant>,
    active_direct_kind: Option<DeviceKind>,
}

impl Default for GestureSourceClassifier {
    fn default() -> Self {
        Self::new(ContinuousSourceHint::Unknown)
    }
}

impl GestureSourceClassifier {
    pub const fn new(source_hint: ContinuousSourceHint) -> Self {
        Self {
            source_hint,
            last_gesture_observation: None,
            gesture_observation_pending: false,
            last_continuous_kind: match source_hint {
                ContinuousSourceHint::MagicMouseOnly => DeviceKind::MagicMouse,
                ContinuousSourceHint::TrackpadOnly
                | ContinuousSourceHint::Both
                | ContinuousSourceHint::Unknown => DeviceKind::Trackpad,
            },
            last_continuous_at: None,
            active_direct_kind: None,
        }
    }

    pub fn observe_gesture(&mut self, touching_fingers: usize, now: Instant) {
        let kind = match touching_fingers {
            0 => return,
            1 => DeviceKind::MagicMouse,
            _ => DeviceKind::Trackpad,
        };
        self.last_gesture_observation = Some((now, kind));
        self.gesture_observation_pending = true;
    }

    /// Applies a hot-plug inventory change and discards observations that
    /// belonged to the previous set of devices.
    pub fn update_source_hint(&mut self, source_hint: ContinuousSourceHint) {
        if self.source_hint != source_hint {
            *self = Self::new(source_hint);
        }
    }

    pub fn classify_scroll(
        &mut self,
        continuous: bool,
        momentum_phase: MomentumPhase,
        now: Instant,
    ) -> DeviceKind {
        self.classify_scroll_with_phases(continuous, ScrollPhase::None, momentum_phase, now)
            .kind
    }

    pub fn classify_scroll_with_evidence(
        &mut self,
        continuous: bool,
        momentum_phase: MomentumPhase,
        now: Instant,
    ) -> ClassifiedDevice {
        self.classify_scroll_with_phases(continuous, ScrollPhase::None, momentum_phase, now)
    }

    pub fn classify_scroll_with_phases(
        &mut self,
        continuous: bool,
        scroll_phase: ScrollPhase,
        momentum_phase: MomentumPhase,
        now: Instant,
    ) -> ClassifiedDevice {
        // Every scroll consumes the pending gesture observation. Otherwise a
        // mouse-wheel tick between a trackpad gesture and a later continuous
        // event could incorrectly lend that stale touch to the later device.
        let observation_pending = std::mem::take(&mut self.gesture_observation_pending);
        if !continuous {
            return ClassifiedDevice {
                kind: DeviceKind::Mouse,
                evidence: ClassificationEvidence::DiscreteWheel,
            };
        }

        if let Some(kind) = self.source_hint.exclusive_kind() {
            return self.remember(
                ClassifiedDevice {
                    kind,
                    evidence: match kind {
                        DeviceKind::Trackpad => ClassificationEvidence::ExclusiveTrackpadInventory,
                        DeviceKind::MagicMouse => {
                            ClassificationEvidence::ExclusiveMagicMouseInventory
                        }
                        DeviceKind::Mouse | DeviceKind::Unknown => {
                            ClassificationEvidence::UnknownInventoryFallback
                        }
                    },
                },
                now,
            );
        }
        if self.source_hint == ContinuousSourceHint::Unknown {
            return self.remember(
                ClassifiedDevice {
                    kind: DeviceKind::Trackpad,
                    evidence: ClassificationEvidence::UnknownInventoryFallback,
                },
                now,
            );
        }

        let observed = observation_pending
            .then_some(self.last_gesture_observation)
            .flatten()
            .filter(|(observed_at, _)| {
                now.saturating_duration_since(*observed_at) < TRACKPAD_TOUCH_WINDOW
            })
            .map(|(_, kind)| ClassifiedDevice {
                kind,
                evidence: match kind {
                    DeviceKind::Trackpad => ClassificationEvidence::RecentTwoFingerGesture,
                    DeviceKind::MagicMouse => ClassificationEvidence::RecentSingleFingerGesture,
                    DeviceKind::Mouse | DeviceKind::Unknown => {
                        ClassificationEvidence::AmbiguousTrackpadFallback
                    }
                },
            });
        let fallback = ClassifiedDevice {
            kind: DeviceKind::Trackpad,
            evidence: ClassificationEvidence::AmbiguousTrackpadFallback,
        };

        let direct = match scroll_phase {
            ScrollPhase::Began => {
                let classified = observed.unwrap_or(fallback);
                self.active_direct_kind = Some(classified.kind);
                Some(classified)
            }
            ScrollPhase::Changed => {
                let classified = observed.unwrap_or_else(|| {
                    self.active_direct_kind
                        .map(|kind| ClassifiedDevice {
                            kind,
                            evidence: ClassificationEvidence::DirectGestureContinuation,
                        })
                        .unwrap_or(fallback)
                });
                self.active_direct_kind = Some(classified.kind);
                Some(classified)
            }
            ScrollPhase::Ended | ScrollPhase::Cancelled => {
                let classified = self
                    .active_direct_kind
                    .map(|kind| ClassifiedDevice {
                        kind,
                        evidence: ClassificationEvidence::DirectGestureContinuation,
                    })
                    .or(observed)
                    .unwrap_or(fallback);
                self.active_direct_kind = None;
                Some(classified)
            }
            ScrollPhase::MayBegin => Some(observed.unwrap_or(fallback)),
            ScrollPhase::None | ScrollPhase::Unknown => None,
        };
        if let Some(classified) = direct {
            return self.remember(classified, now);
        }

        let classified = match momentum_phase {
            MomentumPhase::Began | MomentumPhase::Continued | MomentumPhase::Ended => {
                ClassifiedDevice {
                    kind: self.last_continuous_kind,
                    evidence: ClassificationEvidence::MomentumContinuation,
                }
            }
            MomentumPhase::Unknown => observed.unwrap_or(fallback),
            MomentumPhase::None => observed.unwrap_or_else(|| {
                if self
                    .last_continuous_at
                    .is_some_and(|last| now.saturating_duration_since(last) <= SOURCE_RESET_WINDOW)
                {
                    ClassifiedDevice {
                        kind: self.last_continuous_kind,
                        evidence: ClassificationEvidence::RecentSourceContinuation,
                    }
                } else {
                    fallback
                }
            }),
        };

        self.remember(classified, now)
    }

    fn remember(&mut self, classified: ClassifiedDevice, now: Instant) -> ClassifiedDevice {
        self.last_continuous_kind = classified.kind;
        self.last_continuous_at = Some(now);
        classified
    }

    /// Safe classification when the passive gesture monitor is unavailable.
    /// Exclusive hardware evidence remains useful; ambiguous/unknown input
    /// stays natural by falling back to trackpad.
    pub const fn classify_without_gesture(&self, continuous: bool) -> DeviceKind {
        self.classify_without_gesture_with_evidence(continuous).kind
    }

    pub const fn classify_without_gesture_with_evidence(
        &self,
        continuous: bool,
    ) -> ClassifiedDevice {
        if !continuous {
            return ClassifiedDevice {
                kind: DeviceKind::Mouse,
                evidence: ClassificationEvidence::DiscreteWheel,
            };
        }
        match self.source_hint {
            ContinuousSourceHint::MagicMouseOnly => ClassifiedDevice {
                kind: DeviceKind::MagicMouse,
                evidence: ClassificationEvidence::ExclusiveMagicMouseInventory,
            },
            ContinuousSourceHint::TrackpadOnly => ClassifiedDevice {
                kind: DeviceKind::Trackpad,
                evidence: ClassificationEvidence::ExclusiveTrackpadInventory,
            },
            ContinuousSourceHint::Both => ClassifiedDevice {
                kind: DeviceKind::Trackpad,
                evidence: ClassificationEvidence::AmbiguousTrackpadFallback,
            },
            ContinuousSourceHint::Unknown => ClassifiedDevice {
                kind: DeviceKind::Trackpad,
                evidence: ClassificationEvidence::UnknownInventoryFallback,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn after(start: Instant, milliseconds: u64) -> Instant {
        start + Duration::from_millis(milliseconds)
    }

    fn both_sources() -> GestureSourceClassifier {
        GestureSourceClassifier::new(ContinuousSourceHint::Both)
    }

    #[test]
    fn source_hint_maps_connected_hardware_without_guessing() {
        assert_eq!(
            ContinuousSourceHint::from_presence(true, false),
            ContinuousSourceHint::TrackpadOnly
        );
        assert_eq!(
            ContinuousSourceHint::from_presence(false, true),
            ContinuousSourceHint::MagicMouseOnly
        );
        assert_eq!(
            ContinuousSourceHint::from_presence(true, true),
            ContinuousSourceHint::Both
        );
        assert_eq!(
            ContinuousSourceHint::from_presence(false, false),
            ContinuousSourceHint::Unknown
        );
    }

    #[test]
    fn classification_exposes_the_evidence_used_for_each_major_path() {
        let now = Instant::now();
        let mut both = both_sources();
        both.observe_gesture(2, now);

        assert_eq!(
            both.classify_scroll_with_evidence(true, MomentumPhase::None, now),
            ClassifiedDevice {
                kind: DeviceKind::Trackpad,
                evidence: ClassificationEvidence::RecentTwoFingerGesture,
            }
        );
        assert_eq!(
            both.classify_scroll_with_evidence(true, MomentumPhase::Continued, after(now, 500),),
            ClassifiedDevice {
                kind: DeviceKind::Trackpad,
                evidence: ClassificationEvidence::MomentumContinuation,
            }
        );
        assert_eq!(
            GestureSourceClassifier::new(ContinuousSourceHint::MagicMouseOnly)
                .classify_without_gesture_with_evidence(true)
                .evidence,
            ClassificationEvidence::ExclusiveMagicMouseInventory
        );
        assert_eq!(
            GestureSourceClassifier::default()
                .classify_without_gesture_with_evidence(false)
                .evidence,
            ClassificationEvidence::DiscreteWheel
        );
    }

    #[test]
    fn exclusive_trackpad_inventory_overrides_missing_touch_observations() {
        let now = Instant::now();
        let mut classifier = GestureSourceClassifier::new(ContinuousSourceHint::TrackpadOnly);

        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, now),
            DeviceKind::Trackpad
        );
        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::Continued, now),
            DeviceKind::Trackpad
        );
    }

    #[test]
    fn exclusive_magic_mouse_inventory_needs_no_gesture_monitor() {
        let classifier = GestureSourceClassifier::new(ContinuousSourceHint::MagicMouseOnly);

        assert_eq!(
            classifier.classify_without_gesture(true),
            DeviceKind::MagicMouse
        );
        assert_eq!(
            classifier.classify_without_gesture(false),
            DeviceKind::Mouse
        );
    }

    #[test]
    fn hot_plug_replaces_stale_source_and_touch_state() {
        let now = Instant::now();
        let mut classifier = GestureSourceClassifier::new(ContinuousSourceHint::Both);
        classifier.observe_gesture(2, now);
        classifier.update_source_hint(ContinuousSourceHint::MagicMouseOnly);

        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, now),
            DeviceKind::MagicMouse
        );
        classifier.update_source_hint(ContinuousSourceHint::TrackpadOnly);
        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, now),
            DeviceKind::Trackpad
        );
    }

    #[test]
    fn unknown_or_ambiguous_inventory_falls_back_to_trackpad_without_gestures() {
        assert_eq!(
            GestureSourceClassifier::default().classify_without_gesture(true),
            DeviceKind::Trackpad
        );
        assert_eq!(
            both_sources().classify_without_gesture(true),
            DeviceKind::Trackpad
        );
    }

    #[test]
    fn discrete_scroll_is_always_a_mouse() {
        let now = Instant::now();
        let mut classifier = both_sources();
        classifier.observe_gesture(2, now);

        assert_eq!(
            classifier.classify_scroll(false, MomentumPhase::None, now),
            DeviceKind::Mouse
        );
    }

    #[test]
    fn discrete_scroll_consumes_pending_touch_observation() {
        let start = Instant::now();
        let mut classifier = both_sources();
        classifier.observe_gesture(2, start);
        assert_eq!(
            classifier.classify_scroll(false, MomentumPhase::None, after(start, 10)),
            DeviceKind::Mouse
        );

        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, after(start, 20)),
            DeviceKind::Trackpad
        );
    }

    #[test]
    fn recent_two_finger_gesture_identifies_trackpad() {
        let start = Instant::now();
        let mut classifier = both_sources();
        classifier.observe_gesture(2, start);

        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, after(start, 100)),
            DeviceKind::Trackpad
        );
    }

    #[test]
    fn recent_one_finger_gesture_is_positive_magic_mouse_evidence() {
        let now = Instant::now();
        let mut classifier = both_sources();
        classifier.observe_gesture(1, now);

        assert_eq!(
            classifier.classify_scroll_with_evidence(true, MomentumPhase::None, now),
            ClassifiedDevice {
                kind: DeviceKind::MagicMouse,
                evidence: ClassificationEvidence::RecentSingleFingerGesture,
            }
        );
    }

    #[test]
    fn continuous_scroll_without_touch_falls_back_to_trackpad() {
        let now = Instant::now();
        let mut classifier = both_sources();

        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, now),
            DeviceKind::Trackpad
        );
    }

    #[test]
    fn momentum_keeps_the_last_trackpad_source_after_touch_expires() {
        let start = Instant::now();
        let mut classifier = both_sources();
        classifier.observe_gesture(3, start);
        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::Began, after(start, 20)),
            DeviceKind::Trackpad
        );

        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::Continued, after(start, 500)),
            DeviceKind::Trackpad
        );
        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::Ended, after(start, 700)),
            DeviceKind::Trackpad
        );
    }

    #[test]
    fn normal_scroll_after_stale_touch_falls_back_to_trackpad() {
        let start = Instant::now();
        let mut classifier = both_sources();
        classifier.observe_gesture(2, start);
        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, after(start, 50)),
            DeviceKind::Trackpad
        );

        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, after(start, 334)),
            DeviceKind::Trackpad
        );
    }

    #[test]
    fn two_finger_observation_is_consumed_then_recent_source_is_retained() {
        let start = Instant::now();
        let mut classifier = both_sources();
        classifier.observe_gesture(2, start);
        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, after(start, 10)),
            DeviceKind::Trackpad
        );

        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, after(start, 20)),
            DeviceKind::Trackpad
        );
    }

    #[test]
    fn timing_boundaries_match_the_documented_strict_windows() {
        let start = Instant::now();
        let mut at_touch_boundary = both_sources();
        at_touch_boundary.observe_gesture(2, start);
        assert_eq!(
            at_touch_boundary.classify_scroll(true, MomentumPhase::None, after(start, 222)),
            DeviceKind::Trackpad
        );

        let mut at_reset_boundary = both_sources();
        at_reset_boundary.observe_gesture(2, start);
        assert_eq!(
            at_reset_boundary.classify_scroll(true, MomentumPhase::None, after(start, 10)),
            DeviceKind::Trackpad
        );
        assert_eq!(
            at_reset_boundary.classify_scroll(true, MomentumPhase::None, after(start, 333)),
            DeviceKind::Trackpad
        );
    }

    #[test]
    fn direct_scroll_phase_pins_positive_magic_mouse_evidence_until_end() {
        let start = Instant::now();
        let mut classifier = both_sources();
        classifier.observe_gesture(1, start);

        assert_eq!(
            classifier.classify_scroll_with_phases(
                true,
                ScrollPhase::Began,
                MomentumPhase::None,
                after(start, 10),
            ),
            ClassifiedDevice {
                kind: DeviceKind::MagicMouse,
                evidence: ClassificationEvidence::RecentSingleFingerGesture,
            }
        );
        assert_eq!(
            classifier.classify_scroll_with_phases(
                true,
                ScrollPhase::Changed,
                MomentumPhase::None,
                after(start, 600),
            ),
            ClassifiedDevice {
                kind: DeviceKind::MagicMouse,
                evidence: ClassificationEvidence::DirectGestureContinuation,
            }
        );
        assert_eq!(
            classifier
                .classify_scroll_with_phases(
                    true,
                    ScrollPhase::Ended,
                    MomentumPhase::None,
                    after(start, 700),
                )
                .kind,
            DeviceKind::MagicMouse
        );
        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, after(start, 1_034)),
            DeviceKind::Trackpad
        );
    }

    #[test]
    fn unknown_momentum_does_not_inherit_a_stale_magic_mouse_source() {
        let start = Instant::now();
        let mut classifier = both_sources();
        classifier.observe_gesture(1, start);
        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, start),
            DeviceKind::MagicMouse
        );

        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::Unknown, after(start, 500)),
            DeviceKind::Trackpad
        );
    }

    #[test]
    fn discrete_scroll_remains_mouse_with_unknown_inventory() {
        assert_eq!(
            GestureSourceClassifier::default().classify_without_gesture(false),
            DeviceKind::Mouse
        );
    }
}
