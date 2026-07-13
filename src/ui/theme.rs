//! Design tokens and custom egui controls from the selected handoff.

use eframe::egui::{self, Color32, RichText};

use crate::config::AppConfig;

use super::SettingsTab;

pub(super) fn status_header(
    ui: &mut egui::Ui,
    config: &AppConfig,
    permissions_ready: bool,
    pause_remaining: Option<std::time::Duration>,
) {
    let (dot_color, status_word) = if !config.enabled {
        (Color32::GRAY, "OFF")
    } else if !permissions_ready {
        (Color32::from_rgb(0xE5, 0x9E, 0x2F), "NEEDS PERMISSION")
    } else if pause_remaining.is_some() {
        (Color32::GRAY, "PAUSED")
    } else {
        (Color32::from_rgb(0x34, 0xA8, 0x53), "ON")
    };

    ui.horizontal(|ui| {
        status_dot(ui, dot_color, 6.0, 16.0);
        ui.label(RichText::new(status_word).size(18.0).strong());
    });
    let summary = match (permissions_ready, pause_remaining) {
        (false, _) if config.enabled => {
            "Accessibility is required before reversal can run.".to_string()
        }
        (_, Some(remaining)) => {
            let minutes = remaining.as_secs().div_ceil(60).max(1);
            format!("Temporarily paused. Resumes in {minutes} min; settings are unchanged.")
        }
        (_, None) => config.plain_english_summary(),
    };
    ui.label(RichText::new(summary).weak());
}

pub(super) fn status_dot(ui: &mut egui::Ui, color: Color32, radius: f32, size: f32) {
    // A painted circle, not the "●" glyph: egui's default font can render
    // that codepoint inconsistently, which reads as a broken status icon.
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), radius, color);
}

/// The segmented-control tab strip (handoff "1b"): a light-gray track with
/// the active segment drawn as a raised, contrasting pill. egui has no
/// built-in tab widget, so this is built from `ui.horizontal` plus manually
/// painted rects - the exact colors below are read from the handoff's id="1b"
/// section (`background:#E3E3E7` track / `#fff` active segment with a
/// `box-shadow` in light mode, `#2C2C2E` track / `#48484A` active segment in
/// dark mode). `ui.visuals().dark_mode` is what already drives this
/// project's system-native light/dark following elsewhere, so the same
/// check picks the right pair here instead of introducing a separate theme
/// mechanism.
pub(super) fn tab_strip(ui: &mut egui::Ui, selected: &mut SettingsTab) {
    let dark = ui.visuals().dark_mode;
    let track_color = if dark {
        Color32::from_rgb(0x2C, 0x2C, 0x2E)
    } else {
        Color32::from_rgb(0xE3, 0xE3, 0xE7)
    };
    let active_color = if dark {
        Color32::from_rgb(0x48, 0x48, 0x4A)
    } else {
        Color32::WHITE
    };
    let active_text = if dark {
        Color32::from_rgb(0xF2, 0xF2, 0xF3)
    } else {
        Color32::from_rgb(0x1D, 0x1D, 0x1F)
    };
    let inactive_text = if dark {
        Color32::from_rgb(0x9A, 0x9A, 0xA0)
    } else {
        Color32::from_rgb(0x6E, 0x6E, 0x73)
    };

    egui::Frame::new()
        .fill(track_color)
        .corner_radius(8.0)
        .inner_margin(2.0)
        .show(ui, |ui| {
            let total_width = ui.available_width();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let segment_width = (total_width - 6.0) / 4.0;
                for (tab, label) in [
                    (SettingsTab::General, "General"),
                    (SettingsTab::Devices, "Devices"),
                    (SettingsTab::Permissions, "Permissions"),
                    (SettingsTab::Advanced, "Advanced"),
                ] {
                    let is_active = *selected == tab;
                    let (color, text_color) = if is_active {
                        (active_color, active_text)
                    } else {
                        (Color32::TRANSPARENT, inactive_text)
                    };
                    let button = egui::Button::new(
                        RichText::new(label).size(12.0).strong().color(text_color),
                    )
                    .fill(color)
                    .corner_radius(6.0)
                    .min_size(egui::vec2(segment_width, 24.0));
                    if ui.add(button).clicked() {
                        *selected = tab;
                    }
                }
            });
        });
}

// ---------------------------------------------------------------------
// Design tokens (handoff "1b"/"1f") shared by the custom-painted controls
// below - the checkbox, the wheel-step slider, the bordered-chip buttons,
// and the device-rule dropdown chip all read from these instead of each
// re-deriving the same light/dark pairs inline. `tab_strip` above and
// `debug_filter_strip` below predate this pass and already hardcode their
// own copies of some of the same hex values inline - left as-is rather than
// refactored onto these tokens, since both are explicitly out of scope
// ("already correctly styled, leave that alone, do not touch working code").
// ---------------------------------------------------------------------

