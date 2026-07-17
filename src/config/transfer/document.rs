//! Version detection, migration, schema validation and TOML serialization.

use crate::config::{AppConfig, CONFIG_VERSION};
use crate::error::AppError;

use super::TransferError;
use super::diff::ConfigImportPreview;

const ROOT_FIELDS: &[&str] = &[
    "config_version",
    "enabled",
    "reverse_vertical",
    "reverse_horizontal",
    "reverse_mouse",
    "reverse_trackpad",
    "reverse_magic_mouse",
    "reverse_unknown",
    "discrete_scroll_step_size",
    "smooth_preset",
    "show_discrete_scroll_options",
    "start_at_login",
    "show_menu_bar_icon",
    "check_for_updates",
    "include_beta_updates",
    "reverse_only_raw_input",
    "device_rules",
];

const DEVICE_RULE_FIELDS: &[&str] = &[
    "vendor_id",
    "product_id",
    "serial_number",
    "location_id",
    "name",
    "alias",
    "reverse",
    "step_size",
    "smooth_preset",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    pub source_version: u32,
    pub target_version: u32,
    pub actions: Vec<String>,
}

impl MigrationReport {
    pub fn migrated(&self) -> bool {
        self.source_version != self.target_version || !self.actions.is_empty()
    }
}

pub fn export_document(config: &AppConfig) -> Result<String, TransferError> {
    config
        .validate()
        .map_err(|error| TransferError::InvalidSchema(validation_message(error)))?;
    let body = toml::to_string_pretty(config)
        .map_err(|error| TransferError::Serialize(error.to_string()))?;
    Ok(format!(
        "# Auto Reverse configuration export\n# config_version controls migration compatibility.\n\n{body}"
    ))
}

pub fn preview_import_document(
    document: &str,
    current: &AppConfig,
) -> Result<ConfigImportPreview, TransferError> {
    let mut value: toml::Value =
        toml::from_str(document).map_err(|error| TransferError::InvalidToml(error.to_string()))?;
    let root = value
        .as_table_mut()
        .ok_or_else(|| TransferError::InvalidToml("root must be a TOML table".to_string()))?;

    let source_version = parse_version(root.get("config_version"))?;
    if source_version > CONFIG_VERSION {
        return Err(TransferError::UnsupportedVersion {
            found: source_version,
            supported: CONFIG_VERSION,
        });
    }

    let unknown_fields = unknown_field_paths(root);
    if !unknown_fields.is_empty() {
        return Err(TransferError::UnknownFields(unknown_fields));
    }

    let mut actions = Vec::new();
    if source_version == 0 {
        root.insert(
            "config_version".to_string(),
            toml::Value::Integer(i64::from(CONFIG_VERSION)),
        );
        actions.push(
            "Assigned config_version 1 and filled fields absent from the legacy document with current defaults."
                .to_string(),
        );
    }

    let candidate: AppConfig = value
        .try_into()
        .map_err(|error: toml::de::Error| TransferError::InvalidSchema(error.to_string()))?;
    candidate
        .validate()
        .map_err(|error| TransferError::InvalidSchema(validation_message(error)))?;

    Ok(ConfigImportPreview::new(
        candidate,
        MigrationReport {
            source_version,
            target_version: CONFIG_VERSION,
            actions,
        },
        current,
    ))
}

fn validation_message(error: AppError) -> String {
    match error {
        AppError::InvalidConfig(message) => message,
        other => other.to_string(),
    }
}

fn parse_version(value: Option<&toml::Value>) -> Result<u32, TransferError> {
    let Some(value) = value else {
        return Ok(0);
    };
    let version = value.as_integer().ok_or_else(|| {
        TransferError::InvalidVersion("expected a non-negative integer".to_string())
    })?;
    u32::try_from(version).map_err(|_| {
        TransferError::InvalidVersion("expected a non-negative 32-bit integer".to_string())
    })
}

fn unknown_field_paths(root: &toml::Table) -> Vec<String> {
    let mut unknown = root
        .keys()
        .filter(|field| !ROOT_FIELDS.contains(&field.as_str()))
        .cloned()
        .collect::<Vec<_>>();

    if let Some(rules) = root.get("device_rules").and_then(toml::Value::as_array) {
        for (index, rule) in rules.iter().enumerate() {
            if let Some(table) = rule.as_table() {
                unknown.extend(
                    table
                        .keys()
                        .filter(|field| !DEVICE_RULE_FIELDS.contains(&field.as_str()))
                        .map(|field| format!("device_rules[{index}].{field}")),
                );
            }
        }
    }
    unknown.sort();
    unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_export_round_trips_without_migration_or_changes() {
        let config = AppConfig {
            reverse_horizontal: true,
            ..AppConfig::default()
        };

        let document = export_document(&config).unwrap();
        let preview = preview_import_document(&document, &config).unwrap();

        assert_eq!(preview.candidate, config);
        assert!(!preview.migration.migrated());
        assert!(!preview.has_changes());
    }

    #[test]
    fn missing_and_explicit_zero_versions_migrate_to_current_schema() {
        for document in ["enabled = false\n", "config_version = 0\nenabled = false\n"] {
            let preview = preview_import_document(document, &AppConfig::default()).unwrap();

            assert_eq!(preview.migration.source_version, 0);
            assert_eq!(preview.migration.target_version, CONFIG_VERSION);
            assert!(preview.migration.migrated());
            assert_eq!(preview.candidate.config_version, CONFIG_VERSION);
            assert!(!preview.candidate.enabled);
        }
    }

    #[test]
    fn future_version_is_rejected_before_fields_are_discarded() {
        let error = preview_import_document(
            "config_version = 99\na_future_field = true\n",
            &AppConfig::default(),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            TransferError::UnsupportedVersion {
                found: 99,
                supported: CONFIG_VERSION
            }
        ));
    }

    #[test]
    fn unknown_current_fields_are_rejected_instead_of_silently_lost() {
        let error = preview_import_document(
            "config_version = 1\nenabled = true\ntypo_enabled = false\n",
            &AppConfig::default(),
        )
        .unwrap_err();

        assert!(
            matches!(error, TransferError::UnknownFields(fields) if fields == ["typo_enabled"])
        );
    }

    #[test]
    fn unknown_nested_rule_fields_are_reported_with_their_index() {
        let error = preview_import_document(
            "config_version = 1\n\
             [[device_rules]]\n\
             vendor_id = 1\n\
             product_id = 2\n\
             revers = true\n",
            &AppConfig::default(),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            TransferError::UnknownFields(fields)
                if fields == ["device_rules[0].revers"]
        ));
    }

    #[test]
    fn schema_validation_runs_before_a_preview_is_returned() {
        let error = preview_import_document(
            "config_version = 1\ndiscrete_scroll_step_size = 99\n",
            &AppConfig::default(),
        )
        .unwrap_err();

        assert!(matches!(&error, TransferError::InvalidSchema(_)));
        assert_eq!(
            error.to_string(),
            "config is invalid: discrete_scroll_step_size must be between 0 and 20"
        );
    }
}
