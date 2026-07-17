//! Pure field-level dry-run and reviewed-section application.

use std::fmt;

use super::document::MigrationReport;
use crate::config::{AppConfig, CONFIG_VERSION};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConfigSection {
    General,
    Devices,
    Startup,
    Advanced,
}

impl ConfigSection {
    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Devices => "Devices",
            Self::Startup => "Startup",
            Self::Advanced => "Advanced",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionChange {
    pub section: ConfigSection,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigImportPreview {
    pub candidate: AppConfig,
    pub migration: MigrationReport,
    pub changes: Vec<SectionChange>,
}

impl ConfigImportPreview {
    pub(super) fn new(
        candidate: AppConfig,
        migration: MigrationReport,
        current: &AppConfig,
    ) -> Self {
        Self {
            changes: section_changes(current, &candidate),
            candidate,
            migration,
        }
    }

    pub fn has_changes(&self) -> bool {
        !self.changes.is_empty()
    }

    pub fn changed_section_labels(&self) -> String {
        self.changes
            .iter()
            .map(|change| change.section.label())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Recomputes the review against the latest in-memory settings. This
    /// matters because the review can remain open while another tab autosaves
    /// an edit; an old preview must not claim a section is unchanged.
    pub fn rebase(&mut self, current: &AppConfig) {
        self.changes = section_changes(current, &self.candidate);
    }

    /// Applies only the sections shown in this review. Settings changed after
    /// the file was selected remain intact when their section is absent from
    /// the dry-run diff.
    pub fn apply_changed_sections(&self, current: &AppConfig) -> AppConfig {
        let mut applied = current.clone();
        for change in &self.changes {
            match change.section {
                ConfigSection::General => copy_general(&mut applied, &self.candidate),
                ConfigSection::Devices => {
                    applied
                        .device_rules
                        .clone_from(&self.candidate.device_rules);
                }
                ConfigSection::Startup => {
                    applied.start_at_login = self.candidate.start_at_login;
                }
                ConfigSection::Advanced => copy_advanced(&mut applied, &self.candidate),
            }
        }
        applied.config_version = CONFIG_VERSION;
        applied
    }
}

fn section_changes(current: &AppConfig, candidate: &AppConfig) -> Vec<SectionChange> {
    let mut changes = Vec::new();

    let mut general = Vec::new();
    bool_change(&mut general, "Enabled", current.enabled, candidate.enabled);
    bool_change(
        &mut general,
        "Vertical direction",
        current.reverse_vertical,
        candidate.reverse_vertical,
    );
    bool_change(
        &mut general,
        "Horizontal direction",
        current.reverse_horizontal,
        candidate.reverse_horizontal,
    );
    bool_change(
        &mut general,
        "Mouse wheel",
        current.reverse_mouse,
        candidate.reverse_mouse,
    );
    bool_change(
        &mut general,
        "Trackpad",
        current.reverse_trackpad,
        candidate.reverse_trackpad,
    );
    bool_change(
        &mut general,
        "Magic Mouse",
        current.reverse_magic_mouse,
        candidate.reverse_magic_mouse,
    );
    bool_change(
        &mut general,
        "Unknown devices",
        current.reverse_unknown,
        candidate.reverse_unknown,
    );
    value_change(
        &mut general,
        "Wheel step",
        current.discrete_scroll_step_size,
        candidate.discrete_scroll_step_size,
    );
    value_change(
        &mut general,
        "Smooth preset",
        current.smooth_preset.as_str(),
        candidate.smooth_preset.as_str(),
    );
    bool_change(
        &mut general,
        "Show wheel options",
        current.show_discrete_scroll_options,
        candidate.show_discrete_scroll_options,
    );
    push_section(&mut changes, ConfigSection::General, general);

    if current.device_rules != candidate.device_rules {
        push_section(
            &mut changes,
            ConfigSection::Devices,
            vec![format!(
                "Per-device rule contents: {} -> {} entries",
                current.device_rules.len(),
                candidate.device_rules.len()
            )],
        );
    }

    let mut startup = Vec::new();
    bool_change(
        &mut startup,
        "CLI start at login",
        current.start_at_login,
        candidate.start_at_login,
    );
    push_section(&mut changes, ConfigSection::Startup, startup);

    let mut advanced = Vec::new();
    bool_change(
        &mut advanced,
        "Menu bar icon",
        current.show_menu_bar_icon,
        candidate.show_menu_bar_icon,
    );
    bool_change(
        &mut advanced,
        "Update checks",
        current.check_for_updates,
        candidate.check_for_updates,
    );
    bool_change(
        &mut advanced,
        "Beta updates",
        current.include_beta_updates,
        candidate.include_beta_updates,
    );
    bool_change(
        &mut advanced,
        "Raw input guard",
        current.reverse_only_raw_input,
        candidate.reverse_only_raw_input,
    );
    push_section(&mut changes, ConfigSection::Advanced, advanced);

    changes
}

fn copy_general(target: &mut AppConfig, source: &AppConfig) {
    target.enabled = source.enabled;
    target.reverse_vertical = source.reverse_vertical;
    target.reverse_horizontal = source.reverse_horizontal;
    target.reverse_mouse = source.reverse_mouse;
    target.reverse_trackpad = source.reverse_trackpad;
    target.reverse_magic_mouse = source.reverse_magic_mouse;
    target.reverse_unknown = source.reverse_unknown;
    target.discrete_scroll_step_size = source.discrete_scroll_step_size;
    target.smooth_preset = source.smooth_preset;
    target.show_discrete_scroll_options = source.show_discrete_scroll_options;
}

fn copy_advanced(target: &mut AppConfig, source: &AppConfig) {
    target.show_menu_bar_icon = source.show_menu_bar_icon;
    target.check_for_updates = source.check_for_updates;
    target.include_beta_updates = source.include_beta_updates;
    target.reverse_only_raw_input = source.reverse_only_raw_input;
}

fn push_section(changes: &mut Vec<SectionChange>, section: ConfigSection, details: Vec<String>) {
    if !details.is_empty() {
        changes.push(SectionChange { section, details });
    }
}

fn bool_change(details: &mut Vec<String>, label: &str, before: bool, after: bool) {
    value_change(
        details,
        label,
        if before { "On" } else { "Off" },
        if after { "On" } else { "Off" },
    );
}

fn value_change<T>(details: &mut Vec<String>, label: &str, before: T, after: T)
where
    T: fmt::Display + PartialEq,
{
    if before != after {
        details.push(format!("{label}: {before} -> {after}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview(candidate: AppConfig, current: &AppConfig) -> ConfigImportPreview {
        ConfigImportPreview::new(
            candidate,
            MigrationReport {
                source_version: CONFIG_VERSION,
                target_version: CONFIG_VERSION,
                actions: Vec::new(),
            },
            current,
        )
    }

    #[test]
    fn dry_run_lists_only_sections_that_will_change() {
        let candidate = AppConfig {
            reverse_horizontal: true,
            reverse_only_raw_input: true,
            ..AppConfig::default()
        };

        let preview = preview(candidate, &AppConfig::default());

        assert_eq!(preview.changes.len(), 2);
        assert_eq!(preview.changes[0].section, ConfigSection::General);
        assert_eq!(preview.changes[1].section, ConfigSection::Advanced);
        assert_eq!(preview.changed_section_labels(), "General, Advanced");
        assert!(preview.changes[0].details[0].contains("Horizontal direction"));
    }

    #[test]
    fn applying_reviewed_sections_preserves_unreviewed_sections() {
        let imported = AppConfig {
            reverse_horizontal: true,
            ..AppConfig::default()
        };
        let preview = preview(imported, &AppConfig::default());
        let latest = AppConfig {
            start_at_login: true,
            ..AppConfig::default()
        };

        let applied = preview.apply_changed_sections(&latest);

        assert!(applied.reverse_horizontal);
        assert!(applied.start_at_login);
        assert_eq!(preview.changed_section_labels(), "General");
    }

    #[test]
    fn rebase_adds_a_section_that_changed_while_review_was_open() {
        let imported = AppConfig {
            reverse_horizontal: true,
            ..AppConfig::default()
        };
        let mut preview = preview(imported, &AppConfig::default());
        let latest = AppConfig {
            start_at_login: true,
            ..AppConfig::default()
        };

        preview.rebase(&latest);

        assert_eq!(preview.changed_section_labels(), "General, Startup");
    }
}
