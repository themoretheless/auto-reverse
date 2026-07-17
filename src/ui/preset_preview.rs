//! Compact, temporary UI for the non-live dynamics model.

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText};

use crate::preset_preview::{PresetPreview, PreviewEvent};
use crate::scroll_dynamics::SmoothPreset;

use super::theme::{status_dot, styled_button};

const REVERT_NOTICE_DURATION: Duration = Duration::from_secs(3);

#[derive(Debug, Default)]
pub(super) struct State {
    preview: PresetPreview,
    revert_notice_until: Option<Instant>,
}

impl State {
    pub(super) fn tick(&mut self, committed: SmoothPreset) {
        let now = Instant::now();
        if self.preview.tick(committed, now) == PreviewEvent::Expired {
            self.revert_notice_until = Some(now + REVERT_NOTICE_DURATION);
        }
        if self
            .revert_notice_until
            .is_some_and(|notice_until| now >= notice_until)
        {
            self.revert_notice_until = None;
        }
    }

    pub(super) fn cancel(&mut self) {
        self.preview.cancel();
        self.revert_notice_until = None;
    }
}

/// Returns a newly confirmed preset. Merely selecting a segment changes only
/// this local preview and never mutates the persisted or event-tap config.
pub(super) fn controls(
    ui: &mut egui::Ui,
    state: &mut State,
    committed: SmoothPreset,
) -> Option<SmoothPreset> {
    state.tick(committed);
    let now = Instant::now();
    let displayed = state.preview.displayed(committed);

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for preset in SmoothPreset::ALL {
            if ui
                .selectable_label(displayed == preset, preset.label())
                .clicked()
            {
                state.preview.select(committed, preset, now);
                state.revert_notice_until = None;
            }
        }
    });

    let parameters = state.preview.displayed(committed).parameters();
    let immediate = f32::from(parameters.immediate_per_mille) / 1_000.0;
    let tail = parameters.tail_duration_us as f32 / 120_000.0;
    ui.add(
        egui::ProgressBar::new(immediate)
            .desired_width(ui.available_width())
            .text(format!(
                "Immediate {}%",
                parameters.immediate_per_mille / 10
            )),
    );
    ui.add(
        egui::ProgressBar::new(tail)
            .desired_width(ui.available_width())
            .text(format!("Tail {} ms", parameters.tail_duration_us / 1_000)),
    );
    ui.label(
        RichText::new(state.preview.displayed(committed).goal())
            .small()
            .weak(),
    );
    ui.label(
        RichText::new("Preview model only; live scrolling remains exact.")
            .small()
            .color(Color32::from_rgb(0x6E, 0x6E, 0x73)),
    );

    let mut confirmed = None;
    if state.preview.is_pending(committed) {
        let seconds = state
            .preview
            .remaining(committed, now)
            .map_or(0, |remaining| remaining.as_secs().saturating_add(1));
        ui.horizontal_wrapped(|ui| {
            status_dot(ui, Color32::from_rgb(0xE5, 0x9E, 0x2F), 3.0, 8.0);
            ui.label(RichText::new(format!("Temporary preview · {seconds}s")).small());
            if styled_button(ui, "Use preset", egui::vec2(10.0, 4.0)).clicked() {
                confirmed = state.preview.confirm(committed, now);
            }
            if styled_button(ui, "Revert", egui::vec2(10.0, 4.0)).clicked() {
                state.preview.cancel();
            }
        });
    } else if state.revert_notice_until.is_some() {
        ui.label(
            RichText::new("Preview expired · restored saved preset")
                .small()
                .weak(),
        );
    }

    confirmed
}
