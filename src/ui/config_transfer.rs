//! Settings export/import presentation.
//!
//! The config layer owns parsing, migration, security and diff semantics.
//! This module only coordinates native panels and renders the pending review.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, RichText};

use crate::config::{self, AppConfig, ConfigImportPreview};
use crate::platform::macos::save_panel;

use super::local_export;
use super::theme::styled_button;

#[derive(Default)]
pub(super) struct State {
    pending: Option<PendingImport>,
    error: Option<String>,
    notice: Option<String>,
    last_directory: Option<PathBuf>,
}

struct PendingImport {
    path: PathBuf,
    preview: ConfigImportPreview,
}

pub(super) struct ApplyRequest {
    pub(super) config: AppConfig,
    pub(super) section_labels: String,
}

impl State {
    pub(super) fn finish_apply(&mut self, section_labels: String, result: Result<(), String>) {
        match result {
            Ok(()) => {
                self.notice = Some(format!("Imported {section_labels}"));
                self.error = None;
            }
            Err(error) => {
                self.error = Some(format!("Import was not applied: {error}"));
                self.notice = None;
            }
        }
    }
}

pub(super) fn controls(
    ui: &mut egui::Ui,
    state: &mut State,
    current: &AppConfig,
    config_path: &Path,
) -> Option<ApplyRequest> {
    ui.horizontal_wrapped(|ui| {
        if styled_button(ui, "Export config", egui::vec2(12.0, 5.0)).clicked() {
            run_export(state, current, config_path);
        }
        if styled_button(ui, "Import config", egui::vec2(12.0, 5.0)).clicked() {
            run_import(state, current, config_path);
        }
    });

    if let Some(error) = &state.error {
        ui.label(
            RichText::new(error)
                .small()
                .color(Color32::from_rgb(0xC0, 0x39, 0x2B)),
        );
    }
    if let Some(notice) = &state.notice {
        ui.label(
            RichText::new(notice)
                .small()
                .color(Color32::from_rgb(0x34, 0xA8, 0x53)),
        );
    }

    let mut cancel = false;
    let mut apply = false;
    if let Some(pending) = &mut state.pending {
        pending.preview.rebase(current);
        ui.add_space(4.0);
        ui.separator();
        ui.label(
            RichText::new(format!("Review {}", display_filename(&pending.path)))
                .small()
                .strong(),
        );

        if pending.preview.migration.migrated() {
            ui.label(
                RichText::new(format!(
                    "Schema {} -> {}",
                    pending.preview.migration.source_version,
                    pending.preview.migration.target_version
                ))
                .small()
                .color(Color32::from_rgb(0xE5, 0x9E, 0x2F)),
            );
            for action in &pending.preview.migration.actions {
                ui.label(RichText::new(action).small().weak());
            }
        } else {
            ui.label(RichText::new("Schema 1 · no migration").small().weak());
        }

        if pending.preview.has_changes() {
            for change in &pending.preview.changes {
                ui.add_space(3.0);
                ui.label(RichText::new(change.section.label()).small().strong());
                for detail in &change.details {
                    ui.label(RichText::new(detail).small().weak());
                }
            }
            ui.horizontal_wrapped(|ui| {
                if styled_button(ui, "Cancel", egui::vec2(12.0, 5.0)).clicked() {
                    cancel = true;
                }
                if styled_button(ui, "Apply changed sections", egui::vec2(12.0, 5.0)).clicked() {
                    apply = true;
                }
            });
        } else {
            ui.label(
                RichText::new("No settings would change.")
                    .small()
                    .color(Color32::from_rgb(0x34, 0xA8, 0x53)),
            );
            if styled_button(ui, "Close review", egui::vec2(12.0, 5.0)).clicked() {
                cancel = true;
            }
        }
    }

    if cancel {
        state.pending = None;
        return None;
    }
    if apply {
        let pending = state
            .pending
            .take()
            .expect("an apply action is only rendered for a pending import");
        return Some(ApplyRequest {
            config: pending.preview.apply_changed_sections(current),
            section_labels: pending.preview.changed_section_labels(),
        });
    }
    None
}

fn run_export(state: &mut State, config: &AppConfig, config_path: &Path) {
    let initial_directory = preferred_directory(state, config_path);
    let result: Result<Option<PathBuf>, String> = (|| {
        let Some(path) = save_panel::choose_toml_path(
            "auto-reverse-config-v1.toml",
            initial_directory.as_deref(),
        )?
        else {
            return Ok(None);
        };
        let document = config::export_document(config).map_err(|error| error.to_string())?;
        local_export::write_atomically(&path, &document)?;
        Ok(Some(path))
    })();

    match result {
        Ok(Some(path)) => {
            state.last_directory = path.parent().map(Path::to_path_buf);
            state.notice = Some(format!("Exported {}", display_filename(&path)));
            state.error = None;
        }
        Ok(None) => {}
        Err(error) => {
            state.error = Some(format!("Export failed: {error}"));
            state.notice = None;
        }
    }
}

fn run_import(state: &mut State, current: &AppConfig, config_path: &Path) {
    let initial_directory = preferred_directory(state, config_path);
    let result: Result<Option<PendingImport>, String> = (|| {
        let Some(path) = save_panel::choose_toml_import_path(initial_directory.as_deref())? else {
            return Ok(None);
        };
        let preview =
            config::preview_import_file(&path, current).map_err(|error| error.to_string())?;
        Ok(Some(PendingImport { path, preview }))
    })();

    match result {
        Ok(Some(pending)) => {
            state.last_directory = pending.path.parent().map(Path::to_path_buf);
            state.pending = Some(pending);
            state.error = None;
            state.notice = None;
        }
        Ok(None) => {}
        Err(error) => {
            state.pending = None;
            state.error = Some(format!("Import failed: {error}"));
            state.notice = None;
        }
    }
}

fn preferred_directory(state: &State, config_path: &Path) -> Option<PathBuf> {
    state
        .last_directory
        .as_ref()
        .filter(|path| path.is_dir())
        .cloned()
        .or_else(|| {
            config_path
                .parent()
                .filter(|path| path.is_dir())
                .map(Path::to_path_buf)
        })
}

fn display_filename(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .chars()
        .map(|character| {
            if character.is_whitespace() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_filename_never_injects_layout_whitespace() {
        assert_eq!(
            display_filename(Path::new("/tmp/private\nconfig.toml")),
            "private config.toml"
        );
    }
}
