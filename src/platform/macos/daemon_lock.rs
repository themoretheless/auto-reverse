//! Exclusive-lock guard so the `run` daemon is never started twice.
//!
//! Two independent entry points can both want to start the daemon: a user
//! typing `auto-reverse run`, the LaunchAgent installed by `startup.rs`, and
//! the settings window (`ui.rs`) auto-starting it when the app is opened.
//! Two live `CGEventTap`s on the same scroll stream both rewrite the same
//! events - the second tap "reverses" an already-reversed event, which
//! silently cancels the effect (or worse, behaves inconsistently depending
//! on tap ordering). This is a correctness bug, not a performance concern.
//!
//! This was originally a pid-file: write the current pid, and have every
//! spawner read-and-check-liveness before deciding to start a daemon. That
//! has a real race - the check and the write are two separate steps with an
//! arbitrarily long gap between them (config load, permission prompts, etc.
//! can all happen in between), so two processes can both observe "no daemon
//! running" before either one records itself. `flock` closes this: acquiring
//! the lock IS the atomic decision, there is no separate "check, then
//! write" pair of steps to race between, and the kernel releases the lock
//! automatically on any process exit - including a crash or `kill -9` - so
//! there is no stale-lock file to ever misdetect as "still running".
//!
//! The lock file lives as a sibling of the config file (same directory),
//! which keeps it inside the same `AUTO_REVERSE_CONFIG`-driven isolation the
//! config store already uses - tests overriding that env var never touch
//! `~/Library/Application Support/Auto Reverse`.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{Duration, Instant};

use crate::config::ConfigStore;
use crate::error::{AppError, AppResult};

const LOCK_FILE_NAME: &str = "run.lock";

// From libSystem, always linked. LOCK_EX | LOCK_NB atomically tries to take
// an exclusive advisory lock and fails immediately (rather than blocking) if
// another open file description already holds it. `kill` with SIGTERM asks
// a process to exit; unlike the old pid-file design, the pid recorded here
// is never used to decide mutual exclusion (flock alone does that) - it
// only exists so a "Restart" action can find the right process to signal.
unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
}

const LOCK_EX: i32 = 2;
const LOCK_NB: i32 = 4;
const SIGTERM: i32 = 15;

/// Where the daemon's lock file lives: a sibling of the config file, so it
/// moves with `AUTO_REVERSE_CONFIG` overrides exactly like
/// `ConfigStore::default_path()` does.
pub fn default_path() -> PathBuf {
    match ConfigStore::default_path().parent() {
        Some(parent) => parent.join(LOCK_FILE_NAME),
        None => PathBuf::from(LOCK_FILE_NAME),
    }
}

/// An acquired exclusive lock. Holding this value IS being the one true
/// `run` daemon for this config directory. Keep it alive for as long as the
/// daemon should be considered running - dropping it (or the process
/// exiting for any reason) releases the lock immediately.
pub struct DaemonLock {
    _file: File,
}

/// Tries to become the one true daemon. `Ok(None)` means another live `run`
/// already holds the lock - callers should treat that as a normal, expected
/// outcome (print a message and exit cleanly), not an error. Call this as
/// the very first thing `run` does, before any config load or permission
/// check, so the mutual-exclusion decision does not have to wait on
/// anything that could block (a TCC prompt, disk I/O) - the whole point is
/// that the decision is a single atomic syscall, not a read-then-write pair
/// with an arbitrarily long gap in between.
pub fn try_acquire(path: &Path) -> AppResult<Option<DaemonLock>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|source| AppError::io("create daemon lock directory", parent, source))?;
    }

    // truncate(false): the mutual-exclusion decision never depends on this
    // file's contents, only on flock state - so there is nothing to gain
    // from clearing it up front, and no reason to invite a clippy warning
    // about ambiguous intent. The pid IS written below, but only after this
    // process has confirmed it is the sole lock holder.
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .map_err(|source| AppError::io("open daemon lock file", path, source))?;

    if unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) } == 0 {
        // Best-effort only: a "Restart" action reads this to find which
        // process to signal. If the write fails for some reason, the lock
        // is still correctly held - restart just would not find a pid to
        // terminate, which is a lesser problem than not holding the lock.
        use std::io::Seek as _;
        let _ = file.rewind();
        let _ = file.set_len(0);
        let _ = write!(file, "{}", process::id());
        Ok(Some(DaemonLock { _file: file }))
    } else {
        Ok(None)
    }
}

