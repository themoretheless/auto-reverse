//! Debug Console viewport and its local presentation state.
//!
//! Keeping this surface separate from `SettingsApp` makes the main UI
//! coordinator responsible only for opening/closing the viewport. Filtering
//! and table rendering stay here; `export` owns CSV/trace file workflows.

use eframe::egui::{self, Color32, RichText};

use crate::event_rate::{
    DeviceEventRate, EventRateSample, analyze_event_rates, millihertz_to_hertz,
};
use crate::platform::macos::{debug_log, tap_metrics};

use super::theme::{status_dot, styled_button};

mod export;

#[derive(Default)]
pub(super) struct State {
    filter: Filter,
    search: String,
    export_error: Option<String>,
    reveal_error: Option<String>,
    last_export: Option<export::Receipt>,
    benchmark_requested: bool,
    tap_latency: Option<tap_metrics::TapLatencySnapshot>,
    tap_latency_error: Option<String>,
}

impl State {
    pub(super) fn take_benchmark_request(&mut self) -> bool {
        std::mem::take(&mut self.benchmark_requested)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Filter {
    #[default]
    All,
    Reversed,
    Passed,
    Ignored,
}

/// Renders the Debug Console as a second native viewport. Returns `true`
/// when that viewport asked to close so the caller can update app state.
pub(super) fn show_viewport(ctx: &egui::Context, state: &mut State) -> bool {
    let viewport_id = egui::ViewportId::from_hash_of("auto-reverse-debug-console");
    let builder = egui::ViewportBuilder::default()
        .with_title("Debug Console — Auto Reverse")
        .with_inner_size([640.0, 480.0])
        .with_min_inner_size([480.0, 320.0]);
    let mut close_requested = false;

    ctx.show_viewport_immediate(viewport_id, builder, |ctx, _class| {
        if ctx.input(|input| input.viewport().close_requested()) {
            close_requested = true;
        }

        egui::CentralPanel::default().show(ctx, |ui| contents(ui, state));
    });

    close_requested
}

fn contents(ui: &mut egui::Ui, state: &mut State) {
    let all_events = debug_log::snapshot();

    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut state.search)
                .hint_text("Filter events…")
                .desired_width(120.0),
        );

        ui.add_space(8.0);
        filter_strip(ui, &mut state.filter);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            status_dot(ui, Color32::from_rgb(0x34, 0xA8, 0x53), 3.0, 8.0);
            ui.label(
                RichText::new("Live")
                    .color(Color32::from_rgb(0x34, 0xA8, 0x53))
                    .strong(),
            );
        });
    });
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if styled_button(ui, "Clear", egui::vec2(10.0, 5.0)).clicked() {
                debug_log::clear();
            }
            export_menu(ui, &filtered_events(&all_events, state), state);
            if styled_button(ui, "Benchmark...", egui::vec2(10.0, 5.0)).clicked() {
                state.benchmark_requested = true;
            }
        });
    });

    if let Some(error) = &state.export_error {
        ui.label(
            RichText::new(format!("Export failed: {error}"))
                .color(Color32::from_rgb(0xC0, 0x39, 0x2B))
                .small(),
        );
    }
    if let Some(error) = &state.reveal_error {
        ui.label(
            RichText::new(format!("Reveal failed: {error}"))
                .color(Color32::from_rgb(0xC0, 0x39, 0x2B))
                .small(),
        );
    }
    if let Some(receipt) = state.last_export.clone() {
        ui.horizontal(|ui| {
            let summary_width = (ui.available_width() - 120.0).max(80.0);
            ui.add_sized(
                [summary_width, 22.0],
                egui::Label::new(RichText::new(receipt.summary()).small().weak()).truncate(),
            )
            .on_hover_text(receipt.path().display().to_string());
            if styled_button(ui, "Reveal in Finder", egui::vec2(8.0, 3.0)).clicked() {
                match export::reveal(&receipt) {
                    Ok(()) => state.reveal_error = None,
                    Err(error) => state.reveal_error = Some(error),
                }
            }
        });
    }

    diagnostic_metrics(ui, &all_events, state);

    ui.separator();

    let events = filtered_events(&all_events, state);
    table_header(ui);
    ui.separator();

    let table_height = (ui.available_height() - 40.0).max(120.0);
    egui::ScrollArea::vertical()
        .max_height(table_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if events.is_empty() {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Scroll to see live decisions").strong());
                    ui.label(
                        RichText::new("Events stay on this Mac and are never sent anywhere.")
                            .small()
                            .weak(),
                    );
                });
            } else {
                for event in events.iter().rev() {
                    table_row(ui, event);
                }
            }
        });

    ui.separator();
    ui.label(
        RichText::new(format!(
            "{} events shown · ring buffer holds the last {} · stays on this Mac, never sent over the network",
            events.len(),
            debug_log::CAPACITY
        ))
        .small()
        .weak(),
    );
}

