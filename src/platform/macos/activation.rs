//! Small cross-process mailbox for focusing the existing settings window.
//!
//! `ui.lock` remains the single-instance authority. When a second GUI process
//! cannot acquire it, that process commits any requested recovery change,
//! atomically writes the lock owner's PID to a sibling `ui.activate` file, and
//! exits successfully. The owner polls this file from egui's existing 250 ms
//! logic tick, consumes matching requests, reloads config, and makes its hidden
//! settings viewport visible and focused.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{AppError, AppResult};

use super::daemon_lock;

#[derive(Clone)]
pub struct ActivationInbox {
    path: PathBuf,
    target_pid: u32,
}

/// Keeps `ui.lock` held for the primary GUI and owns the matching activation
/// inbox. Dropping this guard allows a later launch to become primary.
pub struct PrimaryUiInstance {
    _lock: daemon_lock::DaemonLock,
    inbox: ActivationInbox,
}

impl PrimaryUiInstance {
    pub fn inbox(&self) -> ActivationInbox {
        self.inbox.clone()
    }
}

/// Atomically chooses between becoming the primary GUI and activating the
/// existing one. `Ok(None)` means the request was published successfully and
/// this second process should exit without constructing AppKit objects.
pub fn acquire_or_activate(lock_path: &Path) -> AppResult<Option<PrimaryUiInstance>> {
    acquire_or_activate_after(lock_path, || Ok(()))
}

/// Like [`acquire_or_activate`], but commits a recovery change before the
/// activation request becomes visible to the owner. This ordering prevents
/// the primary GUI from consuming the request and reloading the old config.
pub fn acquire_or_activate_after(
    lock_path: &Path,
    before_activate: impl FnOnce() -> AppResult<()>,
) -> AppResult<Option<PrimaryUiInstance>> {
    match daemon_lock::try_acquire(lock_path)? {
        Some(lock) => Ok(Some(PrimaryUiInstance {
            _lock: lock,
            inbox: ActivationInbox::for_lock(lock_path),
        })),
        None => {
            before_activate()?;
            request_existing_window(lock_path)?;
            Ok(None)
        }
    }
}

/// Asks a running primary GUI to reopen its window without claiming ownership
/// when no GUI exists. This is the recovery path used after an external CLI
/// edit restores the menu-bar icon: a live owner reloads the config on receipt,
/// while a later ordinary launch reads the already-persisted value itself.
pub fn request_existing_window_if_running(lock_path: &Path) -> AppResult<bool> {
    match daemon_lock::try_acquire(lock_path)? {
        Some(lock) => {
            drop(lock);
            Ok(false)
        }
        None => {
            request_existing_window(lock_path)?;
            Ok(true)
        }
    }
}

impl ActivationInbox {
    pub fn for_lock(lock_path: &Path) -> Self {
        Self {
            path: activation_path(lock_path),
            target_pid: process::id(),
        }
    }

    /// Consumes at most one request. A stale request addressed to an older
    /// process is removed but never activates the current window.
    pub fn poll(&self) -> AppResult<bool> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(source) if source.kind() == ErrorKind::NotFound => return Ok(false),
            Err(source) => {
                return Err(AppError::io(
                    "read UI activation request",
                    &self.path,
                    source,
                ));
            }
        };
        let parsed = contents.trim().parse::<u32>();

        match fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(source) if source.kind() == ErrorKind::NotFound => {}
            Err(source) => {
                return Err(AppError::io(
                    "consume UI activation request",
                    &self.path,
                    source,
                ));
            }
        }

        let target_pid = parsed.map_err(|_| {
            AppError::Platform(format!(
                "invalid UI activation request in `{}`",
                self.path.display()
            ))
        })?;
        Ok(target_pid == self.target_pid)
    }
}

/// Leaves one idempotent activation request for the process holding
/// `ui.lock`. The PID is only routing data; `flock`, not this file, remains the
/// authority for deciding which process owns the GUI.
fn request_existing_window(lock_path: &Path) -> AppResult<()> {
    let target_pid = daemon_lock::recorded_pid(lock_path)
        .filter(|pid| *pid > 0)
        .ok_or_else(|| {
            AppError::Platform(format!(
                "could not read the owner PID from `{}`",
                lock_path.display()
            ))
        })? as u32;
    write_request(&activation_path(lock_path), target_pid)
}

