//! Local control channel for reloading the active event-tap configuration.
//!
//! The GUI normally shares its `AppConfig` directly with the tap thread, but
//! the supported headless `run` command lives in another process. A private
//! Unix datagram socket gives every successful config writer the same bounded,
//! event-driven way to ask whichever process owns `run.lock` to reload the
//! validated TOML snapshot. The payload contains no config or device data.

use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::config::{AppConfig, ConfigStore};
use crate::error::{AppError, AppResult};

use super::daemon_lock;

const SOCKET_FILE_NAME: &str = "runtime-control.sock";
const RELOAD_COMMAND: &[u8] = b"reload-config-v1";
const MAX_COMMAND_BYTES: usize = 64;
const RECEIVE_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Clone, Copy)]
struct SocketIdentity {
    device: u64,
    inode: u64,
}

/// Owns the event-driven reload endpoint for the process holding `run.lock`.
/// Dropping it wakes and joins the listener before removing its socket path.
pub struct ConfigReloadListener {
    path: PathBuf,
    identity: SocketIdentity,
    stopping: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ConfigReloadListener {
    pub fn start(config: Arc<RwLock<AppConfig>>) -> AppResult<Self> {
        Self::start_at(default_path(), ConfigStore::default(), config)
    }

    fn start_at(
        path: PathBuf,
        store: ConfigStore,
        config: Arc<RwLock<AppConfig>>,
    ) -> AppResult<Self> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|source| {
                AppError::io("create runtime control directory", parent, source)
            })?;
        }

        remove_stale_socket(&path)?;
        let socket = UnixDatagram::bind(&path)
            .map_err(|source| AppError::io("bind runtime control socket", &path, source))?;
        if let Err(source) = fs::set_permissions(&path, fs::Permissions::from_mode(0o600)) {
            let _ = fs::remove_file(&path);
            return Err(AppError::io(
                "set runtime control socket permissions",
                &path,
                source,
            ));
        }
        if let Err(source) = socket.set_read_timeout(Some(RECEIVE_TIMEOUT)) {
            let _ = fs::remove_file(&path);
            return Err(AppError::io(
                "set runtime control socket timeout",
                &path,
                source,
            ));
        }

        let metadata = fs::symlink_metadata(&path)
            .map_err(|source| AppError::io("inspect runtime control socket", &path, source))?;
        let identity = SocketIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        };

        // Close the only startup race: a writer may have committed after the
        // caller loaded config but before this endpoint became reachable.
        if let Err(error) = reload_config(&store, &config) {
            eprintln!("auto-reverse: initial live config reload failed ({error})");
        }

        let stopping = Arc::new(AtomicBool::new(false));
        let thread_stopping = Arc::clone(&stopping);
        let thread_path = path.clone();
        let thread = match thread::Builder::new()
            .name("auto-reverse-config-reload".to_string())
            .spawn(move || listen(socket, thread_path, store, config, thread_stopping))
        {
            Ok(thread) => thread,
            Err(source) => {
                remove_owned_socket(&path, identity);
                return Err(AppError::io(
                    "start runtime config reload listener",
                    &path,
                    source,
                ));
            }
        };

        Ok(Self {
            path,
            identity,
            stopping,
            thread: Some(thread),
        })
    }
}

impl Drop for ConfigReloadListener {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        // A local datagram wakes recv immediately; the timeout remains a
        // bounded fallback if the path was externally removed.
        let _ = request_reload_at(&self.path);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        remove_owned_socket(&self.path, self.identity);
    }
}

pub fn default_path() -> PathBuf {
    daemon_lock::default_path().with_file_name(SOCKET_FILE_NAME)
}

/// Requests a reload from the active tap owner. `Ok(false)` means no current
/// runtime endpoint exists; persisting config is still successful in that
/// case and the next runtime start will load the new snapshot.
pub fn request_reload_if_running() -> AppResult<bool> {
    request_reload_at(&default_path())
}

fn request_reload_at(path: &Path) -> AppResult<bool> {
    let socket = UnixDatagram::unbound()
        .map_err(|source| AppError::io("create runtime control client", path, source))?;
    match socket.send_to(RELOAD_COMMAND, path) {
        Ok(written) => Ok(written == RELOAD_COMMAND.len()),
        Err(source)
            if matches!(
                source.kind(),
                ErrorKind::NotFound | ErrorKind::ConnectionRefused
            ) =>
        {
            Ok(false)
        }
        Err(source) => Err(AppError::io("notify runtime config reload", path, source)),
    }
}

