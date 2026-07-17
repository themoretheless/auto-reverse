//! Bounded Unix-aware file read before untrusted TOML parsing.

use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::Path;

#[cfg(test)]
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};

use super::TransferError;

/// A configuration document is tiny in normal use. This leaves ample room
/// for hundreds of device rules while bounding allocation and parse work.
pub const MAX_IMPORT_BYTES: u64 = 256 * 1024;

pub(super) fn read_secure_file(path: &Path) -> Result<String, TransferError> {
    let selected = std::fs::symlink_metadata(path).map_err(|source| TransferError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if selected.file_type().is_symlink() {
        return Err(TransferError::Symlink(path.to_path_buf()));
    }
    validate_metadata(path, &selected)?;

    let mut file = open_without_following(path)?;
    let opened = file.metadata().map_err(|source| TransferError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    validate_metadata(path, &opened)?;
    if !same_file(&selected, &opened) {
        return Err(TransferError::ChangedWhileReading(path.to_path_buf()));
    }

    let mut bytes = Vec::with_capacity(usize::try_from(opened.len()).unwrap_or(0));
    (&mut file)
        .take(MAX_IMPORT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| TransferError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() as u64 > MAX_IMPORT_BYTES {
        return Err(TransferError::TooLarge {
            actual: bytes.len() as u64,
            maximum: MAX_IMPORT_BYTES,
        });
    }

    let after = file.metadata().map_err(|source| TransferError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    validate_metadata(path, &after)?;
    if !same_file(&opened, &after) || !same_snapshot(&opened, &after) {
        return Err(TransferError::ChangedWhileReading(path.to_path_buf()));
    }

    String::from_utf8(bytes).map_err(|_| TransferError::InvalidUtf8)
}

fn open_without_following(path: &Path) -> Result<File, TransferError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);

    match options.open(path) {
        Ok(file) => Ok(file),
        #[cfg(unix)]
        Err(source) if source.raw_os_error() == Some(libc::ELOOP) => {
            Err(TransferError::Symlink(path.to_path_buf()))
        }
        Err(source) => Err(TransferError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn validate_metadata(path: &Path, metadata: &std::fs::Metadata) -> Result<(), TransferError> {
    if !metadata.is_file() {
        return Err(TransferError::NotRegularFile(path.to_path_buf()));
    }
    if metadata.len() > MAX_IMPORT_BYTES {
        return Err(TransferError::TooLarge {
            actual: metadata.len(),
            maximum: MAX_IMPORT_BYTES,
        });
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o002 != 0 {
        return Err(TransferError::WorldWritable(path.to_path_buf()));
    }
    Ok(())
}

#[cfg(unix)]
fn same_file(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(unix)]
fn same_snapshot(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    left.len() == right.len()
        && left.mtime() == right.mtime()
        && left.mtime_nsec() == right.mtime_nsec()
}

#[cfg(not(unix))]
fn same_file(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    left.len() == right.len()
        && left
            .modified()
            .ok()
            .zip(right.modified().ok())
            .is_none_or(|(a, b)| a == b)
}

#[cfg(not(unix))]
fn same_snapshot(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    left.len() == right.len()
        && left
            .modified()
            .ok()
            .zip(right.modified().ok())
            .is_none_or(|(a, b)| a == b)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn test_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "auto-reverse-transfer-{name}-{}-{nanos}.toml",
            process::id()
        ))
    }

    #[test]
    fn secure_reader_rejects_oversized_file() {
        let path = test_path("large");
        fs::write(&path, vec![b'x'; MAX_IMPORT_BYTES as usize + 1]).unwrap();

        let error = read_secure_file(&path).unwrap_err();

        assert!(matches!(error, TransferError::TooLarge { .. }));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn secure_reader_accepts_an_ordinary_bounded_regular_file() {
        let path = test_path("valid");
        fs::write(&path, "config_version = 1\n").unwrap();

        let document = read_secure_file(&path).unwrap();

        assert_eq!(document, "config_version = 1\n");
        let _ = fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn secure_reader_rejects_symlink_and_world_writable_source() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let original = test_path("original");
        let link = test_path("link");
        fs::write(&original, "config_version = 1\n").unwrap();
        symlink(&original, &link).unwrap();

        assert!(matches!(
            read_secure_file(&link).unwrap_err(),
            TransferError::Symlink(_)
        ));
        assert!(matches!(
            open_without_following(&link).unwrap_err(),
            TransferError::Symlink(_)
        ));

        let mut permissions = fs::metadata(&original).unwrap().permissions();
        permissions.set_mode(0o666);
        fs::set_permissions(&original, permissions).unwrap();
        assert!(matches!(
            read_secure_file(&original).unwrap_err(),
            TransferError::WorldWritable(_)
        ));

        let _ = fs::remove_file(link);
        let _ = fs::remove_file(original);
    }
}
