//! The HTTP surface: one call endpoint mirroring the desktop's commands, a
//! WebSocket carrying the engine's events, and two glance endpoints for
//! anything too small to run the interface.
//!
//! `POST /api/call/<command>` takes the same command names and arguments the
//! interface sends over Tauri's IPC, so the interface needs one transport
//! swapped rather than a second API. Arguments are accepted in either spelling
//! (`runId` as the interface sends, `run_id` as a shell script would type).

use crate::assets;
use crate::Context;
use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use heretic_core::config::{ProjectBinding, Settings};
use heretic_core::detect::ModelHost;
use heretic_core::model::SourceKind;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

pub(crate) fn router(context: Arc<Context>) -> Router {
    Router::new()
        .route("/api/call/{command}", post(call))
        .route("/api/status", get(status))
        .route("/api/ticker", get(ticker))
        .route("/api/events", get(events))
        .route_layer(middleware::from_fn_with_state(
            Arc::clone(&context),
            require_token,
        ))
        .fallback(assets::serve)
        .with_state(context)
}

// --- Authentication ----------------------------------------------------------

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

/// Every `/api` route needs the bearer token.
///
/// A browser cannot set a header on a WebSocket, so the events route also
/// takes the token as a query parameter — and only that route, because a
/// token in a URL ends up in logs.
async fn require_token(
    State(context): State<Arc<Context>>,
    Query(query): Query<TokenQuery>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let presented = bearer(request.headers()).or_else(|| {
        if request.uri().path() == "/api/events" {
            query.token.clone()
        } else {
            None
        }
    });

    match presented {
        Some(token) if constant_time_eq(token.as_bytes(), context.token.as_bytes()) => {
            next.run(request).await
        }
        _ => ApiError::unauthorised().into_response(),
    }
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let token = value.strip_prefix("Bearer ")?.trim();
    (!token.is_empty()).then(|| token.to_string())
}

/// Compare without leaking where the first difference is.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

// --- Errors -----------------------------------------------------------------

/// What a failed call says. The message is the same human-readable text the
/// desktop shows, so the interface treats both transports alike.
#[derive(Debug)]
pub(crate) struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn unauthorised() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: "This token is not accepted. Pair again from the desktop app.".into(),
        }
    }

    fn unknown(command: &str) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: format!("There is no command called {command}."),
        }
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }
}

