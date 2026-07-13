//! Pure presentation catalog for connected, remembered, and unavailable devices.

use std::collections::HashMap;
use std::sync::Arc;

use crate::config::DeviceRule;
use crate::device::{DeviceIdentity, HardwareId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceState {
    Connected,
    Remembered,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedDevice {
    pub identity: Option<DeviceIdentity>,
    pub name: Option<String>,
    pub transport: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCatalogEntry {
    pub state: DeviceState,
    pub identity: Option<DeviceIdentity>,
    pub product_name: Option<String>,
    pub alias: Option<String>,
    pub transport: Option<String>,
    pub display_name: String,
}

pub fn build_device_catalog(
    observed: &[ObservedDevice],
    rules: &[DeviceRule],
) -> Vec<DeviceCatalogEntry> {
    let connected_identities: Vec<&DeviceIdentity> = observed
        .iter()
        .filter_map(|device| device.identity.as_ref())
        .collect();
    let mut entries: Vec<DeviceCatalogEntry> = Vec::with_capacity(observed.len() + rules.len());

    for device in observed {
        if let Some(identity) = device.identity.as_ref()
            && let Some(existing) = entries.iter_mut().find(|entry| {
                entry.state == DeviceState::Connected && entry.identity.as_ref() == Some(identity)
            })
        {
            if existing.product_name.is_none() {
                existing.product_name = device.name.clone();
            }
            if existing.transport.is_none() {
                existing.transport = device.transport.clone();
            }
            continue;
        }
        let alias = device
            .identity
            .as_ref()
            .and_then(|identity| resolved_alias(rules, identity))
            .map(str::to_owned);
        let state = if device.identity.is_some() {
            DeviceState::Connected
        } else {
            DeviceState::Unavailable
        };
        entries.push(DeviceCatalogEntry {
            state,
            identity: device.identity.clone(),
            product_name: device.name.clone(),
            alias,
            transport: device.transport.clone(),
            display_name: String::new(),
        });
    }

    for rule in rules {
        if connected_identities
            .iter()
            .any(|identity| rule.matches(identity))
        {
            continue;
        }
        entries.push(DeviceCatalogEntry {
            state: DeviceState::Remembered,
            identity: Some(identity_for_rule(rule)),
            product_name: rule.name.clone(),
            alias: rule.alias.clone(),
            transport: None,
            display_name: String::new(),
        });
    }

    assign_display_names(&mut entries);
    entries.sort_by(|left, right| {
        state_rank(left.state)
            .cmp(&state_rank(right.state))
            .then_with(|| left.display_name.cmp(&right.display_name))
            .then_with(|| left.identity.cmp(&right.identity))
    });
    entries
}

fn resolved_alias<'a>(rules: &'a [DeviceRule], identity: &DeviceIdentity) -> Option<&'a str> {
    rules
        .iter()
        .filter(|rule| rule.matches(identity))
        .filter_map(|rule| rule.alias.as_deref().map(|alias| (rule, alias)))
        .max_by_key(|(rule, _)| rule.specificity())
        .map(|(_, alias)| alias)
}

fn identity_for_rule(rule: &DeviceRule) -> DeviceIdentity {
    DeviceIdentity::new(
        HardwareId {
            vendor_id: rule.vendor_id,
            product_id: rule.product_id,
        },
        rule.serial_number.as_deref().map(Arc::from),
        rule.location_id,
    )
}

fn assign_display_names(entries: &mut [DeviceCatalogEntry]) {
    let base_names: Vec<String> = entries
        .iter()
        .map(|entry| {
            entry
                .alias
                .as_deref()
                .or(entry.product_name.as_deref())
                .unwrap_or(match entry.state {
                    DeviceState::Remembered => "Remembered mouse",
                    DeviceState::Connected | DeviceState::Unavailable => "Unnamed pointing device",
                })
                .to_owned()
        })
        .collect();
    let mut counts = HashMap::new();
    for name in &base_names {
        *counts.entry(name.to_lowercase()).or_insert(0usize) += 1;
    }

    for (entry, base) in entries.iter_mut().zip(base_names) {
        let duplicate = counts.get(&base.to_lowercase()).copied().unwrap_or(0) > 1;
        entry.display_name = if duplicate {
            entry
                .identity
                .as_ref()
                .map(|identity| format!("{base} · {}", stable_suffix(identity)))
                .unwrap_or(base)
        } else {
            base
        };
    }
}

fn stable_suffix(identity: &DeviceIdentity) -> String {
    identity.compact_qualifier().unwrap_or_else(|| {
        format!(
            "{:04x}:{:04x}",
            identity.hardware.vendor_id, identity.hardware.product_id
        )
    })
}

const fn state_rank(state: DeviceState) -> u8 {
    match state {
        DeviceState::Connected => 0,
        DeviceState::Remembered => 1,
        DeviceState::Unavailable => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(serial: &str) -> DeviceIdentity {
        DeviceIdentity::new(
            HardwareId {
                vendor_id: 0x046d,
                product_id: 0xc52b,
            },
            Some(Arc::from(serial)),
            None,
        )
    }

    #[test]
    fn separates_connected_remembered_and_unavailable_devices() {
        let connected = identity("connected-a");
        let remembered = identity("remembered-b");
        let observed = vec![
            ObservedDevice {
                identity: Some(connected.clone()),
                name: Some("MX Master".to_string()),
                transport: Some("USB".to_string()),
            },
            ObservedDevice {
                identity: None,
                name: Some("Mystery mouse".to_string()),
                transport: None,
            },
        ];
        let rules = vec![DeviceRule::for_identity(&remembered, None, true)];

        let catalog = build_device_catalog(&observed, &rules);

        assert_eq!(catalog[0].state, DeviceState::Connected);
        assert_eq!(catalog[1].state, DeviceState::Remembered);
        assert_eq!(catalog[2].state, DeviceState::Unavailable);
    }

    #[test]
    fn aliases_follow_selector_specificity_without_copying_other_fields() {
        let target = identity("mouse-a");
        let rules = vec![
            DeviceRule {
                alias: Some("Shared model".to_string()),
                ..DeviceRule::for_hardware(target.hardware, None, true)
            },
            DeviceRule {
                alias: Some("Desk mouse".to_string()),
                reverse: None,
                ..DeviceRule::for_identity(&target, None, true)
            },
        ];
        let observed = vec![ObservedDevice {
            identity: Some(target),
            name: Some("MX Master".to_string()),
            transport: Some("USB".to_string()),
        }];

        let catalog = build_device_catalog(&observed, &rules);

        assert_eq!(catalog[0].display_name, "Desk mouse");
        assert_eq!(catalog[0].alias.as_deref(), Some("Desk mouse"));
    }

    #[test]
    fn duplicate_product_names_receive_stable_identity_suffixes() {
        let first = identity("serial-a");
        let second = identity("serial-b");
        let observed = vec![
            ObservedDevice {
                identity: Some(first),
                name: Some("Same mouse".to_string()),
                transport: Some("USB".to_string()),
            },
            ObservedDevice {
                identity: Some(second),
                name: Some("Same mouse".to_string()),
                transport: Some("USB".to_string()),
            },
        ];

        let catalog = build_device_catalog(&observed, &[]);

        assert_ne!(catalog[0].display_name, catalog[1].display_name);
        assert!(
            catalog
                .iter()
                .all(|entry| entry.display_name.contains("serial"))
        );
    }

    #[test]
    fn a_matching_connected_rule_is_not_duplicated_as_remembered() {
        let target = identity("mouse-a");
        let observed = vec![ObservedDevice {
            identity: Some(target.clone()),
            name: Some("Mouse".to_string()),
            transport: Some("USB".to_string()),
        }];
        let rules = vec![DeviceRule::for_identity(&target, None, true)];

        let catalog = build_device_catalog(&observed, &rules);

        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].state, DeviceState::Connected);
    }

    #[test]
    fn multiple_hid_services_for_one_identity_collapse_into_one_row() {
        let target = identity("mouse-a");
        let observed = vec![
            ObservedDevice {
                identity: Some(target.clone()),
                name: None,
                transport: Some("USB".to_string()),
            },
            ObservedDevice {
                identity: Some(target),
                name: Some("Mouse".to_string()),
                transport: None,
            },
        ];

        let catalog = build_device_catalog(&observed, &[]);

        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].product_name.as_deref(), Some("Mouse"));
        assert_eq!(catalog[0].transport.as_deref(), Some("USB"));
    }
}
