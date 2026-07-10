//! Debug-event CSV export workflow.
//!
//! Presentation stays in the parent module. This module owns destination
//! selection, CSV serialization, atomic replacement, the structured success
//! receipt, and Finder reveal.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::config::ConfigStore;
use crate::platform::macos::{debug_log, save_panel};

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

pub(super) fn run(
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
    write_export(&file_path, &csv)?;

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

fn write_export(path: &Path, contents: &str) -> Result<(), String> {
    let file_name = path
        .file_name()
        .ok_or_else(|| "the selected export path has no file name".to_string())?;
    let request_id = NEXT_EXPORT_ID.fetch_add(1, Ordering::Relaxed);
    let mut temp_name = file_name.to_os_string();
    temp_name.push(format!(".{}.{}.tmp", process::id(), request_id));
    let temp_path = path.with_file_name(temp_name);

    std::fs::write(&temp_path, contents).map_err(|error| {
        let _ = std::fs::remove_file(&temp_path);
        format!("could not write `{}`: {error}", temp_path.display())
    })?;
    std::fs::rename(&temp_path, path).map_err(|error| {
        let _ = std::fs::remove_file(&temp_path);
        format!("could not replace `{}`: {error}", path.display())
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

static NEXT_EXPORT_ID: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::device::{DeviceKind, HardwareId};

    use super::*;

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

    #[test]
    fn write_export_atomically_replaces_the_selected_file() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "auto-reverse-debug-export-{}-{nanos}.csv",
            process::id()
        ));
        fs::write(&path, "old").unwrap();

        write_export(&path, "new").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        let _ = fs::remove_file(path);
    }
}