fn diagnostic_metrics(ui: &mut egui::Ui, events: &[debug_log::DebugEvent], state: &mut State) {
    egui::CollapsingHeader::new("Observed input metrics")
        .default_open(false)
        .show(ui, |ui| {
            let rate_samples = events
                .iter()
                .map(|event| EventRateSample {
                    timestamp_us: event.monotonic_us,
                    device_kind: event.device_kind,
                })
                .collect::<Vec<_>>();
            let rates = analyze_event_rates(&rate_samples);
            if rates.is_empty() {
                ui.label(
                    RichText::new("Event rate needs at least two timestamps per device type")
                        .small()
                        .weak(),
                );
            } else {
                rate_header(ui);
                for rate in &rates {
                    rate_row(ui, rate);
                }
            }

            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new("Event tap interval latency").small().strong());
                if styled_button(ui, "Sample now", egui::vec2(8.0, 3.0))
                    .on_hover_text("Reading resets CoreGraphics min/max for the next interval")
                    .clicked()
                {
                    match tap_metrics::current_process_scroll_snapshot() {
                        Ok(snapshot) => {
                            state.tap_latency = Some(snapshot);
                            state.tap_latency_error = None;
                        }
                        Err(error) => state.tap_latency_error = Some(error.to_string()),
                    }
                }
            });

            if let Some(error) = &state.tap_latency_error {
                ui.label(
                    RichText::new(error)
                        .small()
                        .color(Color32::from_rgb(0xC0, 0x39, 0x2B)),
                );
            } else if let Some(snapshot) = &state.tap_latency {
                if snapshot.active_scroll_taps.is_empty() {
                    ui.label(
                        RichText::new("No active scroll filter in this process")
                            .small()
                            .weak(),
                    );
                } else {
                    for tap in &snapshot.active_scroll_taps {
                        ui.label(
                            RichText::new(format!(
                                "tap {}: min {:.1} us  avg {:.1} us  max {:.1} us  {}",
                                tap.event_tap_id,
                                tap.minimum_us,
                                tap.average_us,
                                tap.maximum_us,
                                if tap.enabled { "enabled" } else { "disabled" }
                            ))
                            .small()
                            .monospace(),
                        );
                    }
                    if snapshot.possibly_truncated {
                        ui.label(
                            RichText::new("The system tap list may have changed during sampling")
                                .small()
                                .color(Color32::from_rgb(0xE5, 0x9E, 0x2F)),
                        );
                    }
                }
            }
        });
}

fn rate_header(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        cell(ui, 88.0, RichText::new("Device").small().strong().weak());
        cell(ui, 62.0, RichText::new("p50 Hz").small().strong().weak());
        cell(ui, 62.0, RichText::new("p95 Hz").small().strong().weak());
        cell(ui, 62.0, RichText::new("max Hz").small().strong().weak());
        cell(
            ui,
            ui.available_width(),
            RichText::new("bins <30 | 30-60 | 60-120 | 120-240 | 240+")
                .small()
                .strong()
                .weak(),
        );
    });
}

fn rate_row(ui: &mut egui::Ui, rate: &DeviceEventRate) {
    ui.horizontal(|ui| {
        cell(ui, 88.0, rate.device_kind.to_string());
        cell(
            ui,
            62.0,
            format!("{:.1}", millihertz_to_hertz(rate.rates_millihz.p50)),
        );
        cell(
            ui,
            62.0,
            format!("{:.1}", millihertz_to_hertz(rate.rates_millihz.p95)),
        );
        cell(
            ui,
            62.0,
            format!("{:.1}", millihertz_to_hertz(rate.rates_millihz.max)),
        );
        cell(
            ui,
            ui.available_width(),
            format!(
                "{} | {} | {} | {} | {}",
                rate.histogram.below_30_hz,
                rate.histogram.from_30_to_60_hz,
                rate.histogram.from_60_to_120_hz,
                rate.histogram.from_120_to_240_hz,
                rate.histogram.at_least_240_hz,
            ),
        );
    });
}

