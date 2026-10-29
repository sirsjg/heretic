//! Finding, downloading and installing new releases.
//!
//! Each GitHub release carries a signed `latest.json` describing its bundles.
//! The app reads it, and when there is something newer the interface offers to
//! install it. Installing replaces the running bundle in place and restarts.
//!
//! Not every install can do that. A `.deb` is owned by the package manager, so
//! unless the release carries a signed `.deb` the interface only announces the
//! version and links to the release page instead.

use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, UpdaterExt};

type Response<T> = Result<T, String>;

/// Where people can read about, and download, a release by hand.
const RELEASES: &str = "https://github.com/sirsjg/heretic/releases";

/// The update found by the last check, kept so installing it does not need a
/// second round trip — and cannot install something other than what was shown.
#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<Update>>);

/// A newer release, as the interface shows it.
#[derive(Serialize)]
pub struct UpdateInfo {
    version: String,
    current_version: String,
    /// Release notes, in Markdown.
    notes: Option<String>,
    /// Whether this install can replace itself. When false the interface links
    /// to `release_url` instead of offering to install.
    can_install: bool,
    release_url: String,
}

/// Progress of a download, in bytes. `total` is absent when the server does
/// not say how large the bundle is.
#[derive(Clone, Serialize)]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

/// Look for a newer release. `None` means this is the latest.
#[tauri::command]
pub async fn check_for_update(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Response<Option<UpdateInfo>> {
    let update = app
        .updater()
        .map_err(|error| format!("Could not set up the updater: {error}"))?
        .check()
        .await
        .map_err(|error| format!("Could not check for updates: {error}"))?;

    let info = update.as_ref().map(|update| UpdateInfo {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        notes: update.body.clone(),
        can_install: can_install(update),
        release_url: format!("{RELEASES}/tag/v{}", update.version),
    });

    *pending.0.lock().expect("pending update lock poisoned") = update;
    Ok(info)
}

/// Download and install the update found by the last check, then restart into
/// it. Progress is reported on `update://progress`.
#[tauri::command]
pub async fn install_update(app: AppHandle, pending: State<'_, PendingUpdate>) -> Response<()> {
    let update = pending
        .0
        .lock()
        .expect("pending update lock poisoned")
        .take()
        .ok_or("There is no update to install. Check for updates first.")?;

    if !can_install(&update) {
        return Err(format!(
            "This copy of Heretic is managed by your package manager. Download {} from {RELEASES} instead.",
            update.version
        ));
    }

    let mut downloaded = 0u64;
    let events = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ = events.emit("update://progress", Progress { downloaded, total });
            },
            || {},
        )
        .await
        .map_err(|error| format!("Could not install the update: {error}"))?;

    tracing::info!(version = %update.version, "update installed; restarting");
    app.restart();
}

/// Whether the updater can replace this install with `update`'s bundle.
///
/// The macOS `.app` and the AppImage replace themselves. A `.deb` can only be
/// upgraded with the package the release signed for it: without one, the
/// updater would fall back to the AppImage, which `dpkg` cannot install.
fn can_install(update: &Update) -> bool {
    if cfg!(target_os = "macos") {
        return true;
    }
    if !cfg!(target_os = "linux") {
        return false;
    }
    if std::env::var_os("APPIMAGE").is_some() {
        return true;
    }
    update
        .download_url
        .path()
        .to_ascii_lowercase()
        .ends_with(".deb")
}
