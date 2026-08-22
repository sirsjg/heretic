//! Shared application state.

use accel_core::orchestrator::Engine;
use accel_core::store::SettingsStore;
use accel_core::Settings;
use std::sync::Arc;

pub struct AppState {
    pub engine: Arc<Engine>,
    pub store: SettingsStore,
}

impl AppState {
    /// Load settings from disk, falling back to a starter configuration.
    ///
    /// A corrupt settings file must not stop the app from opening — the user
    /// needs the Settings screen to fix it.
    pub fn load() -> Self {
        let store = SettingsStore::default();
        let settings = match store.load() {
            Ok(settings) => settings,
            Err(error) => {
                tracing::error!(%error, "could not read settings; starting with defaults");
                Settings::with_starter_profiles()
            }
        };

        Self {
            engine: Arc::new(Engine::new(settings)),
            store,
        }
    }
}
