//! Loading and saving settings.

use crate::config::Settings;
use crate::paths;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("could not read settings at {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("could not write settings to {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("settings file is not valid JSON: {0}")]
    Parse(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// Reads and writes the settings file.
#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: PathBuf,
}

impl Default for SettingsStore {
    fn default() -> Self {
        Self::new(paths::settings_file())
    }
}

impl SettingsStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Load settings, falling back to a starter configuration on first run.
    pub fn load(&self) -> Result<Settings> {
        match std::fs::read_to_string(&self.path) {
            Ok(contents) => Ok(serde_json::from_str(&contents)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(Settings::with_starter_profiles())
            }
            Err(source) => Err(StoreError::Read {
                path: self.path.display().to_string(),
                source,
            }),
        }
    }

    /// Save settings, writing to a temporary file first so an interrupted write
    /// cannot leave a truncated config behind.
    pub fn save(&self, settings: &Settings) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| StoreError::Write {
                path: parent.display().to_string(),
                source,
            })?;
        }

        let json = serde_json::to_string_pretty(settings)?;
        let temp = self.path.with_extension("json.tmp");

        std::fs::write(&temp, json).map_err(|source| StoreError::Write {
            path: temp.display().to_string(),
            source,
        })?;
        std::fs::rename(&temp, &self.path).map_err(|source| StoreError::Write {
            path: self.path.display().to_string(),
            source,
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ProjectBinding, Role};

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "heretic-store-{name}-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[test]
    fn a_missing_file_yields_starter_settings() {
        let store = SettingsStore::new(temp_path("missing"));
        let settings = store.load().unwrap();
        assert!(!settings.profiles.is_empty());
        assert!(settings
            .resolve_profile("anything", Role::Implementer)
            .is_some());
    }

    #[test]
    fn settings_survive_a_round_trip() {
        let path = temp_path("roundtrip");
        let store = SettingsStore::new(&path);

        let mut settings = Settings::with_starter_profiles();
        settings.flux.base_url = "http://localhost:4000".into();
        settings.flux.api_key = Some("flx_test".into());
        settings.upsert_binding(ProjectBinding::new("proj1", "/tmp/repo"));

        store.save(&settings).unwrap();
        let loaded = store.load().unwrap();

        assert_eq!(loaded.flux.base_url, "http://localhost:4000");
        assert_eq!(loaded.flux.api_key.as_deref(), Some("flx_test"));
        assert_eq!(loaded.bindings.len(), 1);
        assert_eq!(loaded.bindings[0].project_id, "proj1");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_binding_is_replaced_rather_than_duplicated() {
        let mut settings = Settings::with_starter_profiles();
        settings.upsert_binding(ProjectBinding::new("proj1", "/tmp/one"));
        settings.upsert_binding(ProjectBinding::new("proj1", "/tmp/two"));

        assert_eq!(settings.bindings.len(), 1);
        assert_eq!(settings.bindings[0].repo_path.to_str().unwrap(), "/tmp/two");
    }

    #[test]
    fn corrupt_settings_are_reported_not_silently_replaced() {
        let path = temp_path("corrupt");
        std::fs::write(&path, "{ not json").unwrap();
        let store = SettingsStore::new(&path);
        assert!(matches!(store.load(), Err(StoreError::Parse(_))));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn validation_catches_roles_pointing_at_missing_profiles() {
        let mut settings = Settings::with_starter_profiles();
        settings
            .roles
            .insert(Role::Reviewer, "does-not-exist".into());
        let problems = settings.validate();
        assert!(problems.iter().any(|p| p.contains("Reviewer")));
    }

    #[test]
    fn a_relative_repository_path_is_rejected() {
        let mut settings = Settings::with_starter_profiles();
        settings.upsert_binding(ProjectBinding::new("proj1", "relative/path"));
        assert!(settings.validate().iter().any(|p| p.contains("absolute")));
    }
}
