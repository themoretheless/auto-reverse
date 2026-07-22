//! Small cross-process mailbox for controlling the existing settings window.
//!
//! `ui.lock` remains the single-instance authority. When a second GUI process
//! cannot acquire it, that process commits any requested recovery change,
//! atomically writes the lock owner's PID plus a bounded action to a sibling
//! `ui.activate` file, and exits successfully. The owner polls this file from
//! egui's existing 250 ms logic tick and consumes matching requests. Ordinary
//! CLI edits request a config reload without changing window visibility, while
//! a second GUI launch requests both a reload and activation.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{AppError, AppResult};

use super::daemon_lock;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActivationAction {
    ReloadOnly,
    ReloadAndOpen,
}

impl ActivationAction {
    fn token(self) -> &'static str {
        match self {
            Self::ReloadOnly => "reload",
            Self::ReloadAndOpen => "reload-and-open",
        }
    }

    fn from_token(token: &str) -> Option<Self> {
        match token {
            "reload" => Some(Self::ReloadOnly),
            "reload-and-open" => Some(Self::ReloadAndOpen),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ActivationRequest {
    target_pid: u32,
    action: ActivationAction,
}

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
            request_existing_window(lock_path, ActivationAction::ReloadAndOpen)?;
            Ok(None)
        }
    }
}

/// Asks a running primary GUI to reopen its window without claiming ownership
/// when no GUI exists. This is the recovery path used after an external CLI
/// edit restores the menu-bar icon: a live owner reloads the config on receipt,
/// while a later ordinary launch reads the already-persisted value itself.
pub fn request_existing_window_if_running(lock_path: &Path) -> AppResult<bool> {
    request_existing_gui_if_running(lock_path, ActivationAction::ReloadAndOpen)
}