/// Accent blue: `#2F6FE4` light / `#5B93FF` dark. Drives the checked
/// checkbox fill, the slider's filled track portion, and the device-rule
/// "Reverse" chip's border/text/arrow.
pub(super) fn accent_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x5B, 0x93, 0xFF)
    } else {
        Color32::from_rgb(0x2F, 0x6F, 0xE4)
    }
}

/// Color painted ON TOP of `accent_color` (the checkmark glyph inside a
/// checked checkbox): white in light mode, near-black `#1E1E1F` in dark
/// mode per the handoff. Deliberately NOT the same as `primary_text_color`.
pub(super) fn accent_glyph_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x1E, 0x1E, 0x1F)
    } else {
        Color32::WHITE
    }
}

/// Neutral border: `#C7C7CC` light / `#48484A` dark. Used by the unchecked
/// checkbox border, every bordered-chip button, the slider knob's border,
/// and the device-rule chip's "Default"/"Don't reverse" border.
pub(super) fn control_border_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x48, 0x48, 0x4A)
    } else {
        Color32::from_rgb(0xC7, 0xC7, 0xCC)
    }
}

/// Neutral surface: `#fff` light / `#2C2C2E` dark. Used by the unchecked
/// checkbox background, every bordered-chip button's background, and the
/// device-rule chip's "Default"/"Don't reverse" background.
pub(super) fn control_surface_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x2C, 0x2C, 0x2E)
    } else {
        Color32::WHITE
    }
}

/// Primary label text: `#1D1D1F` light / `#F2F2F3` dark. Used by button
/// labels and the device-rule chip's "Default"/"Reverse" text. (Checkbox
/// labels intentionally use `ui.visuals().text_color()` instead - see
/// `styled_checkbox`'s doc comment for why.)
pub(super) fn primary_text_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0xF2, 0xF2, 0xF3)
    } else {
        Color32::from_rgb(0x1D, 0x1D, 0x1F)
    }
}

/// Muted secondary tone: `#9A9AA0`, used as-is in both themes per the
/// handoff (the same value `tab_strip` above already uses for its own dark
/// `inactive_text`). Used here for the device-rule chip's "Default" dropdown
/// arrow, which the handoff draws in a different, dimmer color than the
/// chip's own text.
pub(super) fn muted_glyph_color() -> Color32 {
    Color32::from_rgb(0x9A, 0x9A, 0xA0)
}

/// The device-rule chip's "Reverse" background in dark mode. NOT shown
/// anywhere in the handoff - it depicts the light-mode Devices tab and the
/// dark-mode General tab, but never a dark-mode Devices tab, so there is no
/// literal value to read. Extrapolated as `accent_color(true)` blended 20%
/// over `control_surface_color(true)` (`#2C2C2E`), mirroring the light
/// mode's own relationship (`#EEF3FE` is accent blue tinted into white).
/// Computed once as a fixed, opaque hex rather than left as a translucent
/// overlay, so it reads the same regardless of what's behind it.
pub(super) fn reverse_chip_dark_bg() -> Color32 {
    Color32::from_rgb(0x35, 0x41, 0x58)
}