fn listen(
    socket: UnixDatagram,
    path: PathBuf,
    store: ConfigStore,
    config: Arc<RwLock<AppConfig>>,
    stopping: Arc<AtomicBool>,
) {
    let mut command = [0_u8; MAX_COMMAND_BYTES];
    while !stopping.load(Ordering::Acquire) {
        match socket.recv(&mut command) {
            Ok(length) => {
                if stopping.load(Ordering::Acquire) {
                    break;
                }
                if command.get(..length) == Some(RELOAD_COMMAND)
                    && let Err(error) = reload_config(&store, &config)
                {
                    eprintln!("auto-reverse: live config reload failed ({error})");
                }
            }
            Err(source)
                if matches!(
                    source.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) => {}
            Err(source) => {
                eprintln!(
                    "auto-reverse: runtime control socket `{}` stopped ({source})",
                    path.display()
                );
                break;
            }
        }
    }
}

fn reload_config(store: &ConfigStore, config: &RwLock<AppConfig>) -> AppResult<()> {
    let reloaded = store.load()?;
    let mut current = match config.write() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *current = reloaded;
    Ok(())
}

fn remove_stale_socket(path: &Path) -> AppResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => fs::remove_file(path)
            .map_err(|source| AppError::io("remove stale runtime control socket", path, source)),
        Ok(_) => Err(AppError::Platform(format!(
            "refusing to replace non-socket runtime control path `{}`",
            path.display()
        ))),
        Err(source) if source.kind() == ErrorKind::NotFound => Ok(()),
        Err(source) => Err(AppError::io("inspect runtime control socket", path, source)),
    }
}

fn remove_owned_socket(path: &Path, identity: SocketIdentity) {
    let owns_current_path = fs::symlink_metadata(path).is_ok_and(|metadata| {
        metadata.file_type().is_socket()
            && metadata.dev() == identity.device
            && metadata.ino() == identity.inode
    });
    if owns_current_path {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    use super::*;

    static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(0);

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "auto-reverse-runtime-ipc-{name}-{}-{}",
            std::process::id(),
            NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn missing_runtime_is_a_normal_noop() {
        let root = test_root("missing");

        assert!(!request_reload_at(&root.join("missing.sock")).unwrap());
    }

    #[test]
    fn listener_reconciles_startup_and_applies_signalled_reload() {
        let root = test_root("reload");
        let store = ConfigStore::new(root.join("config.toml"));
        store
            .update(|config| config.reverse_vertical = false)
            .unwrap();
        let shared = Arc::new(RwLock::new(AppConfig::default()));
        let socket_path = root.join("runtime.sock");

        let listener =
            ConfigReloadListener::start_at(socket_path.clone(), store.clone(), Arc::clone(&shared))
                .unwrap();
        assert!(!shared.read().unwrap().reverse_vertical);
        assert_eq!(
            fs::symlink_metadata(&socket_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        store
            .update(|config| config.reverse_vertical = true)
            .unwrap();
        assert!(request_reload_at(&socket_path).unwrap());

        let deadline = Instant::now() + Duration::from_secs(2);
        while !shared.read().unwrap().reverse_vertical && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(shared.read().unwrap().reverse_vertical);

        drop(listener);
        assert!(!socket_path.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn listener_never_replaces_a_non_socket_path() {
        let root = test_root("regular-file");
        fs::create_dir_all(&root).unwrap();
        let socket_path = root.join("runtime.sock");
        fs::write(&socket_path, b"keep me").unwrap();
        let store = ConfigStore::new(root.join("config.toml"));
        store.update(|_| {}).unwrap();

        let result = ConfigReloadListener::start_at(
            socket_path.clone(),
            store,
            Arc::new(RwLock::new(AppConfig::default())),
        );

        assert!(result.is_err());
        assert_eq!(fs::read(&socket_path).unwrap(), b"keep me");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_external_config_is_rejected_without_stopping_the_listener() {
        let root = test_root("invalid-config");
        let store = ConfigStore::new(root.join("config.toml"));
        store
            .update(|config| config.reverse_vertical = false)
            .unwrap();
        let shared = Arc::new(RwLock::new(AppConfig::default()));
        let socket_path = root.join("runtime.sock");
        let listener =
            ConfigReloadListener::start_at(socket_path.clone(), store.clone(), Arc::clone(&shared))
                .unwrap();

        fs::write(store.path(), b"reverse_vertical = [invalid").unwrap();
        assert!(request_reload_at(&socket_path).unwrap());
        thread::sleep(Duration::from_millis(50));
        assert!(!shared.read().unwrap().reverse_vertical);

        let recovered = AppConfig {
            reverse_vertical: true,
            ..AppConfig::default()
        };
        fs::write(store.path(), toml::to_string_pretty(&recovered).unwrap()).unwrap();
        assert!(request_reload_at(&socket_path).unwrap());
        let deadline = Instant::now() + Duration::from_secs(2);
        while !shared.read().unwrap().reverse_vertical && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(shared.read().unwrap().reverse_vertical);

        drop(listener);
        let _ = fs::remove_dir_all(root);
    }
}
