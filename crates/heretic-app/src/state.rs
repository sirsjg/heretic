//! Shared application state.

use heretic_core::Service;
use heretic_server::Supervisor;
use std::sync::Arc;

pub struct AppState {
    pub service: Arc<Service>,
    /// The remote listener, following the settings.
    pub remote: Arc<Supervisor>,
}

impl AppState {
    pub fn load() -> Self {
        let service = Arc::new(Service::load());
        let remote = Supervisor::new(Arc::clone(&service));
        Self { service, remote }
    }
}