/// A custom-painted checkbox matching the handoff's id="1b" exactly - egui's
/// built-in `Checkbox` widget has a different shape/corner-radius/color
/// than the mockup. Same low-level `ui.allocate_exact_size` + `ui.painter()`
/// pattern `status_dot` above already uses, not a different one. One
/// helper serves both sizes the mockup uses: the master "Reverse scrolling"
/// toggle (18x18, 5px corners) and the four sub-checkboxes (16x16, 4px
/// corners) - callers pass `box_size`/`corner_radius` accordingly.
///
/// The whole row (box + label) is one clickable region, matching
/// `ui.checkbox`'s own behavior where clicking the label also toggles it:
/// the box and label are laid out first with `Sense::hover()` each (so
/// neither reacts to a click on its own), then a single `ui.interact` over
/// their combined bounding rect drives the actual toggle - exactly one
/// toggle per click, not a double-count from two overlapping click senses.
///
/// Checkbox labels deliberately use `ui.visuals().text_color()` (the
/// current theme's ordinary text color) rather than the handoff's literal
/// `#1D1D1F`/`#F2F2F3` hex - every other untouched label in this file
/// already renders through the theme's default text color rather than a
/// hardcoded one, so hardcoding it only for checkbox labels would make them
/// subtly mismatch their neighboring labels instead of matching the mockup
/// more closely.
///
/// Preserves `ui.checkbox`'s exact behavior: toggles `*checked`, returns a
/// `Response` whose `.changed()` is set exactly when the value actually
/// flipped (so existing `changed |= ....changed()` call sites don't need to
/// change), and respects `ui.add_enabled_ui`'s disabled/greyed-out state
/// automatically - disabling a `Ui` multiplies its `Painter`'s opacity (see
/// `Ui::disable`), which applies to raw `ui.painter()` calls exactly like it
/// does to built-in widgets, and also disables `Sense`d interaction on that
/// `Ui` (see `Ui::interact`'s use of `self.enabled`), so no extra plumbing
/// is needed here for either.
pub(super) fn styled_checkbox(
    ui: &mut egui::Ui,
    checked: &mut bool,
    label: impl Into<RichText>,
    box_size: f32,
    corner_radius: f32,
) -> egui::Response {
    let label: RichText = label.into();
    let dark = ui.visuals().dark_mode;
    let fill = accent_color(dark);
    let glyph_color = accent_glyph_color(dark);
    let border_color = control_border_color(dark);
    let unchecked_bg = control_surface_color(dark);
    let text_color = ui.visuals().text_color();

    let id = ui.make_persistent_id(("styled_checkbox", label.text().to_owned()));

    let row = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0; // handoff "gap:10px"
        let (box_rect, _) =
            ui.allocate_exact_size(egui::vec2(box_size, box_size), egui::Sense::hover());
        if ui.is_rect_visible(box_rect) {
            let painter = ui.painter();
            if *checked {
                painter.rect_filled(box_rect, corner_radius, fill);
                // Exact handoff checkmark sizes, not a ratio of box_size:
                // 12px for the 18px master toggle, 11px for the 16px
                // sub-checkboxes - a 0.62 ratio previously used here landed
                // at 11.16px/9.92px, neither an exact match.
                let checkmark_size = if box_size >= 18.0 { 12.0 } else { 11.0 };
                painter.text(
                    box_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "✓",
                    egui::FontId::proportional(checkmark_size),
                    glyph_color,
                );
            } else {
                painter.rect_filled(box_rect, corner_radius, unchecked_bg);
                painter.rect_stroke(
                    box_rect,
                    corner_radius,
                    egui::Stroke::new(1.5, border_color),
                    egui::StrokeKind::Inside,
                );
            }
        }
        // `selectable(false)`: a plain `ui.label()` senses click-and-drag
        // (egui's default `interaction.selectable_labels = true`), which
        // competes with the row's own outer click sense below - a click
        // that drifts a few points while over the label text lands on the
        // label's text-selection drag instead of the checkbox toggle,
        // silently skipping it. Not selectable, this label only senses
        // hover, so the outer `ui.interact` below is the sole click target.
        ui.add(egui::Label::new(label.color(text_color)).selectable(false));
    });

    let mut response = ui.interact(row.response.rect, id, egui::Sense::click());
    if response.clicked() {
        *checked = !*checked;
        response.mark_changed();
    }
    response
}