impl From<String> for ApiError {
    fn from(message: String) -> Self {
        Self::bad_request(message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(serde_json::json!({ "error": self.message }));
        (self.status, body).into_response()
    }
}

// --- Calls ------------------------------------------------------------------

#[derive(Deserialize)]
struct RunArgs {
    #[serde(alias = "runId")]
    run_id: String,
}

#[derive(Deserialize)]
struct BoardArgs {
    #[serde(alias = "projectId")]
    project_id: String,
    #[serde(default)]
    source: Option<SourceKind>,
}

#[derive(Deserialize)]
struct EpicAutoArgs {
    #[serde(alias = "epicId")]
    epic_id: String,
    auto: bool,
    #[serde(default)]
    source: Option<SourceKind>,
}

#[derive(Deserialize)]
struct StartArgs {
    #[serde(alias = "projectId")]
    project_id: String,
    #[serde(alias = "taskId")]
    task_id: String,
}

#[derive(Deserialize)]
struct AnswerArgs {
    #[serde(alias = "runId")]
    run_id: String,
    answer: String,
}

#[derive(Deserialize)]
struct FileDiffArgs {
    #[serde(alias = "runId")]
    run_id: String,
    path: String,
}

#[derive(Deserialize)]
struct CommitDiffArgs {
    #[serde(alias = "runId")]
    run_id: String,
    sha: String,
}

#[derive(Deserialize)]
struct SettingsArgs {
    settings: Settings,
}

#[derive(Deserialize)]
struct BindingArgs {
    binding: ProjectBinding,
}

#[derive(Deserialize)]
struct ProbeArgs {
    name: String,
    #[serde(alias = "baseUrl")]
    base_url: String,
}

#[derive(Deserialize)]
struct HostArgs {
    host: ModelHost,
}

#[derive(Deserialize)]
struct HostIdArgs {
    #[serde(alias = "hostId")]
    host_id: String,
}

#[derive(Deserialize)]
struct BaseUrlArgs {
    #[serde(alias = "baseUrl")]
    base_url: String,
}

fn args<T: DeserializeOwned>(value: &Value) -> Result<T, ApiError> {
    serde_json::from_value(value.clone())
        .map_err(|error| ApiError::bad_request(format!("The arguments are not right: {error}")))
}

fn ok<T: Serialize>(value: T) -> Result<Json<Value>, ApiError> {
    serde_json::to_value(value)
        .map(Json)
        .map_err(|error| ApiError::bad_request(error.to_string()))
}

/// Run one of the interface's commands by name.
async fn call(
    State(context): State<Arc<Context>>,
    Path(command): Path<String>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let value: Value = if body.is_empty() {
        Value::Object(Default::default())
    } else {
        serde_json::from_slice(&body)
            .map_err(|error| ApiError::bad_request(format!("The body is not JSON: {error}")))?
    };
    dispatch(&context.service, &command, &value).await
}

pub(crate) async fn dispatch(
    service: &heretic_core::Service,
    command: &str,
    value: &Value,
) -> Result<Json<Value>, ApiError> {
    match command {
        "get_settings" => ok(service.settings().await),
        "save_settings" => {
            let SettingsArgs { settings } = args(value)?;
            ok(service.save_settings(settings).await?)
        }
        "save_binding" => {
            let BindingArgs { binding } = args(value)?;
            ok(service.save_binding(binding).await?)
        }
        "test_connection" => ok(service.test_connection().await?),
        "test_linear_connection" => ok(service.test_linear_connection().await?),
        "list_projects" => ok(service.list_projects().await?),
        "get_board" => {
            let BoardArgs { project_id, source } = args(value)?;
            ok(service.board(&project_id, source).await?)
        }
        "set_epic_auto" => {
            let EpicAutoArgs {
                epic_id,
                auto,
                source,
            } = args(value)?;
            ok(service.set_epic_auto(&epic_id, auto, source).await?)
        }
        "list_runs" => ok(service.runs().await),
        "start_task" => {
            let StartArgs {
                project_id,
                task_id,
            } = args(value)?;
            ok(service.start_task(&project_id, &task_id).await?)
        }
        "stop_run" => {
            let RunArgs { run_id } = args(value)?;
            ok(service.stop_run(&run_id).await)
        }
        "answer_question" => {
            let AnswerArgs { run_id, answer } = args(value)?;
            ok(service.answer_question(&run_id, answer).await)
        }
        "dismiss_run" => {
            let RunArgs { run_id } = args(value)?;
            ok(service.dismiss_run(&run_id).await)
        }
        "integrate_run" => {
            let RunArgs { run_id } = args(value)?;
            ok(service.integrate_run(&run_id).await?)
        }
        "discard_run_work" => {
            let RunArgs { run_id } = args(value)?;
            ok(service.discard_run_work(&run_id).await?)
        }
        "run_changed_files" => {
            let RunArgs { run_id } = args(value)?;
            ok(service.run_changed_files(&run_id).await?)
        }
        "run_file_diff" => {
            let FileDiffArgs { run_id, path } = args(value)?;
            ok(service.run_file_diff(&run_id, &path).await?)
        }
        "run_commits" => {
            let RunArgs { run_id } = args(value)?;
            ok(service.run_commits(&run_id).await?)
        }
        "run_commit_diff" => {
            let CommitDiffArgs { run_id, sha } = args(value)?;
            ok(service.run_commit_diff(&run_id, &sha).await?)
        }
        "tick_auto" => ok(service.tick_auto().await),
        "detect_environment" => ok(service.detect_environment().await),
        "probe_host" => {
            let ProbeArgs { name, base_url } = args(value)?;
            ok(service.probe_host(name, base_url).await)
        }
        "save_host" => {
            let HostArgs { host } = args(value)?;
            ok(service.save_host(host).await?)
        }
        "remove_host" => {
            let HostIdArgs { host_id } = args(value)?;
            ok(service.remove_host(&host_id).await?)
        }
        "openai_base" => {
            let BaseUrlArgs { base_url } = args(value)?;
            ok(heretic_core::detect::openai_base(&base_url))
        }
        "flux_sign_out" => ok(service.sign_out().await?),
        "rotate_remote_token" => ok(service.rotate_remote_token().await?),
        "test_notifications" => {
            let settings = service.settings().await;
            ok(heretic_core::notify::deliver_test(&settings.notifications).await?)
        }
        "platform" => ok("remote"),
        // Signing in opens a browser window on the desktop and lifts the
        // cookie out of it; there is no window here to open.
        "flux_sign_in" => Err(ApiError::bad_request(
            "Signing in through a browser needs the desktop app. Use an API key here instead.",
        )),
        // The listener's own status is the desktop's to report; from here the
        // answer is simply that it is reachable.
        "remote_status" => ok(serde_json::json!({ "reachable": true })),
        _ => Err(ApiError::unknown(command)),
    }
}

// --- Glances ----------------------------------------------------------------

/// The board in a glance, as JSON.
async fn status(State(context): State<Arc<Context>>) -> Json<heretic_core::service::Summary> {
    Json(context.service.summary().await)
}

/// The board in one line of plain text, for a status bar, a widget, or a pair
/// of glasses.
async fn ticker(State(context): State<Arc<Context>>) -> Response {
    let summary = context.service.summary().await;
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        summary.line,
    )
        .into_response()
}

