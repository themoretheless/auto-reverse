//! Interactive ScrollTest-style benchmark viewport.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Color32, RichText};

use crate::platform::macos::save_panel;
use crate::scroll_benchmark::{
    BenchmarkCase, BenchmarkMatrix, BenchmarkTrial, PhysicalDeviceClass, TargetMode, TrialResult,
};

use super::local_export;
use super::theme::{accent_color, control_border_color, styled_button};

const LINE_DELTA_POINTS: f64 = 40.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum MatrixPreset {
    #[default]
    Compact,
    Full,
}

impl MatrixPreset {
    fn matrix(self) -> BenchmarkMatrix {
        match self {
            Self::Compact => BenchmarkMatrix::compact(),
            Self::Full => BenchmarkMatrix::full(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Phase {
    #[default]
    Setup,
    Ready,
    Running,
    Review,
    Results,
}

pub(super) struct State {
    physical_device: PhysicalDeviceClass,
    target_mode: TargetMode,
    preset: MatrixPreset,
    phase: Phase,
    cases: Vec<BenchmarkCase>,
    current_case: usize,
    trial: Option<BenchmarkTrial>,
    latest_result: Option<TrialResult>,
    results: Vec<TrialResult>,
    error: Option<String>,
    last_export: Option<PathBuf>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            physical_device: PhysicalDeviceClass::default(),
            target_mode: TargetMode::Known,
            preset: MatrixPreset::Compact,
            phase: Phase::Setup,
            cases: Vec::new(),
            current_case: 0,
            trial: None,
            latest_result: None,
            results: Vec::new(),
            error: None,
            last_export: None,
        }
    }
}

pub(super) fn show_viewport(ctx: &egui::Context, state: &mut State) -> bool {
    let viewport_id = egui::ViewportId::from_hash_of("auto-reverse-scroll-benchmark");
    let builder = egui::ViewportBuilder::default()
        .with_title("Scroll Benchmark - Auto Reverse")
        .with_inner_size([720.0, 700.0])
        .with_min_inner_size([560.0, 680.0]);
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
    ui.spacing_mut().item_spacing.y = 10.0;
    header(ui, state);
    ui.separator();

    if let Some(error) = &state.error {
        ui.label(
            RichText::new(error)
                .small()
                .color(Color32::from_rgb(0xC0, 0x39, 0x2B)),
        );
    }

    match state.phase {
        Phase::Setup => setup(ui, state),
        Phase::Ready | Phase::Running | Phase::Review => trial_surface(ui, state),
        Phase::Results => results(ui, state),
    }
}

fn header(ui: &mut egui::Ui, state: &mut State) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Scroll benchmark").size(16.0).strong());
        if !matches!(state.phase, Phase::Setup | Phase::Results) && !state.cases.is_empty() {
            ui.label(
                RichText::new(format!(
                    "Trial {} of {}",
                    state.current_case + 1,
                    state.cases.len()
                ))
                .small()
                .weak(),
            );
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if state.phase != Phase::Setup
                && styled_button(ui, "End session", egui::vec2(9.0, 4.0)).clicked()
            {
                state.phase = Phase::Setup;
                state.trial = None;
                state.latest_result = None;
            }
        });
    });
}

fn setup(ui: &mut egui::Ui, state: &mut State) {
    ui.add_space(8.0);
    ui.label(RichText::new("Input device").small().strong().weak());
    egui::ComboBox::from_id_salt("benchmark-physical-device")
        .selected_text(state.physical_device.label())
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for device in PhysicalDeviceClass::ALL {
                ui.selectable_value(&mut state.physical_device, device, device.label());
            }
        });

    ui.add_space(8.0);
    ui.label(RichText::new("Target condition").small().strong().weak());
    segmented_two(
        ui,
        &mut state.target_mode,
        (TargetMode::Known, "Known target"),
        (TargetMode::Unknown, "Unknown target"),
    );

    ui.add_space(8.0);
    ui.label(RichText::new("Trial matrix").small().strong().weak());
    segmented_two(
        ui,
        &mut state.preset,
        (MatrixPreset::Compact, "Compact - 12"),
        (MatrixPreset::Full, "Full - 36"),
    );

    let matrix = state.preset.matrix();
    let distances = unique_case_values(matrix.cases(), |case| case.distance_points);
    let viewports = unique_case_values(matrix.cases(), |case| case.viewport_height_points);
    let tolerances = unique_case_values(matrix.cases(), |case| case.tolerance_points);
    ui.add_space(8.0);
    egui::Grid::new("benchmark-matrix-summary")
        .num_columns(2)
        .spacing([18.0, 8.0])
        .show(ui, |ui| {
            ui.label(RichText::new("Distances").weak());
            ui.label(format_values(&distances));
            ui.end_row();
            ui.label(RichText::new("Viewport heights").weak());
            ui.label(format_values(&viewports));
            ui.end_row();
            ui.label(RichText::new("Target tolerances").weak());
            ui.label(format_values(&tolerances));
            ui.end_row();
        });

    ui.add_space(16.0);
    if styled_button(ui, "Start session", egui::vec2(14.0, 6.0)).clicked() {
        state.cases = matrix.cases().to_vec();
        state.current_case = 0;
        state.trial = None;
        state.latest_result = None;
        state.results.clear();
        state.error = None;
        state.phase = Phase::Ready;
    }
}

