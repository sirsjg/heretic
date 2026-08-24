//! Where Heretic keeps its files.
//!
//! Follows each platform's convention rather than scattering dotfiles: XDG on
//! Linux, Application Support on macOS.

use std::path::PathBuf;

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Directory holding `settings.json`.
pub fn config_dir() -> PathBuf {
    if let Some(override_dir) = std::env::var_os("HERETIC_CONFIG_DIR") {
        return PathBuf::from(override_dir);
    }

    if cfg!(target_os = "macos") {
        home().join("Library/Application Support/Heretic")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".config"))
            .join("heretic")
    }
}

/// Directory holding worktrees and run logs.
pub fn data_dir() -> PathBuf {
    if let Some(override_dir) = std::env::var_os("HERETIC_DATA_DIR") {
        return PathBuf::from(override_dir);
    }

    if cfg!(target_os = "macos") {
        home().join("Library/Application Support/Heretic")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/share"))
            .join("heretic")
    }
}

pub fn settings_file() -> PathBuf {
    config_dir().join("settings.json")
}

/// Worktrees live under the data directory, deliberately outside the user's
/// repository so agents never see each other's checkouts.
pub fn worktree_root(project_id: &str) -> PathBuf {
    data_dir().join("worktrees").join(project_id)
}

/// Run journals, one JSONL file per run.
pub fn runs_dir() -> PathBuf {
    data_dir().join("runs")
}
