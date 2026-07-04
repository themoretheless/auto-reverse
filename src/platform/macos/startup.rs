//! Start-at-login support for the current CLI/headless binary path.
//!
//! The bundled GUI app uses `login_item.rs` / `SMAppService.mainAppService()`;
//! this module deliberately remains the CLI mechanism. It writes a per-user
//! LaunchAgent that starts this exact executable with the `run` argument on
//! the next login, which is useful for lean/no-GUI builds and terminal-driven
//! installs.
//!
//! "Installed" here means exactly that: the agent file exists in
//! ~/Library/LaunchAgents. macOS can still veto it (the Login Items toggle
//! in System Settings, or a `launchctl disable` override) - we do not query
//! launchd's own registration state.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command};

use crate::error::{AppError, AppResult};

const LABEL: &str = "com.auto-reverse.agent";
const PLIST_FILE: &str = "com.auto-reverse.agent.plist";
const LOG_FILE: &str = "auto-reverse.log";

// From libSystem, always linked; needed to address the gui/<uid> launchd
// domain when booting the agent out.
unsafe extern "C" {
    fn getuid() -> u32;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupStatus {
    pub installed: bool,
    pub agent_path: PathBuf,
    pub configured_for_current_exe: bool,
}

impl StartupStatus {
    pub fn summary(&self) -> String {
        match (self.installed, self.configured_for_current_exe) {
            (true, true) => format!("installed for this binary ({})", self.agent_path.display()),
            (true, false) => format!(
                "installed, but points at a different or unreadable target ({})",
                self.agent_path.display()
            ),
            (false, _) => format!("not installed ({})", self.agent_path.display()),
        }
    }
}

pub fn current_executable() -> AppResult<PathBuf> {
    env::current_exe().map_err(|source| {
        AppError::io("resolve current executable", "<current executable>", source)
    })
}

pub fn status_for_current_executable() -> AppResult<StartupStatus> {
    status_for_executable(&current_executable()?)
}

pub fn status_for_executable(executable: &Path) -> AppResult<StartupStatus> {
    status_at(&launch_agent_path()?, executable)
}

fn status_at(agent_path: &Path, executable: &Path) -> AppResult<StartupStatus> {
    if !agent_path.exists() {
        return Ok(StartupStatus {
            installed: false,
            agent_path: agent_path.to_path_buf(),
            configured_for_current_exe: false,
        });
    }

    // A management tool may have rewritten the agent as a binary plist
    // (launchd accepts that fine); treat anything we cannot parse as
    // "installed, target unknown" instead of failing the whole command.
    let configured_for_current_exe = fs::read(agent_path)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|contents| parse_program_path(&contents))
        .is_some_and(|configured| same_file(&configured, executable));

    Ok(StartupStatus {
        installed: true,
        agent_path: agent_path.to_path_buf(),
        configured_for_current_exe,
    })
}

pub fn enable_for_current_executable() -> AppResult<StartupStatus> {
    enable_for_executable(&current_executable()?)
}

pub fn enable_for_executable(executable: &Path) -> AppResult<StartupStatus> {
    enable_at(&launch_agent_path()?, &log_path()?, executable)
}

fn enable_at(agent_path: &Path, log_path: &Path, executable: &Path) -> AppResult<StartupStatus> {
    if let Some(parent) = agent_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|source| AppError::io("create LaunchAgents directory", parent, source))?;
    }

    let plist = plist_document(executable, log_path);
    let tmp_path = agent_path.with_extension(format!("plist.{}.tmp", process::id()));
    fs::write(&tmp_path, plist).map_err(|source| {
        let _ = fs::remove_file(&tmp_path);
        AppError::io("write temporary launch agent", &tmp_path, source)
    })?;
    fs::rename(&tmp_path, agent_path).map_err(|source| {
        let _ = fs::remove_file(&tmp_path);
        AppError::io("install launch agent", agent_path, source)
    })?;

    status_at(agent_path, executable)
}