fn trial_surface(ui: &mut egui::Ui, state: &mut State) {
    let Some(case) = state.cases.get(state.current_case).copied() else {
        state.error = Some("The benchmark matrix has no current trial.".to_string());
        state.phase = Phase::Setup;
        return;
    };

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(state.physical_device.label())
                .small()
                .strong(),
        );
        ui.label(
            RichText::new(match state.target_mode {
                TargetMode::Known => format!("Target at {} pt", case.distance_points),
                TargetMode::Unknown => "Find TARGET".to_string(),
            })
            .strong(),
        );
        ui.label(
            RichText::new(format!(
                "viewport {} pt  |  tolerance +/-{} pt",
                case.viewport_height_points, case.tolerance_points
            ))
            .small()
            .weak(),
        );
    });

    let stage_height = case.viewport_height_points as f32;
    let (stage_rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), stage_height),
        egui::Sense::hover(),
    );
    paint_stage(
        ui,
        stage_rect,
        case,
        state.target_mode,
        state.trial.as_ref(),
    );

    let now_us = ui_time_us(ui);
    if state.phase == Phase::Running {
        if response.hovered() {
            let deltas = raw_document_deltas(ui, case);
            if let Some(trial) = &mut state.trial {
                for delta in deltas {
                    if let Err(error) = trial.apply_delta(now_us, delta) {
                        state.error = Some(error.to_string());
                        state.phase = Phase::Review;
                        break;
                    }
                }
                match trial.finish_if_settled(now_us) {
                    Ok(Some(result)) => {
                        state.latest_result = Some(result);
                        state.results.push(result);
                        state.phase = Phase::Review;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        state.error = Some(error.to_string());
                        state.phase = Phase::Review;
                    }
                }
            }
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(16));
    }

    ui.add_space(2.0);
    match state.phase {
        Phase::Ready => {
            if styled_button(ui, "Start trial", egui::vec2(14.0, 6.0)).clicked() {
                match BenchmarkTrial::new(state.physical_device, state.target_mode, case, now_us) {
                    Ok(trial) => {
                        state.trial = Some(trial);
                        state.latest_result = None;
                        state.error = None;
                        state.phase = Phase::Running;
                    }
                    Err(error) => state.error = Some(error.to_string()),
                }
            }
        }
        Phase::Running => {
            let (status, color) = match state.trial.as_ref() {
                Some(trial) if trial.target_is_in_tolerance() => {
                    ("Hold still", Color32::from_rgb(0x34, 0xA8, 0x53))
                }
                Some(_) if response.hovered() => ("Scrolling", ui.visuals().text_color()),
                Some(_) => (
                    "Move pointer into the test area",
                    Color32::from_rgb(0xE5, 0x9E, 0x2F),
                ),
                None => ("Trial unavailable", Color32::from_rgb(0xC0, 0x39, 0x2B)),
            };
            ui.label(RichText::new(status).color(color).strong());
        }
        Phase::Review => review_controls(ui, state),
        Phase::Setup | Phase::Results => {}
    }
}

