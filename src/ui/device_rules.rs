//! Connected/remembered device profile editor and exact-device activity test.

use std::collections::HashMap;

use eframe::egui::{self, Color32, RichText};

use crate::config::{
    AppConfig, ProfileSource, with_device_alias, with_device_rule_selection, without_device_profile,
};
use crate::device::{DeviceIdentity, DeviceKind};
use crate::device_catalog::{
    DeviceCatalogEntry, DeviceState, ObservedDevice, build_device_catalog,
};
use crate::device_source::HidSourceClass;
use crate::device_test::{DeviceActivity, DeviceTestSession, DeviceTestStatus};
use crate::input_policy::InputProvenance;
use crate::platform::macos::debug_log;

use super::theme::{device_rule_chip, status_dot, styled_button};

struct Editor<'a> {
    alias_edits: &'a mut HashMap<DeviceIdentity, String>,
    tests: &'a mut HashMap<DeviceIdentity, DeviceTestSession>,
    confirm_reset: &'a mut Option<DeviceIdentity>,
}

/// Returns `(config_changed, refresh_requested)`.
pub(super) fn controls(
    ui: &mut egui::Ui,
    devices: &[ObservedDevice],
    alias_edits: &mut HashMap<DeviceIdentity, String>,
    tests: &mut HashMap<DeviceIdentity, DeviceTestSession>,
    confirm_reset: &mut Option<DeviceIdentity>,
    config: &mut AppConfig,
) -> (bool, bool) {
    let mut editor = Editor {
        alias_edits,
        tests,
        confirm_reset,
    };
    let mut changed = false;
    let mut wants_refresh = false;
    let catalog = build_device_catalog(devices, &config.device_rules);
    let debug_events = debug_log::snapshot();
    let activities = debug_events
        .iter()
        .filter(|event| {
            !event.continuous
                && event.device_kind == DeviceKind::Mouse
                && event.input_provenance == InputProvenance::Hardware
                && event.hid_source == HidSourceClass::Physical
        })
        .filter_map(|event| {
            event.identity.as_deref().map(|identity| DeviceActivity {
                identity,
                timestamp_us: event.monotonic_us,
            })
        })
        .collect::<Vec<_>>();
    let now_us = debug_log::now_monotonic_micros();

    if catalog.is_empty() {
        ui.label(RichText::new("No pointing devices detected.").weak());
    }

    for state in [
        DeviceState::Connected,
        DeviceState::Remembered,
        DeviceState::Unavailable,
    ] {
        let entries: Vec<_> = catalog
            .iter()
            .filter(|entry| entry.state == state)
            .collect();
        if entries.is_empty() {
            continue;
        }

        ui.add_space(4.0);
        ui.label(RichText::new(device_state_label(state)).small().strong());
        for entry in entries {
            changed |= device_rule_row(ui, entry, &mut editor, &activities, now_us, config);
            ui.separator();
        }
    }

    ui.horizontal_wrapped(|ui| {
        if styled_button(ui, "Refresh devices", egui::vec2(12.0, 5.0)).clicked() {
            wants_refresh = true;
        }
        ui.label(
            RichText::new("Rules apply to clicky wheels only, not trackpad-style scrolling.")
                .small()
                .weak(),
        );
    });

    (changed, wants_refresh)
}

fn device_state_label(state: DeviceState) -> &'static str {
    match state {
        DeviceState::Connected => "CONNECTED",
        DeviceState::Remembered => "REMEMBERED",
        DeviceState::Unavailable => "UNAVAILABLE",
    }
}