/// Removes the agent file AND boots the launchd-managed instance out of the
/// gui domain. Deleting the plist alone is not enough: launchd only reads
/// LaunchAgents at login, so a job bootstrapped at the last login would
/// keep running (and keep reversing scroll) until logout.
pub fn disable() -> AppResult<StartupStatus> {
    let executable = current_executable()?;
    let agent_path = launch_agent_path()?;
    remove_agent_file(&agent_path)?;
    bootout_running_agent();
    status_at(&agent_path, &executable)
}

fn remove_agent_file(agent_path: &Path) -> AppResult<()> {
    match fs::remove_file(agent_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(AppError::io("remove launch agent", agent_path, source)),
    }
}

/// Best-effort `launchctl bootout` of the login-launched instance. A manual
/// `auto-reverse run` in a terminal is not affected - only the job launchd
/// itself manages under our label.
fn bootout_running_agent() {
    let uid = unsafe { getuid() };
    let target = format!("gui/{uid}/{LABEL}");
    match Command::new("launchctl")
        .args(["bootout", &target])
        .output()
    {
        Ok(output) if output.status.success() => {
            println!("stopped the login-launched agent instance");
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.contains("No such process") {
                eprintln!("note: launchctl bootout {target}: {}", stderr.trim());
            }
        }
        Err(error) => {
            eprintln!("note: could not run launchctl bootout: {error}");
        }
    }
}

fn launch_agent_path() -> AppResult<PathBuf> {
    if let Some(dir) = env::var_os("AUTO_REVERSE_LAUNCH_AGENT_DIR") {
        return Ok(PathBuf::from(dir).join(PLIST_FILE));
    }

    Ok(home_dir()?
        .join("Library")
        .join("LaunchAgents")
        .join(PLIST_FILE))
}

/// Where the `run` daemon's stdout/stderr are logged. Shared by the
/// LaunchAgent plist (`StandardOutPath`/`StandardErrorPath`) and by the
/// settings window's GUI-spawned daemon (`ui.rs`), so both routes into
/// `run` land in the same file.
pub fn log_path() -> AppResult<PathBuf> {
    if let Some(dir) = env::var_os("AUTO_REVERSE_LAUNCH_AGENT_DIR") {
        return Ok(PathBuf::from(dir).join(LOG_FILE));
    }

    Ok(home_dir()?.join("Library").join("Logs").join(LOG_FILE))
}

fn home_dir() -> AppResult<PathBuf> {
    let Some(home) = env::var_os("HOME") else {
        return Err(AppError::Platform(
            "HOME is not set, cannot locate ~/Library".to_string(),
        ));
    };
    Ok(PathBuf::from(home))
}

/// Extracts ProgramArguments[0] from a plist we wrote ourselves: the first
/// `<string>` right after the ProgramArguments key. A substring check over
/// the whole file was the previous approach and produced false positives
/// whenever the configured path merely contained the current one.
fn parse_program_path(contents: &str) -> Option<PathBuf> {
    let after_key = contents
        .split_once("<key>ProgramArguments</key>")
        .map(|(_, rest)| rest)?;
    let after_open = after_key.split_once("<string>").map(|(_, rest)| rest)?;
    let (raw, _) = after_open.split_once("</string>")?;
    Some(PathBuf::from(xml_unescape(raw)))
}

/// Compares canonicalized paths so symlinks and `../` spellings of the same
/// binary still match; falls back to literal comparison when either path no
/// longer exists.
fn same_file(configured: &Path, executable: &Path) -> bool {
    match (fs::canonicalize(configured), fs::canonicalize(executable)) {
        (Ok(a), Ok(b)) => a == b,
        _ => configured == executable,
    }
}

