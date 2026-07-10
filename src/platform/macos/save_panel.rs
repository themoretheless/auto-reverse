//! Native save and Finder-reveal adapter for user-selected local exports.
//!
//! The Debug Console owns CSV generation and file writing. This module owns
//! only AppKit: presenting `NSSavePanel`, converting its file URL to a Rust
//! path, and asking Finder to select a completed export.

use std::path::{Path, PathBuf};

use objc2::MainThreadMarker;
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSModalResponseCancel, NSModalResponseOK, NSSavePanel, NSWorkspace};
use objc2_foundation::{NSArray, NSString, NSURL};

pub fn choose_csv_path(
    default_filename: &str,
    initial_directory: Option<&Path>,
) -> Result<Option<PathBuf>, String> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "the save panel must be opened on the main thread".to_string())?;
    let panel = NSSavePanel::savePanel(mtm);
    panel.setCanCreateDirectories(true);
    panel.setShowsTagField(false);
    panel.setCanSelectHiddenExtension(false);
    panel.setExtensionHidden(false);
    panel.setAllowsOtherFileTypes(false);
    panel.setNameFieldStringValue(&NSString::from_str(default_filename));
    panel.setPrompt(Some(&NSString::from_str("Export")));

    // allowedContentTypes would require another framework dependency just for
    // one CSV extension. This compatibility API is still supported by AppKit
    // and gives the same extension/validation behavior here.
    let file_types = NSArray::from_retained_slice(&[NSString::from_str("csv")]);
    #[allow(deprecated)]
    panel.setAllowedFileTypes(Some(&file_types));

    if let Some(directory) = initial_directory.filter(|path| path.is_dir()) {
        let directory = NSString::from_str(&directory.to_string_lossy());
        let url = NSURL::fileURLWithPath_isDirectory(&directory, true);
        panel.setDirectoryURL(Some(&url));
    }

    let response = panel.runModal();
    if response == NSModalResponseCancel {
        return Ok(None);
    }
    if response != NSModalResponseOK {
        return Err("the save panel could not complete".to_string());
    }

    let url = panel
        .URL()
        .ok_or_else(|| "the save panel returned no file URL".to_string())?;
    Ok(Some(file_url_path(&url)?))
}

pub fn reveal_in_finder(path: &Path) -> Result<(), String> {
    MainThreadMarker::new()
        .ok_or_else(|| "Finder reveal must run on the main thread".to_string())?;
    if !path.is_file() {
        return Err(format!("export no longer exists at `{}`", path.display()));
    }

    let path = NSString::from_str(&path.to_string_lossy());
    let url = NSURL::fileURLWithPath(&path);
    let urls = NSArray::from_retained_slice(&[url]);
    NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&urls);
    Ok(())
}

fn file_url_path(url: &NSURL) -> Result<PathBuf, String> {
    let path = url
        .path()
        .ok_or_else(|| "the save panel returned a non-file URL".to_string())?;
    Ok(autoreleasepool(|pool| {
        // SAFETY: the borrowed UTF-8 slice is copied into an owned PathBuf
        // before this autorelease pool ends.
        PathBuf::from(unsafe { path.to_str(pool) })
    }))
}