fn review_controls(ui: &mut egui::Ui, state: &mut State) {
    if let Some(result) = state.latest_result {
        ui.horizontal(|ui| {
            metric(
                ui,
                "Time",
                format!("{:.0} ms", result.movement_time_us as f64 / 1_000.0),
            );
            metric(ui, "Switchbacks", result.switchback_count.to_string());
            metric(
                ui,
                "Max overshoot",
                format!("{:.1} pt", result.maximum_overshoot_points),
            );
        });
    }

    let last_case = state.current_case + 1 >= state.cases.len();
    let label = if last_case {
        "View results"
    } else {
        "Next trial"
    };
    if styled_button(ui, label, egui::vec2(14.0, 6.0)).clicked() {
        state.trial = None;
        state.latest_result = None;
        if last_case {
            state.phase = Phase::Results;
        } else {
            state.current_case += 1;
            state.phase = Phase::Ready;
        }
    }
}

fn results(ui: &mut egui::Ui, state: &mut State) {
    if state.results.is_empty() {
        ui.label(RichText::new("No completed trials").weak());
        return;
    }

    let count = state.results.len() as f64;
    let mean_time_ms = state
        .results
        .iter()
        .map(|result| result.movement_time_us as f64 / 1_000.0)
        .sum::<f64>()
        / count;
    let mean_switchbacks = state
        .results
        .iter()
        .map(|result| result.switchback_count as f64)
        .sum::<f64>()
        / count;
    let maximum_overshoot = state
        .results
        .iter()
        .map(|result| result.maximum_overshoot_points)
        .fold(0.0_f64, f64::max);

    ui.label(
        RichText::new(format!("Input: {}", state.physical_device.label()))
            .small()
            .weak(),
    );
    ui.horizontal(|ui| {
        metric(ui, "Mean time", format!("{mean_time_ms:.0} ms"));
        metric(ui, "Mean switchbacks", format!("{mean_switchbacks:.2}"));
        metric(
            ui,
            "Largest overshoot",
            format!("{maximum_overshoot:.1} pt"),
        );
    });
    ui.separator();

    table_header(ui);
    egui::ScrollArea::vertical()
        .max_height((ui.available_height() - 90.0).max(140.0))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for result in &state.results {
                table_row(ui, result);
            }
        });
    ui.separator();

    ui.horizontal(|ui| {
        if styled_button(ui, "Export CSV...", egui::vec2(10.0, 5.0)).clicked() {
            match export_results(&state.results, state.last_export.as_ref()) {
                Ok(Some(path)) => {
                    state.last_export = Some(path);
                    state.error = None;
                }
                Ok(None) => {}
                Err(error) => state.error = Some(format!("Export failed: {error}")),
            }
        }
        if state.last_export.is_some()
            && styled_button(ui, "Reveal in Finder", egui::vec2(10.0, 5.0)).clicked()
            && let Some(path) = &state.last_export
            && let Err(error) = save_panel::reveal_in_finder(path)
        {
            state.error = Some(format!("Reveal failed: {error}"));
        }
    });
}

fn paint_stage(
    ui: &egui::Ui,
    rect: egui::Rect,
    case: BenchmarkCase,
    mode: TargetMode,
    trial: Option<&BenchmarkTrial>,
) {
    let dark = ui.visuals().dark_mode;
    let background = if dark {
        Color32::from_rgb(0x1F, 0x1F, 0x21)
    } else {
        Color32::from_rgb(0xF6, 0xF6, 0xF8)
    };
    let row_color = if dark {
        Color32::from_rgb(0x48, 0x48, 0x4A)
    } else {
        Color32::from_rgb(0xD4, 0xD4, 0xD8)
    };
    let target_color = accent_color(dark);
    let position = trial.map_or(0.0, BenchmarkTrial::position_points);
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 6.0, background);
    painter.rect_stroke(
        rect,
        6.0,
        egui::Stroke::new(1.0, control_border_color(dark)),
        egui::StrokeKind::Inside,
    );

    let band_height = (case.tolerance_points.saturating_mul(2)) as f32;
    let band = egui::Rect::from_center_size(
        rect.center(),
        egui::vec2((rect.width() - 24.0).max(40.0), band_height),
    );
    let band_fill = if dark {
        Color32::from_rgba_unmultiplied(0x34, 0xA8, 0x53, 44)
    } else {
        Color32::from_rgba_unmultiplied(0x34, 0xA8, 0x53, 30)
    };
    painter.rect_filled(band, 4.0, band_fill);
    painter.rect_stroke(
        band,
        4.0,
        egui::Stroke::new(1.0, Color32::from_rgb(0x34, 0xA8, 0x53)),
        egui::StrokeKind::Inside,
    );

    let half_height = f64::from(case.viewport_height_points) / 2.0;
    let first_row = ((position - half_height) / 80.0).floor() as i64 - 1;
    let last_row = ((position + half_height) / 80.0).ceil() as i64 + 1;
    for row in first_row.max(0)..=last_row.max(0) {
        let document_y = row as f64 * 80.0;
        let y = rect.center().y + (document_y - position) as f32;
        if rect.contains(egui::pos2(rect.center().x, y)) {
            painter.line_segment(
                [
                    egui::pos2(rect.left() + 28.0, y),
                    egui::pos2(rect.right() - 28.0, y),
                ],
                egui::Stroke::new(1.0, row_color),
            );
            if mode == TargetMode::Known {
                painter.text(
                    egui::pos2(rect.left() + 14.0, y),
                    egui::Align2::LEFT_CENTER,
                    row.to_string(),
                    egui::FontId::proportional(10.0),
                    row_color,
                );
            }
        }
    }

    let target_y = rect.center().y + (f64::from(case.distance_points) - position) as f32;
    if target_y >= rect.top() - 2.0 && target_y <= rect.bottom() + 2.0 {
        painter.line_segment(
            [
                egui::pos2(rect.left() + 18.0, target_y),
                egui::pos2(rect.right() - 18.0, target_y),
            ],
            egui::Stroke::new(3.0, target_color),
        );
        painter.circle_filled(
            egui::pos2(rect.center().x - 28.0, target_y),
            5.0,
            target_color,
        );
        painter.circle_filled(egui::pos2(rect.center().x, target_y), 5.0, target_color);
        painter.circle_filled(
            egui::pos2(rect.center().x + 28.0, target_y),
            5.0,
            target_color,
        );
        painter.text(
            egui::pos2(rect.right() - 24.0, target_y - 8.0),
            egui::Align2::RIGHT_BOTTOM,
            "TARGET",
            egui::FontId::proportional(11.0),
            target_color,
        );
    }
}