/// Custom-painted equivalent of `egui::Slider::new(value, min..=max)`, used
/// for "Wheel step size" (handoff "1b"): a 4px track, a blue fill portion
/// sized to the current value's proportion of the range, and a 14px round
/// knob with a border and a subtle drop shadow - matching the mockup's
/// slider chrome instead of egui's own default slider widget. Same
/// `ui.allocate_exact_size` + `ui.painter()` + drag-response pattern as this
/// file's other custom-painted controls; same value semantics as the
/// `egui::Slider` it replaces (drag or click along the track to change the
/// value, result clamped to `min..=max`, `Response::changed()` set exactly
/// when the value actually moved).
///
/// Takes an explicit `width` (rather than grabbing `ui.available_width()`
/// internally) because, per the handoff, the trailing numeric value label
/// shares the same row and needs room reserved for it - the caller computes
/// `width` accordingly (see the "Wheel step size" call site).
pub(super) fn styled_step_slider(
    ui: &mut egui::Ui,
    value: &mut i64,
    min: i64,
    max: i64,
    width: f32,
) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let track_bg = if dark {
        Color32::from_rgb(0x2C, 0x2C, 0x2E)
    } else {
        Color32::from_rgb(0xE3, 0xE3, 0xE7)
    };
    let fill_color = accent_color(dark);
    let knob_fill = if dark {
        Color32::from_rgb(0xE8, 0xE8, 0xE9)
    } else {
        Color32::WHITE
    };
    let knob_border = control_border_color(dark);
    // box-shadow alpha: 0.4 dark / 0.15 light, per the handoff.
    let knob_shadow = if dark {
        Color32::from_black_alpha(0x66)
    } else {
        Color32::from_black_alpha(0x26)
    };

    let desired_size = egui::vec2(width, 20.0);
    let (rect, mut response) = ui.allocate_exact_size(desired_size, egui::Sense::click_and_drag());

    let span = (max - min).max(1) as f32;
    if (response.dragged() || response.clicked())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let t = ((pointer.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
        let new_value = (min + (t * span).round() as i64).clamp(min, max);
        if new_value != *value {
            *value = new_value;
            response.mark_changed();
        }
    }

    if ui.is_rect_visible(rect) {
        let track_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), 4.0));
        let painter = ui.painter();
        painter.rect_filled(track_rect, 2.0, track_bg);

        let t = ((*value - min) as f32 / span).clamp(0.0, 1.0);
        let fill_width = track_rect.width() * t;
        if fill_width > 0.0 {
            let fill_rect = egui::Rect::from_min_size(track_rect.min, egui::vec2(fill_width, 4.0));
            painter.rect_filled(fill_rect, 2.0, fill_color);
        }

        let knob_center = egui::pos2(track_rect.left() + fill_width, rect.center().y);
        const KNOB_RADIUS: f32 = 7.0; // 14px diameter, per the handoff
        painter.circle_filled(knob_center + egui::vec2(0.0, 1.0), KNOB_RADIUS, knob_shadow);
        painter.circle_filled(knob_center, KNOB_RADIUS, knob_fill);
        painter.circle_stroke(
            knob_center,
            KNOB_RADIUS,
            egui::Stroke::new(1.0, knob_border),
        );
    }

    response
}

/// Wraps `label` in the handoff's bordered-chip button chrome
/// (`border:1px solid #C7C7CC`/`#48484A`, `border-radius:6px`,
/// `background:#fff`/`#2C2C2E`, 12px text) used for every plain button in
/// the mockup - Restore defaults, Refresh devices, Turn on/off, Retry
/// starting scroll reversal, Open Privacy & Security settings, and the
/// Debug Console's Export…/Clear. Only the padding differs between call
/// sites (see each call site's own comment for the handoff's exact value),
/// so `padding` is the one thing left for the caller to supply.
///
/// egui's `Button` has no per-instance padding knob, so this scopes
/// `ui.spacing_mut().button_padding` for the single `ui.add(...)` call via
/// `ui.scope` (which builds a genuine child `Ui` with its own cloned style,
/// so the override never leaks to sibling widgets) - the same mechanism
/// `device_rule_chip` below uses for its own scoped visuals, just touching
/// `Spacing` instead of `Visuals`.
pub(super) fn styled_button(ui: &mut egui::Ui, label: &str, padding: egui::Vec2) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let bg = control_surface_color(dark);
    let border = control_border_color(dark);
    let text_color = primary_text_color(dark);

    ui.scope(|ui| {
        ui.spacing_mut().button_padding = padding;
        ui.add(
            egui::Button::new(RichText::new(label).size(12.0).color(text_color))
                .fill(bg)
                .stroke(egui::Stroke::new(1.0, border))
                .corner_radius(6.0),
        )
    })
    .inner
}

