//! Tauri commands — the API the interface calls.
//!
//! Every command returns `Result<_, String>` because Tauri surfaces the error
//! string directly to the caller, and the interface shows it to the user. The
//! messages are therefore written to be read by a person, not a developer.

use crate::state::AppState;
use accel_core::config::{ProjectBinding, Settings};
use accel_core::model::{Epic, Project, Task};
use accel_core::orchestrator::RunRecord;
use accel_core::selection::BoardSnapshot;
use accel_core::FluxClient;
use serde::Serialize;
use std::collections::HashSet;
use tauri::{Manager, State};

/// Label of the sign-in window, so a second attempt reuses it.
const SIGN_IN_WINDOW: &str = "flux-sign-in";

/// How long to wait for someone to finish signing in.
const SIGN_IN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

type Response<T> = Result<T, String>;

/// A task plus whether Accelerate may start it, and why not.
#[derive(Serialize)]
pub struct TaskView {
    task: Task,
    ineligible: Option<&'static str>,
}

/// Everything one board screen needs, in a single round trip.
#[derive(Serialize)]
pub struct BoardView {
    project: Project,
    epics: Vec<Epic>,
    tasks: Vec<TaskView>,
    /// Task ids that could be started now, most important first.
    ready: Vec<String>,
}

#[derive(Serialize)]
pub struct ConnectionState {
    connected: bool,
    error: Option<String>,
    /// What kind of problem it is, so the interface can point at the fix rather
    /// than showing one undifferentiated red dot.
    kind: &'static str,
    /// Configuration that will bite later, even when the connection works.
    warnings: Vec<String>,
}