fn device_rule_row(
    ui: &mut egui::Ui,
    entry: &DeviceCatalogEntry,
    editor: &mut Editor<'_>,
    activities: &[DeviceActivity<'_>],
    now_us: u64,
    config: &mut AppConfig,
) -> bool {
    let Some(identity) = entry.identity.as_ref() else {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(&entry.display_name);
                ui.label(
                    RichText::new(
                        entry
                            .transport
                            .as_deref()
                            .map(|transport| format!("{transport} · stable identity unavailable"))
                            .unwrap_or_else(|| "Stable identity unavailable".to_string()),
                    )
                    .small()
                    .weak(),
                );
            });
        });
        return false;
    };

    let current = config
        .preferred_device_rule(identity)
        .and_then(|rule| rule.reverse);
    let has_saved_profile = config.preferred_device_rule(identity).is_some();
    let resolved = config.resolve_device_profile(DeviceKind::Mouse, Some(identity));
    let inherited_note = if current.is_none() && resolved.reverse.source.is_device_rule() {
        let scope = match resolved.reverse.source {
            ProfileSource::ExactSerial => "Serial rule",
            ProfileSource::ExactLocation => "Port rule",
            ProfileSource::Hardware => "Shared model rule",
            ProfileSource::DeviceKind(_) | ProfileSource::GlobalDefault => "Inherited rule",
        };
        Some(format!(
            "{scope}: {}",
            if resolved.reverse.value {
                "Reverse"
            } else {
                "Don't reverse"
            }
        ))
    } else {
        None
    };
    let configured_alias = config
        .preferred_device_rule(identity)
        .and_then(|rule| rule.alias.as_deref())
        .unwrap_or_default()
        .to_string();
    let mut selection = current;
    let mut commit_alias = false;
    let mut finish_alias_edit = false;
    let mut alias_value = String::new();

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(&entry.display_name);
            if entry.alias.is_some()
                && let Some(product_name) = &entry.product_name
            {
                ui.label(RichText::new(product_name).small().weak());
            }
            ui.label(
                RichText::new(compact_device_identity(identity))
                    .small()
                    .monospace()
                    .weak(),
            );
            if let Some(note) = &inherited_note {
                ui.label(RichText::new(note).small().weak());
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.vertical(|ui| {
                device_rule_chip(ui, ("device-rule", identity), &mut selection);
                let edit = editor
                    .alias_edits
                    .entry(identity.clone())
                    .or_insert_with(|| configured_alias.clone());
                let response = ui.add(
                    egui::TextEdit::singleline(edit)
                        .hint_text("Alias")
                        .char_limit(64)
                        .desired_width(150.0),
                );
                let enter_pressed =
                    response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                finish_alias_edit = response.lost_focus() || enter_pressed;
                let valid_live_edit =
                    response.changed() && (edit.is_empty() || edit.trim() == edit.as_str());
                commit_alias = finish_alias_edit || valid_live_edit;
                if commit_alias {
                    alias_value = edit.trim().to_string();
                }
                if enter_pressed {
                    response.surrender_focus();
                }
            });
        });
    });

    let mut changed = false;
    if selection != current {
        config.device_rules = with_device_rule_selection(
            &config.device_rules,
            identity,
            entry.product_name.as_deref(),
            selection,
        );
        changed = true;
    }

    if entry.state == DeviceState::Connected {
        let active_profile = config.resolve_device_profile(DeviceKind::Mouse, Some(identity));
        let active_rule = format!(
            "Active: {} · {}",
            if active_profile.reverse.value {
                "Reverse"
            } else {
                "Don't reverse"
            },
            active_profile.reverse.source.label(),
        );
        ui.horizontal_wrapped(|ui| {
            if identity.serial_number.is_none() && identity.location_id.is_none() {
                ui.label(
                    RichText::new(format!(
                        "Exact test unavailable · model-wide identity · {active_rule}"
                    ))
                    .small()
                    .weak(),
                );
                return;
            }

            if styled_button(ui, "Test this device", egui::vec2(10.0, 4.0)).clicked() {
                editor
                    .tests
                    .insert(identity.clone(), DeviceTestSession::start(now_us));
            }

            let Some(session) = editor.tests.get_mut(identity) else {
                ui.label(RichText::new(active_rule).small().weak());
                return;
            };
            match session.observe(identity, now_us, activities) {
                DeviceTestStatus::Listening { .. } => {
                    status_dot(ui, Color32::from_rgb(0xE5, 0x9E, 0x2F), 3.0, 8.0);
                    ui.label(
                        RichText::new(format!("Listening... · {active_rule}"))
                            .small()
                            .weak(),
                    );
                }
                DeviceTestStatus::Detected { age_us } => {
                    status_dot(ui, Color32::from_rgb(0x34, 0xA8, 0x53), 3.0, 8.0);
                    ui.label(
                        RichText::new(format!(
                            "Detected {:.1}s ago · {active_rule}",
                            age_us as f64 / 1_000_000.0
                        ))
                        .small()
                        .color(Color32::from_rgb(0x34, 0xA8, 0x53)),
                    );
                }
                DeviceTestStatus::TimedOut => {
                    status_dot(ui, Color32::from_rgb(0xE5, 0x9E, 0x2F), 3.0, 8.0);
                    ui.label(
                        RichText::new(format!("No event in 5s · {active_rule}"))
                            .small()
                            .weak(),
                    );
                }
            }
        });
    }

    if commit_alias {
        if finish_alias_edit {
            editor.alias_edits.remove(identity);
        }
        let alias = (!alias_value.is_empty()).then_some(alias_value.as_str());
        if alias != (!configured_alias.is_empty()).then_some(configured_alias.as_str()) {
            config.device_rules = with_device_alias(
                &config.device_rules,
                identity,
                entry.product_name.as_deref(),
                alias,
            );
            changed = true;
        }
    }

    if has_saved_profile {
        let awaiting_confirmation = editor.confirm_reset.as_ref() == Some(identity);
        ui.horizontal_wrapped(|ui| {
            if awaiting_confirmation {
                ui.label(RichText::new("Remove this device's saved profile?").small());
                if styled_button(ui, "Cancel", egui::vec2(10.0, 4.0)).clicked() {
                    *editor.confirm_reset = None;
                }
                if styled_button(ui, "Reset device", egui::vec2(10.0, 4.0)).clicked() {
                    *config = without_device_profile(config, identity);
                    editor.alias_edits.remove(identity);
                    editor.tests.remove(identity);
                    *editor.confirm_reset = None;
                    changed = true;
                }
            } else if styled_button(ui, "Reset this device", egui::vec2(10.0, 4.0)).clicked() {
                *editor.confirm_reset = Some(identity.clone());
            }
        });
    }
    changed
}

fn compact_device_identity(identity: &DeviceIdentity) -> String {
    let mut label = format!(
        "{:04x}:{:04x}",
        identity.hardware.vendor_id, identity.hardware.product_id
    );
    if let Some(qualifier) = identity.compact_qualifier() {
        label.push_str(&format!(" · {qualifier}"));
    } else {
        label.push_str(" · model-wide ID");
    }
    label
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::device::HardwareId;

    use super::*;

    fn hardware() -> HardwareId {
        HardwareId {
            vendor_id: 0x046d,
            product_id: 0xc52b,
        }
    }

    #[test]
    fn serial_label_keeps_a_bounded_distinguishing_suffix() {
        let identity =
            DeviceIdentity::new(hardware(), Some(Arc::from("1234567890abcdef")), Some(42));

        assert_eq!(
            compact_device_identity(&identity),
            "046d:c52b · serial …567890abcdef"
        );
    }

    #[test]
    fn location_fallback_is_named_as_a_port() {
        let identity = DeviceIdentity::new(hardware(), None, Some(42));

        assert_eq!(
            compact_device_identity(&identity),
            "046d:c52b · port 0x0000002a"
        );
    }
}
