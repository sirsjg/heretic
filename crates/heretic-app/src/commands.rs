//! Tauri commands — the API the interface calls.
//!
//! Each one hands straight to `heretic_core::Service`, which is where the
//! behaviour lives; the remote server exposes the same calls over HTTP. The
//! only command that could not be moved is signing in, which needs a window.
//!
//! Every command returns `Result<_, String>` because Tauri surfaces the error
//! string directly to the caller, and the interface shows it to the user.

use crate::state::AppState;
use heretic_core::config::{ProjectBinding, Settings};
use heretic_core::detect::{self, HostProbe, ModelHost};
use heretic_core::model::{Project, SourceKind};
use heretic_core::orchestrator::RunRecord;
use heretic_core::service::{BoardView, ConnectionState, Environment};
use heretic_core::worktree::{Commit, FileChange};
use heretic_core::FluxClient;
use heretic_server::RemoteStatus;
use tauri::{Manager, State};

/// Label of the sign-in window, so a second attempt reuses it.
const SIGN_IN_WINDOW: &str = "flux-sign-in";

/// How long to wait for someone to finish signing in.
const SIGN_IN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

type Response<T> = Result<T, String>;

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Response<Settings> {
    Ok(state.service.settings().await)
}

#[tauri::command]
pub async fn save_settings(state: State<'_, AppState>, settings: Settings) -> Response<()> {
    state.service.save_settings(settings).await?;
    // Apply a change to remote access now rather than on the next check, so
    // the Settings screen sees the listener come up as the switch is flipped.
    state.remote.reconcile().await;
    Ok(())
}

/// Insert or update one project's binding, leaving the rest of the settings alone.
#[tauri::command]
pub async fn save_binding(state: State<'_, AppState>, binding: ProjectBinding) -> Response<()> {
    state.service.save_binding(binding).await
}

#[tauri::command]
pub async fn test_connection(state: State<'_, AppState>) -> Response<ConnectionState> {
    state.service.test_connection().await
}

#[tauri::command]
pub async fn test_linear_connection(state: State<'_, AppState>) -> Response<ConnectionState> {
    state.service.test_linear_connection().await
}

#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> Response<Vec<Project>> {
    state.service.list_projects().await
}

#[tauri::command]
pub async fn get_board(
    state: State<'_, AppState>,
    project_id: String,
    source: Option<SourceKind>,
) -> Response<BoardView> {
    state.service.board(&project_id, source).await
}

#[tauri::command]
pub async fn set_epic_auto(
    state: State<'_, AppState>,
    epic_id: String,
    auto: bool,
    source: Option<SourceKind>,
) -> Response<()> {
    state.service.set_epic_auto(&epic_id, auto, source).await
}

#[tauri::command]
pub async fn list_runs(state: State<'_, AppState>) -> Response<Vec<RunRecord>> {
    Ok(state.service.runs().await)
}

#[tauri::command]
pub async fn start_task(
    state: State<'_, AppState>,
    project_id: String,
    task_id: String,
) -> Response<String> {
    state.service.start_task(&project_id, &task_id).await
}

#[tauri::command]
pub async fn stop_run(state: State<'_, AppState>, run_id: String) -> Response<bool> {
    Ok(state.service.stop_run(&run_id).await)
}

#[tauri::command]
pub async fn answer_question(
    state: State<'_, AppState>,
    run_id: String,
    answer: String,
) -> Response<bool> {
    Ok(state.service.answer_question(&run_id, answer).await)
}

#[tauri::command]
pub async fn dismiss_run(state: State<'_, AppState>, run_id: String) -> Response<bool> {
    Ok(state.service.dismiss_run(&run_id).await)
}

#[tauri::command]
pub async fn integrate_run(state: State<'_, AppState>, run_id: String) -> Response<()> {
    state.service.integrate_run(&run_id).await
}

#[tauri::command]
pub async fn discard_run_work(state: State<'_, AppState>, run_id: String) -> Response<()> {
    state.service.discard_run_work(&run_id).await
}

#[tauri::command]
pub async fn run_changed_files(
    state: State<'_, AppState>,
    run_id: String,
) -> Response<Vec<FileChange>> {
    state.service.run_changed_files(&run_id).await
}

#[tauri::command]
pub async fn run_file_diff(
    state: State<'_, AppState>,
    run_id: String,
    path: String,
) -> Response<String> {
    state.service.run_file_diff(&run_id, &path).await
}

#[tauri::command]
pub async fn run_commits(state: State<'_, AppState>, run_id: String) -> Response<Vec<Commit>> {
    state.service.run_commits(&run_id).await
}

#[tauri::command]
pub async fn run_commit_diff(
    state: State<'_, AppState>,
    run_id: String,
    sha: String,
) -> Response<String> {
    state.service.run_commit_diff(&run_id, &sha).await
}

#[tauri::command]
pub async fn tick_auto(state: State<'_, AppState>) -> Response<Vec<String>> {
    Ok(state.service.tick_auto().await)
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
    let settings = state.service.settings().await;
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
                    state.service.keep_cookie(cookie).await?;
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
    state.service.sign_out().await
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

// --- Discovering what is available to run ------------------------------------

#[tauri::command]
pub async fn detect_environment(state: State<'_, AppState>) -> Response<Environment> {
    Ok(state.service.detect_environment().await)
}

#[tauri::command]
pub async fn probe_host(
    state: State<'_, AppState>,
    name: String,
    base_url: String,
) -> Response<HostProbe> {
    Ok(state.service.probe_host(name, base_url).await)
}

#[tauri::command]
pub async fn save_host(state: State<'_, AppState>, host: ModelHost) -> Response<()> {
    state.service.save_host(host).await
}

#[tauri::command]
pub async fn remove_host(state: State<'_, AppState>, host_id: String) -> Response<()> {
    state.service.remove_host(&host_id).await
}

/// Which platform this is. Called once at startup, so the interface can leave
/// room for the macOS window controls.
#[tauri::command]
pub fn platform() -> &'static str {
    std::env::consts::OS
}

/// The OpenAI-compatible base a runner should use for a host.
#[tauri::command]
pub fn openai_base(base_url: String) -> String {
    detect::openai_base(&base_url)
}

// --- Remote access -----------------------------------------------------------

/// Whether the listener is up, where, and the link a phone pairs with.
#[tauri::command]
pub async fn remote_status(state: State<'_, AppState>) -> Response<RemoteStatus> {
    Ok(state.remote.status().await)
}

/// Mint a new token, logging every paired device out.
#[tauri::command]
pub async fn rotate_remote_token(state: State<'_, AppState>) -> Response<String> {
    let token = state.service.rotate_remote_token().await?;
    state.remote.reconcile().await;
    Ok(token)
}

/// Post a test message to whatever notification services are configured.
#[tauri::command]
pub async fn test_notifications(state: State<'_, AppState>) -> Response<()> {
    let settings = state.service.settings().await;
    heretic_core::notify::deliver_test(&settings.notifications).await
}
