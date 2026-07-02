//! Start-at-login support for the current CLI binary.
//!
//! A future packaged `.app` can use `SMAppService`, but the project is
//! currently a CLI binary. A per-user LaunchAgent is the honest integration
//! that works now: it starts this exact executable with the `run` argument
//! on the next login.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use crate::error::{AppError, AppResult};

const LABEL: &str = "com.auto-reverse.agent";
const PLIST_FILE: &str = "com.auto-reverse.agent.plist";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupStatus {
    pub enabled: bool,
    pub agent_path: PathBuf,
    pub configured_for_current_exe: bool,
}

impl StartupStatus {
    pub fn summary(&self) -> String {
        match (self.enabled, self.configured_for_current_exe) {
            (true, true) => format!("enabled for this binary ({})", self.agent_path.display()),
            (true, false) => format!(
                "enabled, but points at a different binary ({})",
                self.agent_path.display()
            ),
            (false, _) => format!("disabled ({})", self.agent_path.display()),
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
    let agent_path = launch_agent_path()?;
    if !agent_path.exists() {
        return Ok(StartupStatus {
            enabled: false,
            agent_path,
            configured_for_current_exe: false,
        });
    }

    let contents = fs::read_to_string(&agent_path)
        .map_err(|source| AppError::io("read launch agent", &agent_path, source))?;
    let configured_for_current_exe =
        contents.contains(&xml_escape(&executable.display().to_string()));
    Ok(StartupStatus {
        enabled: true,
        agent_path,
        configured_for_current_exe,
    })
}

pub fn enable_for_current_executable() -> AppResult<StartupStatus> {
    enable_for_executable(&current_executable()?)
}

pub fn enable_for_executable(executable: &Path) -> AppResult<StartupStatus> {
    let agent_path = launch_agent_path()?;
    if let Some(parent) = agent_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|source| AppError::io("create LaunchAgents directory", parent, source))?;
    }

    let plist = plist_for_executable(executable);
    let tmp_path = agent_path.with_extension(format!("plist.{}.tmp", process::id()));
    fs::write(&tmp_path, plist).map_err(|source| {
        let _ = fs::remove_file(&tmp_path);
        AppError::io("write temporary launch agent", &tmp_path, source)
    })?;
    fs::rename(&tmp_path, &agent_path).map_err(|source| {
        let _ = fs::remove_file(&tmp_path);
        AppError::io("install launch agent", &agent_path, source)
    })?;

    status_for_executable(executable)
}

pub fn disable() -> AppResult<StartupStatus> {
    let executable = current_executable()?;
    let agent_path = launch_agent_path()?;
    match fs::remove_file(&agent_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => return Err(AppError::io("remove launch agent", &agent_path, source)),
    }

    status_for_executable(&executable)
}

fn launch_agent_path() -> AppResult<PathBuf> {
    let Some(home) = env::var_os("HOME") else {
        return Err(AppError::Platform(
            "HOME is not set, cannot locate ~/Library/LaunchAgents".to_string(),
        ));
    };

    Ok(PathBuf::from(home)
        .join("Library")
        .join("LaunchAgents")
        .join(PLIST_FILE))
}

fn plist_for_executable(executable: &Path) -> String {
    let executable = xml_escape(&executable.display().to_string());
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
  <false/>
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
    fn plist_contains_run_argument_and_escaped_executable() {
        let plist = plist_for_executable(Path::new("/Applications/A&B/auto-reverse"));

        assert!(plist.contains("<string>/Applications/A&amp;B/auto-reverse</string>"));
        assert!(plist.contains("<string>run</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
    }
}
