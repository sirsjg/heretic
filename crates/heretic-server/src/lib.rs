//! Heretic's remote access.
//!
//! The desktop app runs the agents; this serves the same interface it shows in
//! its own window over HTTP, so a phone on the same tailnet or LAN can watch
//! the runs, answer an agent's question, and merge or discard the work.
//!
//! Everything a client can do goes through one bearer token. The interface
//! bundle itself is served without one — it is not a secret — but nothing
//! under `/api` answers without it, and the token is compared in constant
//! time. There is deliberately no user model, no session and no cookie: a
//! phone is paired by scanning a QR code that carries the token, and the
//! desktop can rotate it to log everything out.

mod api;
mod assets;
pub mod net;
mod supervisor;

pub use supervisor::{Address, RemoteStatus, Supervisor};

use heretic_core::config::RemoteConfig;
use heretic_core::Service;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::oneshot;

/// What every handler can reach.
pub(crate) struct Context {
    pub service: Arc<Service>,
    pub token: String,
}

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("remote access has no token; switch it off and on again to mint one")]
    NoToken,

    #[error("{0} is not an address this machine can listen on")]
    BadAddress(String),

    #[error("could not listen on {addr}: {source}")]
    Bind {
        addr: SocketAddr,
        #[source]
        source: std::io::Error,
    },
}

/// A server that is listening. Dropping it does not stop it; call [`stop`].
///
/// [`stop`]: Running::stop
pub struct Running {
    pub addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl Running {
    /// Stop accepting connections and wait for the ones in flight to end.
    pub async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = self.task.await;
    }
}

/// Bind and start serving. Must be called from inside a Tokio runtime.
pub async fn start(service: Arc<Service>, config: &RemoteConfig) -> Result<Running, StartError> {
    let token = config
        .token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or(StartError::NoToken)?
        .to_string();

    let ip: std::net::IpAddr = config
        .bind
        .trim()
        .parse()
        .map_err(|_| StartError::BadAddress(config.bind.clone()))?;
    let addr = SocketAddr::new(ip, config.port);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|source| StartError::Bind { addr, source })?;
    let addr = listener.local_addr().unwrap_or(addr);

    let context = Arc::new(Context { service, token });
    let app = api::router(context);

    let (shutdown, wait) = oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        let serving = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = wait.await;
        });
        if let Err(error) = serving.await {
            tracing::error!(%error, "remote server stopped");
        }
    });

    tracing::info!(%addr, "remote access listening");
    Ok(Running {
        addr,
        shutdown: Some(shutdown),
        task,
    })
}