fn export_menu(ui: &mut egui::Ui, events: &[debug_log::DebugEvent], state: &mut State) {
    ui.menu_button("Export…", |ui| {
        if ui
            .button("Privacy trace…")
            .on_hover_text("Replayable TOML without device identity, process IDs, or wall time")
            .clicked()
        {
            ui.close();
            apply_export_result(export::run_trace(events, state.last_export.as_ref()), state);
        }
        if ui
            .button("Detailed CSV…")
            .on_hover_text("Support export with the visible diagnostic source fields")
            .clicked()
        {
            ui.close();
            apply_export_result(export::run_csv(events, state.last_export.as_ref()), state);
        }
    });
}

fn apply_export_result(result: Result<Option<export::Receipt>, String>, state: &mut State) {
    match result {
        Ok(Some(receipt)) => {
            state.last_export = Some(receipt);
            state.export_error = None;
            state.reveal_error = None;
        }
        Ok(None) => {}
        Err(error) => state.export_error = Some(error),
    }
}

fn filter_strip(ui: &mut egui::Ui, selected: &mut Filter) {
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
        .corner_radius(6.0)
        .inner_margin(2.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (filter, label) in [
                    (Filter::All, "All"),
                    (Filter::Reversed, "Reversed"),
                    (Filter::Passed, "Passed"),
                    (Filter::Ignored, "Ignored"),
                ] {
                    let is_active = *selected == filter;
                    let (color, text_color) = if is_active {
                        (active_color, active_text)
                    } else {
                        (Color32::TRANSPARENT, inactive_text)
                    };
                    let button = egui::Button::new(
                        RichText::new(label).size(11.5).strong().color(text_color),
                    )
                    .fill(color)
                    .corner_radius(5.0)
                    .min_size(egui::vec2(64.0, 22.0));
                    if ui.add(button).clicked() {
                        *selected = filter;
                    }
                }
            });
        });
}

fn table_header(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        cell(ui, 96.0, RichText::new("Time").small().strong().weak());
        cell(ui, 180.0, RichText::new("Device").small().strong().weak());
        cell(ui, 76.0, RichText::new("Axis").small().strong().weak());
        cell(
            ui,
            92.0,
            RichText::new("Δ raw → out").small().strong().weak(),
        );
        cell(
            ui,
            ui.available_width(),
            RichText::new("Decision").small().strong().weak(),
        );
    });
}

fn table_row(ui: &mut egui::Ui, event: &debug_log::DebugEvent) {
    let device_description = event.device_description();
    let decision_text = event.decision_text();

    ui.horizontal(|ui| {
        cell(
            ui,
            96.0,
            RichText::new(format_timestamp(event.timestamp_ms)).monospace(),
        );
        cell(ui, 180.0, device_description.as_str());
        cell(ui, 76.0, event.axis.label());
        cell(
            ui,
            92.0,
            RichText::new(format!("{} → {}", event.raw_delta, event.output_delta)).monospace(),
        );
        let color = match event.category() {
            debug_log::DecisionCategory::Reversed => Color32::from_rgb(0x34, 0xA8, 0x53),
            debug_log::DecisionCategory::Passed => Color32::GRAY,
            debug_log::DecisionCategory::Ignored => Color32::from_rgb(0xE5, 0x9E, 0x2F),
        };
        cell(
            ui,
            ui.available_width(),
            RichText::new(decision_text.as_ref()).color(color),
        );
    });
}

fn cell(ui: &mut egui::Ui, width: f32, text: impl Into<egui::WidgetText>) {
    ui.add_sized([width.max(24.0), 18.0], egui::Label::new(text).truncate());
}

fn filtered_events(
    all_events: &[debug_log::DebugEvent],
    state: &State,
) -> Vec<debug_log::DebugEvent> {
    all_events
        .iter()
        .filter(|event| match state.filter {
            Filter::All => true,
            Filter::Reversed => event.category() == debug_log::DecisionCategory::Reversed,
            Filter::Passed => event.category() == debug_log::DecisionCategory::Passed,
            Filter::Ignored => event.category() == debug_log::DecisionCategory::Ignored,
        })
        .filter(|event| event.matches_search(&state.search))
        .cloned()
        .collect()
}

fn format_timestamp(timestamp_ms: u128) -> String {
    let total_ms = timestamp_ms % (24 * 60 * 60 * 1000);
    let hours = total_ms / (60 * 60 * 1000);
    let minutes = (total_ms / (60 * 1000)) % 60;
    let seconds = (total_ms / 1000) % 60;
    let millis = total_ms % 1000;
    format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
}
