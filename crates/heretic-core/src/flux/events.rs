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
    let key = config.api_key.as_deref().filter(|k| !k.is_empty());
    let proxy_owns_authorization = config.authorization_taken_by_proxy();

    // Prefer the Authorization header so the key stays out of URLs and proxy
    // access logs. Fall back to Flux's documented `?token=` only when the proxy
    // has claimed that header for its own credential.
    if proxy_owns_authorization {
        if let Some(key) = key {
            url.push_str(&format!("?token={}", urlencode(key)));
        }
    }

    // No overall timeout: this request is meant to stay open indefinitely, and
    // a zero Duration means "time out immediately", not "never". The connect
    // timeout still applies, so an unreachable server fails promptly.
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .build()?;

    let mut request = client.get(&url);
    if !proxy_owns_authorization {
        if let Some(key) = key {
            request = request.bearer_auth(key);
        }
    }
    for (name, value) in &config.headers {
        request = request.header(name, value);
    }
    if let Some(cookie) = config.cookie.as_deref().filter(|c| !c.is_empty()) {
        request = request.header(reqwest::header::COOKIE, cookie);
    }

    let response = request.send().await?;
    if !response.status().is_success() {
        return Err(format!("event stream rejected with status {}", response.status()).into());
    }

    // A proxy sign-in page returns 200 with HTML, which would otherwise sit here
    // looking like an event stream that never produces an event.
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if content_type.contains("text/html") {
        return Err(
            "the event stream returned a sign-in page — Heretic is not authenticated with the \
proxy in front of Flux"
                .into(),
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    /// A minimal server that speaks just enough HTTP to hand back an SSE stream,
    /// so the watcher can be exercised without a Flux server.
    ///
    /// Returns its base URL and the raw request line + headers it received.
    async fn sse_server(
        body: &'static str,
        content_type: &'static str,
    ) -> (String, tokio::sync::oneshot::Receiver<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = tokio::sync::oneshot::channel();

        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };

            // Read the request head so the test can assert on the headers sent.
            let mut request = Vec::new();
            let mut buffer = [0u8; 1024];
            loop {
                match tokio::io::AsyncReadExt::read(&mut socket, &mut buffer).await {
                    Ok(0) => break,
                    Ok(n) => {
                        request.extend_from_slice(&buffer[..n]);
                        if request.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = tx.send(String::from_utf8_lossy(&request).to_string());

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nConnection: keep-alive\r\n\r\n{body}"
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.flush().await;
            // Hold the connection open so the watcher does not immediately retry.
            tokio::time::sleep(Duration::from_secs(5)).await;
        });

        (format!("http://127.0.0.1:{port}"), rx)
    }

    async fn first_event(config: FluxConfig) -> FluxEvent {
        let watcher = FluxWatcher::start(config);
        let mut events = watcher.subscribe();
        tokio::time::timeout(Duration::from_secs(10), events.recv())
            .await
            .expect("the watcher produced no event")
            .expect("stream closed")
    }

    /// Regression guard: the stream must stay open. Setting reqwest's overall
    /// timeout to zero means "time out immediately", not "never", which silently
    /// disabled live updates.
    #[tokio::test]
    async fn a_long_lived_stream_is_not_cut_off_by_a_timeout() {
        let (url, _requests) =
            sse_server("event: connected\ndata: \"ok\"\n\n", "text/event-stream").await;

        let event = first_event(FluxConfig {
            base_url: url,
            ..FluxConfig::default()
        })
        .await;

        assert!(
            matches!(event, FluxEvent::Connected),
            "expected Connected, got {event:?}"
        );
    }

    #[tokio::test]
    async fn board_changes_are_parsed_off_the_wire() {
        let (url, _requests) = sse_server(
            "event: change\ndata: {\"event\":\"task.status_changed\",\"project_id\":\"p1\",\"status\":\"done\"}\n\n",
            "text/event-stream",
        )
        .await;

        let event = first_event(FluxConfig {
            base_url: url,
            ..FluxConfig::default()
        })
        .await;

        match event {
            FluxEvent::Changed(change) => {
                assert_eq!(change.event, "task.status_changed");
                assert_eq!(change.project_id.as_deref(), Some("p1"));
            }
            other => panic!("expected a change event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn proxy_credentials_are_sent_on_the_event_stream_too() {
        let (url, requests) =
            sse_server("event: connected\ndata: \"ok\"\n\n", "text/event-stream").await;

        let config = FluxConfig {
            base_url: url,
            api_key: Some("flx_key".into()),
            headers: std::collections::BTreeMap::from([(
                "CF-Access-Client-Id".to_string(),
                "abc.access".to_string(),
            )]),
            cookie: Some("session=abc".into()),
        };
        let _ = first_event(config).await;

        // Header names go out lower-cased, which is what HTTP/1.1 treats as
        // equivalent and what HTTP/2 requires.
        let request = requests.await.unwrap().to_ascii_lowercase();
        assert!(
            request.contains("cf-access-client-id: abc.access"),
            "{request}"
        );
        assert!(request.contains("cookie: session=abc"), "{request}");
        // The proxy left Authorization alone, so Flux's key rides on the header
        // rather than being exposed in the query string.
        assert!(
            request.contains("authorization: bearer flx_key"),
            "{request}"
        );
        assert!(
            !request.contains("token=flx_key"),
            "key should not be in the URL: {request}"
        );
    }

    #[tokio::test]
    async fn when_the_proxy_owns_authorization_the_flux_key_moves_to_the_query() {
        let (url, requests) =
            sse_server("event: connected\ndata: \"ok\"\n\n", "text/event-stream").await;

        let config = FluxConfig {
            base_url: url,
            api_key: Some("flx_key".into()),
            headers: std::collections::BTreeMap::from([(
                "Authorization".to_string(),
                "Bearer proxy-jwt".to_string(),
            )]),
            cookie: None,
        };
        let _ = first_event(config).await;

        let request = requests.await.unwrap().to_ascii_lowercase();
        assert!(request.contains("token=flx_key"), "{request}");
        assert!(request.contains("bearer proxy-jwt"), "{request}");
        // Flux's key must not also ride on the header the proxy has claimed.
        assert!(!request.contains("bearer flx_key"), "{request}");
    }

    #[tokio::test]
    async fn a_sign_in_page_on_the_event_stream_is_reported_not_awaited() {
        let (url, _requests) = sse_server("<!DOCTYPE html><html>login</html>", "text/html").await;

        let event = first_event(FluxConfig {
            base_url: url,
            ..FluxConfig::default()
        })
        .await;

        match event {
            FluxEvent::Disconnected { error, .. } => {
                assert!(error.contains("sign-in page"), "{error}");
            }
            other => panic!("expected a disconnect, got {other:?}"),
        }
    }

    #[test]
    fn frames_are_parsed_from_the_wire_format() {
        assert!(matches!(
            parse_frame("event: connected\ndata: \"ok\""),
            Some(FluxEvent::Connected)
        ));
        assert!(matches!(
            parse_frame("event: data-changed\ndata: {\"ts\":1}"),
            Some(FluxEvent::Invalidated)
        ));
        // Keep-alive comments and unknown events carry nothing.
        assert!(parse_frame(": keep-alive").is_none());
        assert!(parse_frame("event: something-else\ndata: {}").is_none());
    }
}
