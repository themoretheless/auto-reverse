//! Shared local CSV helpers for diagnostics windows.

use std::path::Path;
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) fn write_atomically(path: &Path, contents: &str) -> Result<(), String> {
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

pub(super) fn csv_escape(value: &str) -> String {
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
    use std::time::{SystemTime, UNIX_EPOCH};

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
    fn atomic_write_replaces_the_selected_file() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "auto-reverse-local-export-{}-{nanos}.csv",
            process::id()
        ));
        fs::write(&path, "old").unwrap();

        write_atomically(&path, "new").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        let _ = fs::remove_file(path);
    }
}