fn activation_path(lock_path: &Path) -> PathBuf {
    lock_path.with_extension("activate")
}

fn write_request(path: &Path, target_pid: u32) -> AppResult<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|source| AppError::io("create UI activation directory", parent, source))?;
    }

    let request_id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let temp_path = path.with_extension(format!("activate.{}.{}.tmp", process::id(), request_id));
    fs::write(&temp_path, format!("{target_pid}\n")).map_err(|source| {
        let _ = fs::remove_file(&temp_path);
        AppError::io("write temporary UI activation request", &temp_path, source)
    })?;
    fs::rename(&temp_path, path).map_err(|source| {
        let _ = fs::remove_file(&temp_path);
        AppError::io("publish UI activation request", path, source)
    })
}

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
mod tests {
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn test_lock_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!("auto-reverse-activation-{name}-{nanos}.lock"))
    }

    fn cleanup(lock_path: &Path) {
        let _ = fs::remove_file(lock_path);
        let _ = fs::remove_file(activation_path(lock_path));
    }

    #[test]
    fn activation_file_is_a_sibling_of_the_ui_lock() {
        let lock_path = Path::new("/tmp/Auto Reverse/ui.lock");

        assert_eq!(
            activation_path(lock_path),
            Path::new("/tmp/Auto Reverse/ui.activate")
        );
    }

    #[test]
    fn matching_request_is_consumed_once() {
        let lock_path = test_lock_path("matching");
        let primary = acquire_or_activate(&lock_path).unwrap().unwrap();

        assert!(acquire_or_activate(&lock_path).unwrap().is_none());

        assert!(primary.inbox().poll().unwrap());
        assert!(!primary.inbox().poll().unwrap());
        drop(primary);
        cleanup(&lock_path);
    }

    #[test]
    fn recovery_request_is_only_published_for_a_live_owner() {
        let lock_path = test_lock_path("recovery");

        assert!(!request_existing_window_if_running(&lock_path).unwrap());
        let primary = acquire_or_activate(&lock_path).unwrap().unwrap();
        assert!(request_existing_window_if_running(&lock_path).unwrap());
        assert!(primary.inbox().poll().unwrap());

        drop(primary);
        cleanup(&lock_path);
    }

    #[test]
    fn recovery_change_is_committed_before_activation_is_published() {
        let lock_path = test_lock_path("ordered-recovery");
        let primary = acquire_or_activate(&lock_path).unwrap().unwrap();
        let changed = std::cell::Cell::new(false);

        assert!(
            acquire_or_activate_after(&lock_path, || {
                assert!(!activation_path(&lock_path).exists());
                changed.set(true);
                Ok(())
            })
            .unwrap()
            .is_none()
        );
        assert!(changed.get());
        assert!(primary.inbox().poll().unwrap());

        drop(primary);
        cleanup(&lock_path);
    }

    #[test]
    fn primary_launch_does_not_apply_secondary_recovery_change() {
        let lock_path = test_lock_path("primary-keeps-config");
        let changed = std::cell::Cell::new(false);

        let primary = acquire_or_activate_after(&lock_path, || {
            changed.set(true);
            Ok(())
        })
        .unwrap()
        .unwrap();
        assert!(!changed.get());

        drop(primary);
        cleanup(&lock_path);
    }

    #[test]
    fn stale_request_is_consumed_without_activating() {
        let lock_path = test_lock_path("stale");
        let inbox = ActivationInbox::for_lock(&lock_path);
        write_request(&activation_path(&lock_path), process::id() + 1).unwrap();

        assert!(!inbox.poll().unwrap());
        assert!(!activation_path(&lock_path).exists());
        cleanup(&lock_path);
    }

    #[test]
    fn malformed_request_is_removed_and_reported_once() {
        let lock_path = test_lock_path("malformed");
        let request_path = activation_path(&lock_path);
        fs::write(&request_path, "not-a-pid\n").unwrap();
        let inbox = ActivationInbox::for_lock(&lock_path);

        assert!(inbox.poll().is_err());
        assert!(!inbox.poll().unwrap());
        cleanup(&lock_path);
    }
}