/// Reports whether a `run` daemon currently holds the lock, without taking
/// it. Deliberately distinct from `try_acquire`: a naive "peek" that just
/// calls `try_acquire` and drops the result on the floor would still
/// briefly hold the exclusive lock itself for the duration of the call,
/// which is harmless for a single check but would be wrong to build a
/// polling status display on. This instead takes the lock only if nobody
/// else has it, and unlocks again immediately before returning, so a
/// caller polling this every frame (as the settings window's status
/// display does) never contends with the real daemon acquiring it.
pub fn is_running(path: &Path) -> bool {
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return false;
    }

    let Ok(file) = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
    else {
        return false;
    };

    let we_got_it = unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) } == 0;
    // Dropping `file` here closes our file descriptor, which releases any
    // lock we just took - we were only ever checking, not claiming it.
    !we_got_it
}

/// The pid `try_acquire`'s current lock holder recorded, if any. Purely
/// informational - never used to decide mutual exclusion (flock alone does
/// that) - so a missing or unparseable value just means "nothing to signal
/// for a restart", not an error.
pub(super) fn recorded_pid(path: &Path) -> Option<i32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Asks whichever daemon currently holds the lock to exit (`SIGTERM`, the
/// default disposition of which is to terminate - `run` installs no signal
/// handler of its own), then polls `is_running` until the lock is free or
/// `timeout` elapses. Used to back a "Restart" action: config changes only
/// take effect for a daemon that (re)reads the config file at startup, and
/// an already-running one keeps whatever it loaded until it exits.
///
/// Returns true once the lock is confirmed free (including if it was
/// already free - nothing to do). Returns false if a daemon is still
/// holding the lock after `timeout` - the caller should not treat this as
/// safe to spawn a replacement into, since the old one may still be alive.
pub fn terminate_and_wait(path: &Path, timeout: Duration) -> bool {
    if !is_running(path) {
        return true;
    }

    if let Some(pid) = recorded_pid(path) {
        unsafe { kill(pid, SIGTERM) };
    }

    let deadline = Instant::now() + timeout;
    while is_running(path) {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    true
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn test_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!("auto-reverse-{name}-{nanos}.lock"))
    }

    #[test]
    fn first_caller_acquires_the_lock() {
        let path = test_path("first-caller");

        let lock = try_acquire(&path).unwrap();

        assert!(lock.is_some());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_second_caller_is_refused_while_the_first_still_holds_it() {
        let path = test_path("second-caller-refused");

        let first = try_acquire(&path).unwrap();
        assert!(first.is_some());

        let second = try_acquire(&path).unwrap();
        assert!(
            second.is_none(),
            "a second acquire must fail while the first lock is still held"
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn dropping_the_lock_releases_it_for_the_next_caller() {
        let path = test_path("released-on-drop");

        let first = try_acquire(&path).unwrap();
        assert!(first.is_some());
        drop(first);

        let second = try_acquire(&path).unwrap();
        assert!(
            second.is_some(),
            "dropping the first lock must release it immediately"
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn is_running_reflects_whether_a_lock_is_currently_held() {
        let path = test_path("is-running");

        assert!(!is_running(&path), "no one holds the lock yet");

        let held = try_acquire(&path).unwrap();
        assert!(held.is_some());
        assert!(
            is_running(&path),
            "the lock is held, so this should be true"
        );

        drop(held);
        assert!(
            !is_running(&path),
            "released, so this should be false again"
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn checking_is_running_does_not_itself_take_the_lock() {
        let path = test_path("peek-does-not-hold");

        assert!(!is_running(&path));

        // If the check above had left the lock held, this would fail.
        let lock = try_acquire(&path).unwrap();
        assert!(
            lock.is_some(),
            "is_running must release any lock it took while just checking"
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn try_acquire_records_its_own_pid() {
        let path = test_path("records-own-pid");

        let lock = try_acquire(&path).unwrap();
        assert!(lock.is_some());

        assert_eq!(recorded_pid(&path), Some(process::id() as i32));

        drop(lock);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn terminate_and_wait_returns_true_immediately_when_nothing_is_running() {
        let path = test_path("terminate-nothing-running");

        assert!(terminate_and_wait(&path, Duration::from_millis(100)));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn default_path_is_a_sibling_of_the_config_file() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let config_path = env::temp_dir().join(format!("auto-reverse-cfg-{nanos}/config.toml"));
        // SAFETY: test-only env mutation; no other thread in this test
        // binary reads AUTO_REVERSE_CONFIG concurrently with this test.
        unsafe {
            env::set_var("AUTO_REVERSE_CONFIG", &config_path);
        }

        let lock_path = default_path();

        unsafe {
            env::remove_var("AUTO_REVERSE_CONFIG");
        }

        assert_eq!(lock_path, config_path.parent().unwrap().join("run.lock"));
    }
}
