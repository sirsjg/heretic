//! Live board updates over Flux's Server-Sent Events stream.
//!
//! Flux publishes `/api/events` with three event names:
//! `connected`, `change` (one per API mutation) and `data-changed` (a generic
//! invalidation ping emitted when the CLI writes the data file directly).
//!
//! `EventSource` cannot set an `Authorization` header, so Flux also accepts the
//! API key as a `token` query parameter — that is what we use here.

use super::client::FluxConfig;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::broadcast;

/// A board mutation announced by Flux.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BoardChange {
    /// Webhook event type, e.g. `task.status_changed`.
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub project_name: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

/// What the watcher reports to the rest of the app.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FluxEvent {
    /// The stream opened. A full refresh is due, since changes may have been
    /// missed while disconnected.
    Connected,
    /// A specific mutation landed.
    Changed(BoardChange),
    /// Something changed but Flux could not say what — refresh everything.
    Invalidated,
    /// The stream dropped; a reconnect is scheduled in `retry_in`.
    Disconnected {
        error: String,
        #[serde(serialize_with = "crate::flux::events::serialize_secs")]
        retry_in: Duration,
    },
}

pub(crate) fn serialize_secs<S: serde::Serializer>(
    value: &Duration,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    serializer.serialize_u64(value.as_secs())
}

/// Subscribes to a Flux server and rebroadcasts its events, reconnecting with
/// capped exponential backoff for as long as the returned handle is alive.
pub struct FluxWatcher {
    tx: broadcast::Sender<FluxEvent>,
    shutdown: tokio_util_shim::Flag,
}

impl FluxWatcher {
    /// Start watching. The task stops when the returned `FluxWatcher` is dropped.
    ///
    /// Must be called from inside a Tokio runtime — it spawns the reader task.
    pub fn start(config: FluxConfig) -> Self {
        let (tx, _) = broadcast::channel(256);
        let shutdown = tokio_util_shim::Flag::new();
        let watcher = Self {
            tx: tx.clone(),
            shutdown: shutdown.clone(),
        };

        tokio::spawn(async move {
            let mut backoff = Duration::from_secs(1);
            let max_backoff = Duration::from_secs(30);

            while !shutdown.is_set() {
                match stream_once(&config, &tx, &shutdown).await {
                    Ok(()) => backoff = Duration::from_secs(1),
                    Err(error) => {
                        let _ = tx.send(FluxEvent::Disconnected {
                            error: error.to_string(),
                            retry_in: backoff,
                        });
                    }
                }
                if shutdown.is_set() {
                    break;
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(max_backoff);
            }
        });

        watcher
    }

    /// Receive events. Every subscriber sees every event from the moment it subscribes.
    pub fn subscribe(&self) -> broadcast::Receiver<FluxEvent> {
        self.tx.subscribe()
    }
}

impl Drop for FluxWatcher {
    fn drop(&mut self) {
        self.shutdown.set();
    }
}

async fn stream_once(
    config: &FluxConfig,
    tx: &broadcast::Sender<FluxEvent>,
    shutdown: &tokio_util_shim::Flag,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut url = format!("{}/api/events", config.normalised_base());
    if let Some(key) = config.api_key.as_deref().filter(|k| !k.is_empty()) {
        url.push_str(&format!("?token={}", urlencode(key)));
    }

    // No overall timeout: this request is meant to stay open indefinitely.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(0))
        .build()?;
    let response = client.get(&url).send().await?;
    if !response.status().is_success() {
        return Err(format!("event stream rejected with status {}", response.status()).into());
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        if shutdown.is_set() {
            return Ok(());
        }
        buffer.push_str(&String::from_utf8_lossy(&chunk?));

        // SSE frames are separated by a blank line.
        while let Some(index) = find_frame_end(&buffer) {
            let frame: String = buffer.drain(..index).collect();
            // Drop the separator itself.
            let separator_len = if buffer.starts_with("\r\n\r\n") { 4 } else { 2 };
            buffer.drain(..separator_len.min(buffer.len()));

            if let Some(event) = parse_frame(&frame) {
                let _ = tx.send(event);
            }
        }
    }

    Ok(())
}

fn find_frame_end(buffer: &str) -> Option<usize> {
    let lf = buffer.find("\n\n");
    let crlf = buffer.find("\r\n\r\n");
    match (lf, crlf) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

/// Turn one SSE frame into a [`FluxEvent`], or `None` for frames we ignore
/// (comments, keep-alives, unknown event names).
pub(crate) fn parse_frame(frame: &str) -> Option<FluxEvent> {
    let mut event_name = String::new();
    let mut data = String::new();

    for line in frame.lines() {
        let line = line.trim_end_matches('\r');
        if line.starts_with(':') {
            continue; // comment / keep-alive
        }
        if let Some(rest) = line.strip_prefix("event:") {
            event_name = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
        }
    }

    match event_name.as_str() {
        "connected" => Some(FluxEvent::Connected),
        "data-changed" => Some(FluxEvent::Invalidated),
        "change" => {
            let change = serde_json::from_str::<BoardChange>(&data).unwrap_or_default();
            Some(FluxEvent::Changed(change))
        }
        _ => None,
    }
}

fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// A tiny shared boolean flag, so the crate does not need `tokio-util` just for
/// cancellation.
pub(crate) mod tokio_util_shim {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    #[derive(Debug, Clone, Default)]
    pub struct Flag(Arc<AtomicBool>);

    impl Flag {
        pub fn new() -> Self {
            Self(Arc::new(AtomicBool::new(false)))
        }
        pub fn set(&self) {
            self.0.store(true, Ordering::SeqCst);
        }
        pub fn is_set(&self) -> bool {
            self.0.load(Ordering::SeqCst)
        }
    }
}
