//! Debug-event detailed CSV and privacy trace export workflows.
//!
//! Presentation stays in the parent module. This module owns destination
//! selection, CSV serialization, atomic replacement, the structured success
//! receipt, and Finder reveal.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::config::ConfigStore;
use crate::platform::macos::{debug_log, save_panel};
use crate::scroll_trace::{ScrollTrace, TraceSample};

use super::super::local_export;

#[derive(Clone)]
pub(super) struct Receipt {
    path: PathBuf,
    event_count: usize,
}

impl Receipt {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn summary(&self) -> String {
        let filename: String = self
            .path
            .file_name()
            .unwrap_or(self.path.as_os_str())
            .to_string_lossy()
            .chars()
            .map(|character| {
                if character.is_whitespace() {
                    ' '
                } else {
                    character
                }
            })
            .collect();
        let noun = if self.event_count == 1 {
            "event"
        } else {
            "events"
        };
        format!("Exported {} {noun} to {filename}", self.event_count)
    }
}

pub(super) fn run_csv(
    events: &[debug_log::DebugEvent],
    previous: Option<&Receipt>,
) -> Result<Option<Receipt>, String> {
    let initial_directory = export_directory(previous);
    let now_ms = debug_log::now_millis();
    let Some(file_path) = save_panel::choose_csv_path(
        &format!("debug-events-{now_ms}.csv"),
        initial_directory.as_deref(),
    )?
    else {
        return Ok(None);
    };
    let csv = events_to_csv(events);
    local_export::write_atomically(&file_path, &csv)?;

    Ok(Some(Receipt {
        path: file_path,
        event_count: events.len(),
    }))
}

pub(super) fn run_trace(
    events: &[debug_log::DebugEvent],
    previous: Option<&Receipt>,
) -> Result<Option<Receipt>, String> {
    let trace = events_to_trace(events)?;
    let toml = trace.to_toml().map_err(|error| error.to_string())?;
    let initial_directory = export_directory(previous);
    let now_ms = debug_log::now_millis();
    let Some(file_path) = save_panel::choose_toml_path(
        &format!("scroll-trace-{now_ms}.toml"),
        initial_directory.as_deref(),
    )?
    else {
        return Ok(None);
    };
    local_export::write_atomically(&file_path, &toml)?;

    Ok(Some(Receipt {
        path: file_path,
        event_count: events.len(),
    }))
}

pub(super) fn reveal(receipt: &Receipt) -> Result<(), String> {
    save_panel::reveal_in_finder(&receipt.path)
}

fn export_directory(previous: Option<&Receipt>) -> Option<PathBuf> {
    if let Some(directory) = previous
        .and_then(|receipt| receipt.path.parent())
        .filter(|path| path.is_dir())
    {
        return Some(directory.to_path_buf());
    }

    let config_directory = ConfigStore::default_path().parent()?.to_path_buf();
    let legacy_export_directory = config_directory.join("Debug Logs");
    Some(if legacy_export_directory.is_dir() {
        legacy_export_directory
    } else {
        config_directory
    })
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
            local_export::csv_escape(&device_description),
            event.device_kind.as_str(),
            local_export::csv_escape(device_name),
            vendor_id,
            product_id,
            event.source_pid,
            event.synthetic,
            event.axis.code(),
            event.raw_delta,
            event.output_delta,
            event.category().code(),
            event.reason.code(),
            local_export::csv_escape(&decision_text),
        )
        .expect("writing to a String cannot fail");
    }

    csv
}

fn events_to_trace(events: &[debug_log::DebugEvent]) -> Result<ScrollTrace, String> {
    let origin_us = events
        .first()
        .map(|event| event.monotonic_us)
        .ok_or_else(|| "there are no events to export as a trace".to_string())?;
    let samples = events
        .iter()
        .map(|event| TraceSample {
            timestamp_us: event.monotonic_us.saturating_sub(origin_us),
            device_kind: event.device_kind,
            continuous: event.continuous,
            axis: event.axis,
            input_delta: event.raw_delta,
            observed_output_delta: event.output_delta,
            decision_reason: event.reason,
        })
        .collect();
    ScrollTrace::new(samples).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::device::{DeviceKind, HardwareId};

    use super::*;

    #[test]
    fn csv_export_contains_raw_structured_source_and_reason_fields() {
        let event = debug_log::DebugEvent {
            timestamp_ms: 42,
            monotonic_us: 42_000,
            device_kind: DeviceKind::Mouse,
            device_name: Some(Arc::from("MX, Master\n3S")),
            hardware: Some(HardwareId {
                vendor_id: 0x046d,
                product_id: 0xb034,
            }),
            source_pid: 123,
            synthetic: true,
            continuous: false,
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

    #[test]
    fn privacy_trace_uses_relative_time_and_strips_source_identity() {
        let events = [
            debug_log::DebugEvent {
                timestamp_ms: 1_000_000,
                monotonic_us: 80_000,
                device_kind: DeviceKind::Mouse,
                device_name: Some(Arc::from("Private Mouse Name")),
                hardware: Some(HardwareId {
                    vendor_id: 0x046d,
                    product_id: 0xb034,
                }),
                source_pid: 123,
                synthetic: false,
                continuous: false,
                axis: debug_log::Axis::Vertical,
                raw_delta: 1,
                output_delta: -3,
                reason: debug_log::DecisionReason::Reversed,
            },
            debug_log::DebugEvent {
                timestamp_ms: 1_000_008,
                monotonic_us: 88_000,
                axis: debug_log::Axis::Horizontal,
                ..debug_log::DebugEvent {
                    timestamp_ms: 1_000_000,
                    monotonic_us: 80_000,
                    device_kind: DeviceKind::Mouse,
                    device_name: Some(Arc::from("Private Mouse Name")),
                    hardware: Some(HardwareId {
                        vendor_id: 0x046d,
                        product_id: 0xb034,
                    }),
                    source_pid: 123,
                    synthetic: false,
                    continuous: false,
                    axis: debug_log::Axis::Vertical,
                    raw_delta: 1,
                    output_delta: -3,
                    reason: debug_log::DecisionReason::Reversed,
                }
            },
        ];

        let trace = events_to_trace(&events).unwrap();
        let serialized = trace.to_toml().unwrap();

        assert_eq!(trace.samples()[0].timestamp_us, 0);
        assert_eq!(trace.samples()[1].timestamp_us, 8_000);
        for private_value in [
            "Private Mouse Name",
            "source_pid",
            "vendor_id",
            "product_id",
            "1000000",
        ] {
            assert!(!serialized.contains(private_value));
        }
    }

    #[test]
    fn receipt_uses_compact_singular_and_plural_copy() {
        let path = PathBuf::from("/tmp/debug-events.csv");

        assert_eq!(
            Receipt {
                path: path.clone(),
                event_count: 1,
            }
            .summary(),
            "Exported 1 event to debug-events.csv"
        );
        assert_eq!(
            Receipt {
                path,
                event_count: 2,
            }
            .summary(),
            "Exported 2 events to debug-events.csv"
        );
    }

    #[test]
    fn receipt_summary_normalizes_filename_whitespace_only_for_display() {
        let receipt = Receipt {
            path: PathBuf::from("/tmp/debug\nevents.csv"),
            event_count: 3,
        };

        assert_eq!(receipt.summary(), "Exported 3 events to debug events.csv");
        assert_eq!(receipt.path(), Path::new("/tmp/debug\nevents.csv"));
    }
}
