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
use tauri::State;

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
    let client = match client(&state).await {
        Ok(client) => client,
        Err(error) => {
            return Ok(ConnectionState {
                connected: false,
                error: Some(error),
            })
        }
    };

    // `/health` proves the server is up; listing projects proves our key works.
    match client.list_projects().await {
        Ok(_) => Ok(ConnectionState {
            connected: true,
            error: None,
        }),
        Err(error) => Ok(ConnectionState {
            connected: false,
            error: Some(if error.is_auth() {
                "Flux rejected the API key. Check it in Settings.".to_string()
            } else {
                error.to_string()
            }),
        }),
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