/// Sends a best-effort-style control request only when a primary GUI owns the
/// lock. The caller decides whether delivery errors are fatal; no request is
/// published and no GUI ownership is claimed when the lock is free.
pub fn request_existing_gui_if_running(
    lock_path: &Path,
    action: ActivationAction,
) -> AppResult<bool> {
    match daemon_lock::try_acquire(lock_path)? {
        Some(lock) => {
            drop(lock);
            Ok(false)
        }
        None => {
            request_existing_window(lock_path, action)?;
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
    /// process is removed but never controls the current window.
    pub fn poll(&self) -> AppResult<Option<ActivationAction>> {
        // Claim the current inode before reading it. A writer that publishes
        // after this rename creates a new `ui.activate` path, so consuming the
        // claimed request cannot accidentally unlink that newer request.
        let claim_id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        let claimed_path = self
            .path
            .with_extension(format!("activate.{}.{}.claimed", self.target_pid, claim_id));
        match fs::rename(&self.path, &claimed_path) {
            Ok(()) => {}
            Err(source) if source.kind() == ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(AppError::io(
                    "claim UI activation request",
                    &self.path,
                    source,
                ));
            }
        }

        let contents = match fs::read_to_string(&claimed_path) {
            Ok(contents) => contents,
            Err(source) => {
                let _ = fs::remove_file(&claimed_path);
                return Err(AppError::io(
                    "read UI activation request",
                    &claimed_path,
                    source,
                ));
            }
        };
        let parsed = parse_request(&contents, &claimed_path);

        match fs::remove_file(&claimed_path) {
            Ok(()) => {}
            Err(source) if source.kind() == ErrorKind::NotFound => {}
            Err(source) => {
                return Err(AppError::io(
                    "consume UI activation request",
                    &claimed_path,
                    source,
                ));
            }
        }

        let request = parsed?;
        Ok((request.target_pid == self.target_pid).then_some(request.action))
    }
}

/// Leaves one idempotent activation request for the process holding
/// `ui.lock`. The PID is only routing data; `flock`, not this file, remains the
/// authority for deciding which process owns the GUI.
fn request_existing_window(lock_path: &Path, action: ActivationAction) -> AppResult<()> {
    let target_pid = daemon_lock::recorded_pid(lock_path)
        .filter(|pid| *pid > 0)
        .ok_or_else(|| {
            AppError::Platform(format!(
                "could not read the owner PID from `{}`",
                lock_path.display()
            ))
        })? as u32;
    write_request(&activation_path(lock_path), target_pid, action)
}

fn activation_path(lock_path: &Path) -> PathBuf {
    lock_path.with_extension("activate")
}

fn parse_request(contents: &str, path: &Path) -> AppResult<ActivationRequest> {
    let mut fields = contents.split_whitespace();
    let target_pid = fields
        .next()
        .and_then(|field| field.parse::<u32>().ok())
        .ok_or_else(|| invalid_request(path))?;

    // PID-only requests were written by releases before typed actions were
    // introduced. Preserve their original reload-and-open behavior so an
    // already-published request survives an in-place app update.
    let action = match fields.next() {
        Some(token) => ActivationAction::from_token(token).ok_or_else(|| invalid_request(path))?,
        None => ActivationAction::ReloadAndOpen,
    };
    if fields.next().is_some() {
        return Err(invalid_request(path));
    }

    Ok(ActivationRequest { target_pid, action })
}

fn invalid_request(path: &Path) -> AppError {
    AppError::Platform(format!(
        "invalid UI activation request in `{}`",
        path.display()
    ))
}

fn write_request(path: &Path, target_pid: u32, action: ActivationAction) -> AppResult<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|source| AppError::io("create UI activation directory", parent, source))?;
    }

    // Coalesce repeated reloads and never let a later background-only edit
    // downgrade a pending request to expose the window. ReloadAndOpen still
    // overwrites ReloadOnly below, so the strongest pending action wins.
    if action == ActivationAction::ReloadOnly
        && fs::read_to_string(path)
            .ok()
            .and_then(|contents| parse_request(&contents, path).ok())
            .is_some_and(|request| request.target_pid == target_pid)
    {
        return Ok(());
    }

    let request_id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let temp_path = path.with_extension(format!("activate.{}.{}.tmp", process::id(), request_id));
    fs::write(&temp_path, format!("{target_pid} {}\n", action.token())).map_err(|source| {
        let _ = fs::remove_file(&temp_path);
        AppError::io("write temporary UI activation request", &temp_path, source)
    })?;
    publish_request(&temp_path, path, target_pid, action).map_err(|source| {
        let _ = fs::remove_file(&temp_path);
        AppError::io("publish UI activation request", path, source)
    })
}