async fn client(state: &State<'_, AppState>) -> Response<FluxClient> {
    let settings = state.engine.settings().await;
    FluxClient::new(settings.flux).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Response<Settings> {
    Ok(state.engine.settings().await)
}

#[tauri::command]
pub async fn save_settings(state: State<'_, AppState>, settings: Settings) -> Response<()> {
    state
        .store
        .save(&settings)
        .map_err(|error| error.to_string())?;
    state.engine.set_settings(settings).await;
    Ok(())
}

/// Insert or update one project's binding, leaving the rest of the settings alone.
#[tauri::command]
pub async fn save_binding(state: State<'_, AppState>, binding: ProjectBinding) -> Response<()> {
    let mut settings = state.engine.settings().await;
    settings.upsert_binding(binding);
    state
        .store
        .save(&settings)
        .map_err(|error| error.to_string())?;
    state.engine.set_settings(settings).await;
    Ok(())
}

#[tauri::command]
pub async fn test_connection(state: State<'_, AppState>) -> Response<ConnectionState> {
    let settings = state.engine.settings().await;
    let warnings = settings.flux.access_warnings();

    let client = match client(&state).await {
        Ok(client) => client,
        Err(error) => {
            return Ok(ConnectionState {
                connected: false,
                error: Some(error),
                kind: "unreachable",
                warnings,
            })
        }
    };

    // Listing projects exercises the whole path: the proxy, then Flux's own key.
    match client.list_projects().await {
        Ok(_) => {
            // Success is not proof of authentication. A server that requires a
            // key still answers a keyless GET with the public projects — an
            // empty list for a private board, which looks exactly like a
            // connected server with nothing on it.
            if let Ok(status) = client.auth_status().await {
                if status.needs_key() {
                    let base = settings.flux.normalised_base();
                    return Ok(ConnectionState {
                        connected: false,
                        error: Some(format!(
                            "This Flux server requires an API key, and none was accepted — so only \
public projects are visible. Create a key at {base}/auth and paste it above."
                        )),
                        kind: "flux_auth",
                        warnings,
                    });
                }
            }

            Ok(ConnectionState {
                connected: true,
                error: None,
                kind: "ok",
                warnings,
            })
        }
        Err(error) => {
            let (kind, message) = if error.is_proxy_challenge() {
                ("proxy_challenge", error.to_string())
            } else if error.is_auth() {
                (
                    "flux_auth",
                    "Flux rejected the API key. Check it in Settings.".to_string(),
                )
            } else {
                ("unreachable", error.to_string())
            };

            Ok(ConnectionState {
                connected: false,
                error: Some(message),
                kind,
                warnings,
            })
        }
    }
}

#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> Response<Vec<Project>> {
    client(&state)
        .await?
        .list_projects()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_board(state: State<'_, AppState>, project_id: String) -> Response<BoardView> {
    let client = client(&state).await?;

    let (project, epics, tasks) = tokio::try_join!(
        client.get_project(&project_id),
        client.list_epics(&project_id),
        client.list_tasks(&project_id),
    )
    .map_err(|error| error.to_string())?;

    let running: HashSet<String> = state.engine.running_task_ids().await;
    let board = BoardSnapshot {
        epics: &epics,
        tasks: &tasks,
    };

    let ready: Vec<String> = board
        .candidates(&running)
        .into_iter()
        .map(|candidate| candidate.task.id)
        .collect();

    let views = tasks
        .iter()
        .map(|task| TaskView {
            task: task.clone(),
            ineligible: board.eligibility(task, &running).map(|why| why.describe()),
        })
        .collect();

    Ok(BoardView {
        project,
        epics,
        tasks: views,
        ready,
    })
}

/// Flip an epic's Auto switch. This writes to Flux, so the change is visible on
/// the board and to anything else watching it.
#[tauri::command]
pub async fn set_epic_auto(
    state: State<'_, AppState>,
    epic_id: String,
    auto: bool,
) -> Response<()> {
    client(&state)
        .await?
        .set_epic_auto(&epic_id, auto)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_runs(state: State<'_, AppState>) -> Response<Vec<RunRecord>> {
    Ok(state.engine.runs().await)
}

#[tauri::command]
pub async fn start_task(
    state: State<'_, AppState>,
    project_id: String,
    task_id: String,
) -> Response<String> {
    state
        .engine
        .start_task(&project_id, &task_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn stop_run(state: State<'_, AppState>, run_id: String) -> Response<bool> {
    Ok(state.engine.stop_run(&run_id).await)
}

#[tauri::command]
pub async fn dismiss_run(state: State<'_, AppState>, run_id: String) -> Response<bool> {
    Ok(state.engine.dismiss_run(&run_id).await)
}

/// Start whatever auto-enabled work is ready right now.
#[tauri::command]
pub async fn tick_auto(state: State<'_, AppState>) -> Response<Vec<String>> {
    Ok(state.engine.tick_auto().await)
}

// --- Signing in through an identity proxy ------------------------------------

/// Open a real browser window at the Flux server so the user can complete their
/// provider's OAuth flow, then lift the resulting session cookie out of the
/// webview and keep it.
///
/// This is the convenience path. A proxy session expires, which is fine while
/// someone is watching but not for unattended runs — those want a service token
/// under Settings → Access instead.
#[tauri::command]
pub async fn flux_sign_in(app: tauri::AppHandle, state: State<'_, AppState>) -> Response<String> {
    let settings = state.engine.settings().await;
    let base = settings.flux.normalised_base();

    let url = tauri::Url::parse(&base).map_err(|_| format!("{base} is not a valid URL."))?;

    // Reuse the window if a previous attempt left one open.
    if let Some(existing) = app.get_webview_window(SIGN_IN_WINDOW) {
        let _ = existing.close();
    }

    let window = tauri::WebviewWindowBuilder::new(
        &app,
        SIGN_IN_WINDOW,
        tauri::WebviewUrl::External(url.clone()),
    )
    .title("Sign in to Flux")
    .inner_size(520.0, 720.0)
    .build()
    .map_err(|error| format!("Could not open a sign-in window: {error}"))?;

    // Poll until the cookies we hold are good enough to reach the Flux API.
    let deadline = std::time::Instant::now() + SIGN_IN_TIMEOUT;
    loop {
        if std::time::Instant::now() > deadline {
            let _ = window.close();
            return Err("Sign-in timed out.".into());
        }

        // The user closing the window is a cancellation, not a failure to report.
        if app.get_webview_window(SIGN_IN_WINDOW).is_none() {
            return Err("Sign-in was cancelled.".into());
        }

        if let Some(cookie) = collect_cookies(&window, &url) {
            let mut candidate = settings.flux.clone();
            candidate.cookie = Some(cookie.clone());

            if let Ok(client) = FluxClient::new(candidate.clone()) {
                if client.list_projects().await.is_ok() {
                    let mut saved = state.engine.settings().await;
                    saved.flux.cookie = Some(cookie);
                    state.store.save(&saved).map_err(|e| e.to_string())?;
                    state.engine.set_settings(saved).await;

                    let _ = window.close();
                    return Ok("Signed in.".to_string());
                }
            }
        }

        tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    }
}

/// Forget the stored session cookie.
#[tauri::command]
pub async fn flux_sign_out(state: State<'_, AppState>) -> Response<()> {
    let mut settings = state.engine.settings().await;
    settings.flux.cookie = None;
    state.store.save(&settings).map_err(|e| e.to_string())?;
    state.engine.set_settings(settings).await;
    Ok(())
}

/// Every cookie the webview holds for the Flux origin, as a `Cookie` header value.
///
/// Proxy session cookies have no standard name, so we take them all rather than
/// guessing at `CF_Authorization`, `_oauth2_proxy`, `authelia_session` and the rest.
fn collect_cookies(window: &tauri::WebviewWindow, url: &tauri::Url) -> Option<String> {
    let cookies = window.cookies_for_url(url.clone()).ok()?;
    let pairs: Vec<String> = cookies
        .iter()
        .map(|cookie| format!("{}={}", cookie.name(), cookie.value()))
        .collect();

    (!pairs.is_empty()).then(|| pairs.join("; "))
}