fn raw_document_deltas(ui: &egui::Ui, case: BenchmarkCase) -> Vec<f64> {
    ui.input(|input| {
        input
            .raw
            .events
            .iter()
            .filter_map(|event| match event {
                egui::Event::MouseWheel { unit, delta, .. } => {
                    let content_delta = match unit {
                        egui::MouseWheelUnit::Point => f64::from(delta.y),
                        egui::MouseWheelUnit::Line => f64::from(delta.y) * LINE_DELTA_POINTS,
                        egui::MouseWheelUnit::Page => {
                            f64::from(delta.y) * f64::from(case.viewport_height_points)
                        }
                    };
                    Some(-content_delta)
                }
                _ => None,
            })
            .collect()
    })
}

fn ui_time_us(ui: &egui::Ui) -> u64 {
    let seconds = ui.input(|input| input.time.max(0.0));
    let micros = seconds * 1_000_000.0;
    if micros >= u64::MAX as f64 {
        u64::MAX
    } else {
        micros as u64
    }
}

fn segmented_two<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    selected: &mut T,
    first: (T, &str),
    second: (T, &str),
) {
    let dark = ui.visuals().dark_mode;
    let track = if dark {
        Color32::from_rgb(0x2C, 0x2C, 0x2E)
    } else {
        Color32::from_rgb(0xE3, 0xE3, 0xE7)
    };
    let active = if dark {
        Color32::from_rgb(0x48, 0x48, 0x4A)
    } else {
        Color32::WHITE
    };
    egui::Frame::new()
        .fill(track)
        .corner_radius(6.0)
        .inner_margin(2.0)
        .show(ui, |ui| {
            let width = (ui.available_width() - 2.0) / 2.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (value, label) in [first, second] {
                    let button = egui::Button::new(RichText::new(label).size(12.0).strong())
                        .fill(if *selected == value {
                            active
                        } else {
                            Color32::TRANSPARENT
                        })
                        .corner_radius(5.0)
                        .min_size(egui::vec2(width, 24.0));
                    if ui.add(button).clicked() {
                        *selected = value;
                    }
                }
            });
        });
}

fn metric(ui: &mut egui::Ui, label: &str, value: String) {
    ui.vertical(|ui| {
        ui.label(RichText::new(label).small().weak());
        ui.label(RichText::new(value).strong());
    });
    ui.add_space(22.0);
}

