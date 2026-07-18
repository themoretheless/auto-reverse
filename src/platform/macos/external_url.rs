//! Narrow macOS adapter for opening trusted product URLs in the default browser.

use std::process::Command;

use crate::error::{AppError, AppResult};
use crate::update_policy::ReleaseChannel;

pub fn open_release_page(channel: ReleaseChannel) -> AppResult<()> {
    let status = Command::new("/usr/bin/open")
        .arg(channel.url())
        .status()
        .map_err(|error| {
            AppError::io(
                "open release page",
                format!("release URL ({})", channel.label()),
                error,
            )
        })?;
    if !status.success() {
        return Err(AppError::Platform(format!(
            "opening {} failed with {status}",
            channel.label()
        )));
    }
    Ok(())
}