fn publish_request(
    temp_path: &Path,
    path: &Path,
    target_pid: u32,
    action: ActivationAction,
) -> std::io::Result<()> {
    if action == ActivationAction::ReloadOnly {
        match fs::hard_link(temp_path, path) {
            Ok(()) => {
                let _ = fs::remove_file(temp_path);
                return Ok(());
            }
            Err(source) if source.kind() == ErrorKind::AlreadyExists => {
                let same_owner_request_is_pending = fs::read_to_string(path)
                    .ok()
                    .and_then(|contents| parse_request(&contents, path).ok())
                    .is_some_and(|request| request.target_pid == target_pid);
                if same_owner_request_is_pending {
                    let _ = fs::remove_file(temp_path);
                    return Ok(());
                }
            }
            Err(source) => return Err(source),
        }
    }

    // ReloadAndOpen intentionally replaces a pending ReloadOnly. ReloadOnly
    // reaches this rename only when the existing request was stale or invalid.
    fs::rename(temp_path, path)
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

        assert_eq!(
            primary.inbox().poll().unwrap(),
            Some(ActivationAction::ReloadAndOpen)
        );
        assert_eq!(primary.inbox().poll().unwrap(), None);
        drop(primary);
        cleanup(&lock_path);
    }

    #[test]
    fn typed_actions_round_trip_without_becoming_gui_ownership() {
        let lock_path = test_lock_path("typed-actions");
        let primary = acquire_or_activate(&lock_path).unwrap().unwrap();

        assert!(request_existing_gui_if_running(&lock_path, ActivationAction::ReloadOnly).unwrap());
        assert_eq!(
            primary.inbox().poll().unwrap(),
            Some(ActivationAction::ReloadOnly)
        );

        assert!(
            request_existing_gui_if_running(&lock_path, ActivationAction::ReloadAndOpen).unwrap()
        );
        assert_eq!(
            primary.inbox().poll().unwrap(),
            Some(ActivationAction::ReloadAndOpen)
        );

        drop(primary);
        cleanup(&lock_path);
    }

    #[test]
    fn reload_only_cannot_downgrade_a_pending_open_request() {
        let lock_path = test_lock_path("open-dominates-reload");
        let primary = acquire_or_activate(&lock_path).unwrap().unwrap();

        request_existing_gui_if_running(&lock_path, ActivationAction::ReloadAndOpen).unwrap();
        request_existing_gui_if_running(&lock_path, ActivationAction::ReloadOnly).unwrap();

        assert_eq!(
            primary.inbox().poll().unwrap(),
            Some(ActivationAction::ReloadAndOpen)
        );
        drop(primary);
        cleanup(&lock_path);
    }

    #[test]
    fn reload_and_open_upgrades_a_pending_reload_only_request() {
        let lock_path = test_lock_path("open-upgrades-reload");
        let primary = acquire_or_activate(&lock_path).unwrap().unwrap();

        request_existing_gui_if_running(&lock_path, ActivationAction::ReloadOnly).unwrap();
        request_existing_gui_if_running(&lock_path, ActivationAction::ReloadAndOpen).unwrap();

        assert_eq!(
            primary.inbox().poll().unwrap(),
            Some(ActivationAction::ReloadAndOpen)
        );
        drop(primary);
        cleanup(&lock_path);
    }

    #[test]
    fn recovery_request_is_only_published_for_a_live_owner() {
        let lock_path = test_lock_path("recovery");

        assert!(!request_existing_window_if_running(&lock_path).unwrap());
        let primary = acquire_or_activate(&lock_path).unwrap().unwrap();
        assert!(request_existing_window_if_running(&lock_path).unwrap());
        assert_eq!(
            primary.inbox().poll().unwrap(),
            Some(ActivationAction::ReloadAndOpen)
        );

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
        assert_eq!(
            primary.inbox().poll().unwrap(),
            Some(ActivationAction::ReloadAndOpen)
        );

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
        write_request(
            &activation_path(&lock_path),
            process::id() + 1,
            ActivationAction::ReloadOnly,
        )
        .unwrap();

        assert_eq!(inbox.poll().unwrap(), None);
        assert!(!activation_path(&lock_path).exists());
        cleanup(&lock_path);
    }

    #[test]
    fn legacy_pid_only_request_keeps_reload_and_open_behavior() {
        let lock_path = test_lock_path("legacy");
        let request_path = activation_path(&lock_path);
        fs::write(&request_path, format!("{}\n", process::id())).unwrap();
        let inbox = ActivationInbox::for_lock(&lock_path);

        assert_eq!(inbox.poll().unwrap(), Some(ActivationAction::ReloadAndOpen));
        cleanup(&lock_path);
    }

    #[test]
    fn malformed_request_is_removed_and_reported_once() {
        let lock_path = test_lock_path("malformed");
        let request_path = activation_path(&lock_path);
        fs::write(&request_path, "not-a-pid\n").unwrap();
        let inbox = ActivationInbox::for_lock(&lock_path);

        assert!(inbox.poll().is_err());
        assert_eq!(inbox.poll().unwrap(), None);
        cleanup(&lock_path);
    }

    #[test]
    fn unknown_action_is_removed_and_reported_once() {
        let lock_path = test_lock_path("unknown-action");
        let request_path = activation_path(&lock_path);
        fs::write(&request_path, format!("{} surprise\n", process::id())).unwrap();
        let inbox = ActivationInbox::for_lock(&lock_path);

        assert!(inbox.poll().is_err());
        assert_eq!(inbox.poll().unwrap(), None);
        cleanup(&lock_path);
    }
}
