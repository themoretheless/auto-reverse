//! Temporary selection policy for experimental scroll-dynamics presets.

use std::time::{Duration, Instant};

use crate::scroll_dynamics::SmoothPreset;

pub const PRESET_PREVIEW_DURATION: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewEvent {
    None,
    Expired,
    Superseded,
}

#[derive(Debug, Clone, Copy)]
struct PendingPreview {
    committed: SmoothPreset,
    candidate: SmoothPreset,
    expires_at: Instant,
}

#[derive(Debug, Default)]
pub struct PresetPreview {
    pending: Option<PendingPreview>,
}

impl PresetPreview {
    pub fn select(&mut self, committed: SmoothPreset, candidate: SmoothPreset, now: Instant) {
        self.pending = (candidate != committed).then_some(PendingPreview {
            committed,
            candidate,
            expires_at: now + PRESET_PREVIEW_DURATION,
        });
    }

    pub fn tick(&mut self, committed: SmoothPreset, now: Instant) -> PreviewEvent {
        let Some(pending) = self.pending else {
            return PreviewEvent::None;
        };
        if pending.committed != committed {
            self.pending = None;
            return PreviewEvent::Superseded;
        }
        if now >= pending.expires_at {
            self.pending = None;
            return PreviewEvent::Expired;
        }
        PreviewEvent::None
    }

    pub fn displayed(&self, committed: SmoothPreset) -> SmoothPreset {
        self.pending
            .filter(|pending| pending.committed == committed)
            .map_or(committed, |pending| pending.candidate)
    }

    pub fn remaining(&self, committed: SmoothPreset, now: Instant) -> Option<Duration> {
        self.pending
            .filter(|pending| pending.committed == committed)
            .map(|pending| pending.expires_at.saturating_duration_since(now))
    }

    pub fn confirm(&mut self, committed: SmoothPreset, now: Instant) -> Option<SmoothPreset> {
        if self.tick(committed, now) != PreviewEvent::None {
            return None;
        }
        self.pending.take().map(|pending| pending.candidate)
    }

    pub fn cancel(&mut self) {
        self.pending = None;
    }

    pub fn is_pending(&self, committed: SmoothPreset) -> bool {
        self.pending
            .is_some_and(|pending| pending.committed == committed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unconfirmed_selection_expires_back_to_committed_value() {
        let now = Instant::now();
        let mut preview = PresetPreview::default();
        preview.select(SmoothPreset::Off, SmoothPreset::Balanced, now);

        assert_eq!(preview.displayed(SmoothPreset::Off), SmoothPreset::Balanced);
        assert_eq!(
            preview.tick(SmoothPreset::Off, now + PRESET_PREVIEW_DURATION),
            PreviewEvent::Expired
        );
        assert_eq!(preview.displayed(SmoothPreset::Off), SmoothPreset::Off);
    }

    #[test]
    fn confirmation_returns_candidate_only_before_deadline() {
        let now = Instant::now();
        let mut preview = PresetPreview::default();
        preview.select(SmoothPreset::Off, SmoothPreset::Fast, now);

        assert_eq!(
            preview.confirm(SmoothPreset::Off, now + Duration::from_secs(2)),
            Some(SmoothPreset::Fast)
        );
        assert!(!preview.is_pending(SmoothPreset::Off));
    }

    #[test]
    fn external_commit_supersedes_pending_preview() {
        let now = Instant::now();
        let mut preview = PresetPreview::default();
        preview.select(SmoothPreset::Off, SmoothPreset::Precise, now);

        assert_eq!(
            preview.tick(SmoothPreset::Fast, now + Duration::from_secs(1)),
            PreviewEvent::Superseded
        );
        assert_eq!(preview.displayed(SmoothPreset::Fast), SmoothPreset::Fast);
    }
}