fn plist_document(executable: &Path, log_path: &Path) -> String {
    let executable = xml_escape(&executable.display().to_string());
    let log = xml_escape(&log_path.display().to_string());
    // KeepAlive/Crashed=true: restart after a crash (the tap process is
    // long-lived), but do NOT respawn-loop on clean exits - a plain
    // SuccessfulExit=false would hot-loop every ThrottleInterval on the
    // legitimate exit-1 path when permissions are missing after a rebuild.
    // StandardOutPath/StandardErrorPath: without them launchd points the
    // job's stdio at /dev/null and every login-time failure is invisible.
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{executable}</string>
    <string>run</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <dict>
    <key>Crashed</key>
    <true/>
  </dict>
  <key>StandardOutPath</key>
  <string>{log}</string>
  <key>StandardErrorPath</key>
  <string>{log}</string>
</dict>
</plist>
"#
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn xml_unescape(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_escape_handles_special_characters() {
        assert_eq!(
            xml_escape("/tmp/a&b/<c>\"d'e"),
            "/tmp/a&amp;b/&lt;c&gt;&quot;d&apos;e"
        );
    }

    #[test]
    fn xml_unescape_round_trips_escape() {
        let original = "/tmp/a&b/<c>\"d'e";
        assert_eq!(xml_unescape(&xml_escape(original)), original);
    }

    #[test]
    fn plist_contains_run_argument_escaped_executable_and_log_paths() {
        let plist = plist_document(
            Path::new("/Applications/A&B/auto-reverse"),
            Path::new("/Users/x/Library/Logs/auto-reverse.log"),
        );

        assert!(plist.contains("<string>/Applications/A&amp;B/auto-reverse</string>"));
        assert!(plist.contains("<string>run</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
        assert!(plist.contains("<key>Crashed</key>"));
        assert!(plist.contains("<key>StandardOutPath</key>"));
        assert!(plist.contains("<string>/Users/x/Library/Logs/auto-reverse.log</string>"));
    }

    #[test]
    fn parse_program_path_extracts_the_first_program_argument() {
        let plist = plist_document(
            Path::new("/opt/tools/a&b/auto-reverse"),
            Path::new("/tmp/log"),
        );

        assert_eq!(
            parse_program_path(&plist),
            Some(PathBuf::from("/opt/tools/a&b/auto-reverse"))
        );
    }

    #[test]
    fn parse_program_path_rejects_plists_without_program_arguments() {
        assert_eq!(parse_program_path("<plist><dict></dict></plist>"), None);
        // Binary plists never make it here (non-UTF8), but a rewritten XML
        // plist with a different structure must not match by accident.
        assert_eq!(
            parse_program_path("<key>Label</key><string>/some/path</string>"),
            None
        );
    }

    #[test]
    fn a_path_that_merely_contains_the_executable_does_not_match() {
        // Regression test for the substring false positive: the configured
        // path extends the current executable's path.
        let configured = PathBuf::from("/usr/local/bin/auto-reverse-nightly");
        let executable = PathBuf::from("/usr/local/bin/auto-reverse");
        assert!(!same_file(&configured, &executable));
    }

    #[test]
    fn enable_status_disable_round_trip_in_an_isolated_directory() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!("auto-reverse-startup-{nanos}"));
        let agent = dir.join(PLIST_FILE);
        let log = dir.join(LOG_FILE);
        let exe = PathBuf::from("/opt/fake/auto-reverse");

        let before = status_at(&agent, &exe).unwrap();
        assert!(!before.installed);

        let enabled = enable_at(&agent, &log, &exe).unwrap();
        assert!(enabled.installed);
        assert!(enabled.configured_for_current_exe);

        // A different executable must not claim the same agent.
        let other = status_at(&agent, Path::new("/opt/fake/other")).unwrap();
        assert!(other.installed);
        assert!(!other.configured_for_current_exe);

        remove_agent_file(&agent).unwrap();
        let after = status_at(&agent, &exe).unwrap();
        assert!(!after.installed);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unreadable_or_foreign_plist_reports_installed_but_unknown_target() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!("auto-reverse-binary-plist-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        let agent = dir.join(PLIST_FILE);
        // Simulate a binary plist: not valid UTF-8, launchd would accept it.
        fs::write(&agent, [0x62u8, 0x70, 0x6c, 0x69, 0x73, 0x74, 0xff, 0xfe]).unwrap();

        let status = status_at(&agent, Path::new("/opt/fake/auto-reverse")).unwrap();

        assert!(status.installed);
        assert!(!status.configured_for_current_exe);

        let _ = fs::remove_dir_all(dir);
    }
}
