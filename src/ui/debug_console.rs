//! Debug Console viewport and its local presentation state.
//!
//! Keeping this surface separate from `SettingsApp` makes the main UI
//! coordinator responsible only for opening/closing the viewport. Filtering,
//! table rendering and local CSV export stay together in this module.

use eframe::egui::{self, Color32, RichText};
use std::fmt::Write as _;

use crate::config::ConfigStore;
use crate::platform::macos::debug_log;

use super::theme::{status_dot, styled_button};

#[derive(Default)]
pub(super) struct State {
    filter: Filter,
    search: String,
    export_error: Option<String>,
    export_success: Option<String>,
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
                .desired_width(150.0),
        );

        ui.add_space(8.0);
        filter_strip(ui, &mut state.filter);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if styled_button(ui, "Clear", egui::vec2(10.0, 5.0)).clicked() {
                debug_log::clear();
            }
            if styled_button(ui, "Export…", egui::vec2(10.0, 5.0)).clicked() {
                match export_events(&filtered_events(&all_events, state)) {
                    Ok(path) => {
                        state.export_success = Some(format!("Exported to {}", path.display()));
                        state.export_error = None;
                    }
                    Err(error) => {
                        state.export_error = Some(error);
                        state.export_success = None;
                    }
                }
            }
            ui.add_space(6.0);
            status_dot(ui, Color32::from_rgb(0x34, 0xA8, 0x53), 3.0, 8.0);
            ui.label(
                RichText::new("Live")
                    .color(Color32::from_rgb(0x34, 0xA8, 0x53))
                    .strong(),
            );
        });
    });

    if let Some(error) = &state.export_error {
        ui.label(
            RichText::new(format!("Export failed: {error}"))
                .color(Color32::from_rgb(0xC0, 0x39, 0x2B))
                .small(),
        );
    }
    if let Some(success) = &state.export_success {
        ui.label(RichText::new(success).small().weak());
    }

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

fn export_events(events: &[debug_log::DebugEvent]) -> Result<std::path::PathBuf, String> {
    let config_path = ConfigStore::default_path();
    let export_dir = config_path
        .parent()
        .ok_or_else(|| "could not determine config directory".to_string())?
        .join("Debug Logs");
    std::fs::create_dir_all(&export_dir).map_err(|error| error.to_string())?;

    let now_ms = debug_log::now_millis();
    let file_path = export_dir.join(format!("debug-events-{now_ms}.csv"));
    let csv = events_to_csv(events);

    std::fs::write(&file_path, csv).map_err(|error| error.to_string())?;
    Ok(file_path)
}

fn events_to_csv(events: &[debug_log::DebugEvent]) -> String {
    let mut csv = String::from(
        "timestamp_ms,device,device_kind,device_name,vendor_id,product_id,source_pid,synthetic,axis,raw_delta,output_delta,category,reason_code,decision\n",
    );

    for event in events {
        let device_description = event.device_description();
        let decision_text = event.decision_text();
        let device_name = event.device_name.as_deref().unwrap_or_default();
        let (vendor_id, product_id) = event
            .hardware
            .map(|hardware| {
                (
                    format!("0x{:04x}", hardware.vendor_id),
                    format!("0x{:04x}", hardware.product_id),
                )
            })
            .unwrap_or_default();

        writeln!(
            csv,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            event.timestamp_ms,
            csv_escape(&device_description),
            event.device_kind.as_str(),
            csv_escape(device_name),
            vendor_id,
            product_id,
            event.source_pid,
            event.synthetic,
            event.axis.code(),
            event.raw_delta,
            event.output_delta,
            event.category().code(),
            event.reason.code(),
            csv_escape(&decision_text),
        )
        .expect("writing to a String cannot fail");
    }

    csv
}

fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains(['\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{DeviceKind, HardwareId};
    use std::sync::Arc;

    #[test]
    fn csv_escape_quotes_commas_quotes_and_newlines() {
        assert_eq!(csv_escape("plain"), "plain");
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("a\"b"), "\"a\"\"b\"");
        assert_eq!(csv_escape("a\nb"), "\"a\nb\"");
        assert_eq!(csv_escape("a\rb"), "\"a\rb\"");
    }

    #[test]
    fn csv_export_contains_raw_structured_source_and_reason_fields() {
        let event = debug_log::DebugEvent {
            timestamp_ms: 42,
            device_kind: DeviceKind::Mouse,
            device_name: Some(Arc::from("MX, Master\n3S")),
            hardware: Some(HardwareId {
                vendor_id: 0x046d,
                product_id: 0xb034,
            }),
            source_pid: 123,
            synthetic: true,
            axis: debug_log::Axis::Vertical,
            raw_delta: 1,
            output_delta: -1,
            reason: debug_log::DecisionReason::SyntheticEvent,
        };

        let csv = events_to_csv(&[event]);

        assert!(csv.starts_with(
            "timestamp_ms,device,device_kind,device_name,vendor_id,product_id,source_pid,synthetic,axis,raw_delta,output_delta,category,reason_code,decision\n"
        ));
        assert!(csv.contains("Mouse wheel · MX, Master 3S"));
        assert!(csv.contains("\"MX, Master\n3S\""));
        assert!(csv.contains("0x046d,0xb034,123,true,vertical,1,-1,ignored,synthetic_event"));
    }
}
