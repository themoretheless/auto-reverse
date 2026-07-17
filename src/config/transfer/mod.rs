//! Versioned configuration transfer split by responsibility.
//!
//! `document` owns TOML/version/schema migration, `diff` owns the pure
//! section review and application policy, and `secure_file` owns the bounded
//! filesystem trust boundary. This facade is the only surface callers need.

use std::fmt;
use std::path::{Path, PathBuf};

use super::AppConfig;

mod diff;
mod document;
mod secure_file;

pub use diff::{ConfigImportPreview, ConfigSection, SectionChange};
pub use document::{MigrationReport, export_document, preview_import_document};
pub use secure_file::MAX_IMPORT_BYTES;

#[derive(Debug)]
pub enum TransferError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Symlink(PathBuf),
    NotRegularFile(PathBuf),
    WorldWritable(PathBuf),
    TooLarge {
        actual: u64,
        maximum: u64,
    },
    ChangedWhileReading(PathBuf),
    InvalidUtf8,
    InvalidToml(String),
    InvalidVersion(String),
    UnsupportedVersion {
        found: u32,
        supported: u32,
    },
    UnknownFields(Vec<String>),
    InvalidSchema(String),
    Serialize(String),
}

impl fmt::Display for TransferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "could not read `{}`: {source}", path.display())
            }
            Self::Symlink(path) => write!(
                formatter,
                "`{}` is a symbolic link; choose the original config file",
                path.display()
            ),
            Self::NotRegularFile(path) => {
                write!(formatter, "`{}` is not a regular file", path.display())
            }
            Self::WorldWritable(path) => write!(
                formatter,
                "`{}` is writable by every local user and cannot be imported safely",
                path.display()
            ),
            Self::TooLarge { actual, maximum } => write!(
                formatter,
                "config import is {actual} bytes; the limit is {maximum} bytes"
            ),
            Self::ChangedWhileReading(path) => write!(
                formatter,
                "`{}` changed while it was being reviewed; choose it again",
                path.display()
            ),
            Self::InvalidUtf8 => write!(formatter, "config import is not valid UTF-8"),
            Self::InvalidToml(message) => write!(formatter, "config TOML is invalid: {message}"),
            Self::InvalidVersion(message) => {
                write!(formatter, "config_version is invalid: {message}")
            }
            Self::UnsupportedVersion { found, supported } => write!(
                formatter,
                "config_version {found} is newer than supported version {supported}"
            ),
            Self::UnknownFields(fields) => write!(
                formatter,
                "unknown config field(s): {}; refusing to discard them",
                fields.join(", ")
            ),
            Self::InvalidSchema(message) => write!(formatter, "config is invalid: {message}"),
            Self::Serialize(message) => {
                write!(formatter, "could not serialize configuration: {message}")
            }
        }
    }
}

impl std::error::Error for TransferError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Symlink(_)
            | Self::NotRegularFile(_)
            | Self::WorldWritable(_)
            | Self::TooLarge { .. }
            | Self::ChangedWhileReading(_)
            | Self::InvalidUtf8
            | Self::InvalidToml(_)
            | Self::InvalidVersion(_)
            | Self::UnsupportedVersion { .. }
            | Self::UnknownFields(_)
            | Self::InvalidSchema(_)
            | Self::Serialize(_) => None,
        }
    }
}

pub fn preview_import_file(
    path: &Path,
    current: &AppConfig,
) -> Result<ConfigImportPreview, TransferError> {
    let document = secure_file::read_secure_file(path)?;
    preview_import_document(&document, current)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn file_facade_runs_security_then_document_preview() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "auto-reverse-transfer-facade-{}-{nanos}.toml",
            process::id()
        ));
        let expected = AppConfig {
            reverse_horizontal: true,
            ..AppConfig::default()
        };
        fs::write(&path, export_document(&expected).unwrap()).unwrap();

        let preview = preview_import_file(&path, &AppConfig::default()).unwrap();

        assert_eq!(preview.candidate, expected);
        let _ = fs::remove_file(path);
    }
}