// --- Events -----------------------------------------------------------------

/// One message on the socket: which stream it came from, and the event.
#[derive(Serialize)]
struct Envelope<'a, T: Serialize> {
    channel: &'a str,
    event: T,
}

/// How often to ping, so a proxy between here and the phone does not decide
/// an idle socket is dead.
const PING_EVERY: Duration = Duration::from_secs(25);

async fn events(State(context): State<Arc<Context>>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| stream_events(socket, context))
}

async fn stream_events(socket: WebSocket, context: Arc<Context>) {
    let (mut sink, mut stream) = socket.split();
    let mut engine = context.service.subscribe();
    let mut flux = context.service.subscribe_flux();
    let mut ping = tokio::time::interval(PING_EVERY);
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        let message = tokio::select! {
            event = engine.recv() => match event {
                Ok(event) => Some(envelope("engine", &event)),
                // Lagged just means the phone fell behind a burst of output;
                // the next event still arrives, and the interface re-reads
                // the run list when it notices a gap.
                Err(broadcast::error::RecvError::Lagged(_)) => Some(envelope("engine", &serde_json::json!({ "kind": "lagged" }))),
                Err(broadcast::error::RecvError::Closed) => break,
            },
            event = flux.recv() => match event {
                Ok(event) => Some(envelope("flux", &event)),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            },
            _ = ping.tick() => {
                if sink.send(Message::Ping(Bytes::new())).await.is_err() {
                    break;
                }
                None
            }
            incoming = stream.next() => match incoming {
                // Clients only listen; anything they say is a keepalive.
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => None,
            },
        };

        if let Some(text) = message {
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    }
}

fn envelope<T: Serialize>(channel: &str, event: &T) -> String {
    serde_json::to_string(&Envelope { channel, event }).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_compare_in_full() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(!constant_time_eq(b"", b"a"));
    }

    #[test]
    fn bearer_is_read_from_the_header() {
        let mut headers = HeaderMap::new();
        assert!(bearer(&headers).is_none());
        headers.insert(header::AUTHORIZATION, "Bearer  abc ".parse().unwrap());
        assert_eq!(bearer(&headers).as_deref(), Some("abc"));
        headers.insert(header::AUTHORIZATION, "Basic abc".parse().unwrap());
        assert!(bearer(&headers).is_none());
        headers.insert(header::AUTHORIZATION, "Bearer ".parse().unwrap());
        assert!(bearer(&headers).is_none());
    }

    #[test]
    fn arguments_take_either_spelling() {
        let camel: RunArgs = args(&serde_json::json!({ "runId": "r1" })).unwrap();
        let snake: RunArgs = args(&serde_json::json!({ "run_id": "r1" })).unwrap();
        assert_eq!(camel.run_id, snake.run_id);

        let missing: Result<RunArgs, _> = args(&serde_json::json!({}));
        let error = missing.err().expect("a missing id should be refused");
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
    }
}