/// The per-device Default/Reverse/Don't-reverse picker (handoff "1b"),
/// rendered as a bordered pill/chip rather than a plain `ComboBox`. Still
/// backed by a real `egui::ComboBox` for the popup mechanics (open/close,
/// keyboard nav, `selectable_value`) - only the CLOSED button's appearance
/// is restyled, via three stock `ComboBox` extension points, rather than a
/// hand-rolled clickable-chip-plus-popup:
/// - `ui.scope` + `ui.visuals_mut().widgets.{inactive,hovered,active,open}`
///   to recolor the chip's own fill/border/corner-radius;
/// - `.selected_text(RichText::new(...).color(...))` to color the text
///   independent of those visuals (a `RichText`'s own explicit color wins
///   over a widget-visuals fallback color - see `Painter::galley`'s doc
///   comment on `fallback_color`);
/// - `.icon(...)` to paint the "▾" glyph in its own color, since the
///   handoff's "Default" state uses a DIFFERENT color for the arrow
///   (`#9A9AA0`) than for the text (`#1D1D1F`) - a single overridden
///   `fg_stroke` could not represent that, since `ComboBox` uses it for
///   both the text and (by default) the arrow.
///
/// This was chosen over a fully custom chip with a hand-rolled popup
/// because all three hooks above are already stock, documented `ComboBox`
/// API - reusing its real popup mechanics is less code and less risk than
/// reimplementing click-to-open/selection/keyboard-nav for a three-item
/// menu from scratch.
///
/// Colors: "Default" and "Reverse" match the handoff's id="1b" Devices tab
/// exactly in light mode (dark mode's "Reverse" background is an
/// extrapolation - see `reverse_chip_dark_bg`'s doc comment). "Don't
/// reverse" has NO mockup example in either theme anywhere in the handoff -
/// it renders identically to "Default" (neutral chip) here rather than
/// inventing an unspecified third accent color the design never showed.
///
/// Preserves the exact `Option<bool>` semantics the caller already relies
/// on (`None` = Default, `Some(true)` = Reverse, `Some(false)` = Don't
/// reverse): this function only paints and edits `*selection` via the
/// popup's `selectable_value` calls, identically to the `egui::ComboBox` it
/// replaces. `device_rules` still does its own `retain`/`push` afterward
/// based on whether `*selection` differs from the value it had going in -
/// unchanged by this function.
pub(super) fn device_rule_chip(
    ui: &mut egui::Ui,
    id_salt: impl egui::AsIdSalt,
    selection: &mut Option<bool>,
) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let is_reverse = *selection == Some(true);

    let bg = if is_reverse {
        if dark {
            reverse_chip_dark_bg()
        } else {
            Color32::from_rgb(0xEE, 0xF3, 0xFE)
        }
    } else {
        control_surface_color(dark)
    };
    let border = if is_reverse {
        accent_color(dark)
    } else {
        control_border_color(dark)
    };
    let text_color = if is_reverse {
        accent_color(dark)
    } else {
        primary_text_color(dark)
    };
    let arrow_color = if is_reverse {
        accent_color(dark)
    } else {
        muted_glyph_color()
    };

    let label_text = match *selection {
        None => "Default",
        Some(true) => "Reverse",
        Some(false) => "Don't reverse",
    };

    ui.scope(|ui| {
        ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0); // handoff "padding:4px 8px"
        ui.spacing_mut().icon_spacing = 6.0; // handoff "gap:6px"

        {
            // The mockup only depicts the resting state, but a fixed look
            // across inactive/hovered/active/open (a prior pass) is a real
            // interaction-feedback regression versus both egui's own
            // default ComboBox and every other control in this window -
            // hovering or pressing the chip must not look identical to
            // resting. Tint the background towards white (dark mode) or
            // black (light mode) by a small amount, more for
            // active/open than hovered, and thicken the border to match.
            let towards = if dark { Color32::WHITE } else { Color32::BLACK };
            let widgets = &mut ui.visuals_mut().widgets;
            widgets.inactive.weak_bg_fill = bg;
            widgets.inactive.bg_stroke = egui::Stroke::new(1.0, border);
            widgets.inactive.corner_radius = 6.0.into();

            widgets.hovered.weak_bg_fill = bg.lerp_to_gamma(towards, 0.06);
            widgets.hovered.bg_stroke = egui::Stroke::new(1.0, border);
            widgets.hovered.corner_radius = 6.0.into();

            for visuals in [&mut widgets.active, &mut widgets.open] {
                visuals.weak_bg_fill = bg.lerp_to_gamma(towards, 0.12);
                visuals.bg_stroke = egui::Stroke::new(1.5, border);
                visuals.corner_radius = 6.0.into();
            }
        }

        egui::ComboBox::from_id_salt(id_salt)
            .selected_text(RichText::new(label_text).size(12.0).color(text_color))
            .icon(
                move |ui: &egui::Ui,
                      rect: egui::Rect,
                      _visuals: &egui::style::WidgetVisuals,
                      _is_open: bool| {
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "▾",
                        // handoff: the arrow <span> has no font-size of its
                        // own, so it inherits the chip's own 12px, not a
                        // smaller glyph.
                        egui::FontId::proportional(12.0),
                        arrow_color,
                    );
                },
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(selection, None, "Default");
                ui.selectable_value(selection, Some(true), "Reverse");
                ui.selectable_value(selection, Some(false), "Don't reverse");
            })
            .response
    })
    .inner
}

pub(super) fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(8.0);
    let color = if ui.visuals().dark_mode {
        Color32::from_rgb(0x7C, 0x7C, 0x82)
    } else {
        Color32::from_rgb(0x8E, 0x8E, 0x93)
    };
    ui.label(
        RichText::new(title.to_uppercase())
            .size(11.0)
            .strong()
            .color(color),
    );
}