fn table_header(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        table_cell(ui, 80.0, RichText::new("Distance").small().strong().weak());
        table_cell(ui, 80.0, RichText::new("Viewport").small().strong().weak());
        table_cell(ui, 80.0, RichText::new("Tolerance").small().strong().weak());
        table_cell(ui, 90.0, RichText::new("Time").small().strong().weak());
        table_cell(
            ui,
            90.0,
            RichText::new("Switchbacks").small().strong().weak(),
        );
        table_cell(
            ui,
            ui.available_width(),
            RichText::new("Overshoot").small().strong().weak(),
        );
    });
}

fn table_row(ui: &mut egui::Ui, result: &TrialResult) {
    ui.horizontal(|ui| {
        table_cell(ui, 80.0, format!("{} pt", result.case.distance_points));
        table_cell(
            ui,
            80.0,
            format!("{} pt", result.case.viewport_height_points),
        );
        table_cell(ui, 80.0, format!("+/-{} pt", result.case.tolerance_points));
        table_cell(
            ui,
            90.0,
            format!("{:.0} ms", result.movement_time_us as f64 / 1_000.0),
        );
        table_cell(ui, 90.0, result.switchback_count.to_string());
        table_cell(
            ui,
            ui.available_width(),
            format!("{:.1} pt", result.maximum_overshoot_points),
        );
    });
}

fn table_cell(ui: &mut egui::Ui, width: f32, text: impl Into<egui::WidgetText>) {
    ui.add_sized([width.max(24.0), 19.0], egui::Label::new(text).truncate());
}

fn unique_case_values(cases: &[BenchmarkCase], value: impl Fn(BenchmarkCase) -> u32) -> Vec<u32> {
    let mut values = cases.iter().copied().map(value).collect::<Vec<_>>();
    values.sort_unstable();
    values.dedup();
    values
}

fn format_values(values: &[u32]) -> String {
    values
        .iter()
        .map(|value| format!("{value} pt"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn export_results(
    results: &[TrialResult],
    previous: Option<&PathBuf>,
) -> Result<Option<PathBuf>, String> {
    let initial_directory = previous
        .and_then(|path| path.parent())
        .filter(|path| path.is_dir());
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let Some(path) =
        save_panel::choose_csv_path(&format!("scroll-benchmark-{now_ms}.csv"), initial_directory)?
    else {
        return Ok(None);
    };

    let csv = results_csv(results);
    local_export::write_atomically(&path, &csv)?;
    Ok(Some(path))
}

fn results_csv(results: &[TrialResult]) -> String {
    let mut csv = String::from(
        "physical_device,target_mode,transfer,distance_points,viewport_height_points,tolerance_points,movement_time_ms,switchbacks,maximum_overshoot_points,event_count\n",
    );
    for result in results {
        writeln!(
            csv,
            "{},{},{},{},{},{},{:.3},{},{:.3},{}",
            result.physical_device.as_str(),
            result.target_mode.as_str(),
            result.transfer.as_str(),
            result.case.distance_points,
            result.case.viewport_height_points,
            result.case.tolerance_points,
            result.movement_time_us as f64 / 1_000.0,
            result.switchback_count,
            result.maximum_overshoot_points,
            result.event_count,
        )
        .expect("writing to a String cannot fail");
    }
    csv
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scroll_benchmark::BenchmarkTransfer;

    #[test]
    fn setup_value_lists_are_sorted_and_deduplicated() {
        let cases = [
            BenchmarkCase {
                distance_points: 960,
                viewport_height_points: 240,
                tolerance_points: 12,
            },
            BenchmarkCase {
                distance_points: 240,
                viewport_height_points: 360,
                tolerance_points: 12,
            },
        ];
        assert_eq!(
            unique_case_values(&cases, |case| case.distance_points),
            vec![240, 960]
        );
        assert_eq!(format_values(&[12, 32]), "12 pt, 32 pt");
    }

    #[test]
    fn result_csv_keeps_condition_and_scrolltest_metrics() {
        let csv = results_csv(&[TrialResult {
            physical_device: PhysicalDeviceClass::MagicMouse,
            target_mode: TargetMode::Unknown,
            transfer: BenchmarkTransfer::Baseline,
            case: BenchmarkCase {
                distance_points: 960,
                viewport_height_points: 360,
                tolerance_points: 20,
            },
            movement_time_us: 1_250_000,
            switchback_count: 2,
            maximum_overshoot_points: 14.5,
            event_count: 9,
        }]);

        assert!(csv.starts_with("physical_device,target_mode,transfer,distance_points"));
        assert!(csv.contains("magic_mouse,unknown,baseline,960,360,20,1250.000,2,14.500,9"));
    }
}
