//! The desktop shell.
//!
//! Deliberately thin: it owns the window, exposes the service as Tauri
//! commands, and forwards engine and Flux events to the interface. All
//! behaviour lives in `heretic-core`, which is why it can be tested without a
//! GUI — and why `heretic-server` can offer the same commands over HTTP.

mod commands;
mod state;

use state::AppState;
use std::sync::Arc;
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "heretic_app=info,heretic_core=info,heretic_server=info".into()
            }),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = AppState::load();
            let service = Arc::clone(&state.service);
            let remote = Arc::clone(&state.remote);
            app.manage(state);

            // Relay engine events to the interface.
            let handle = app.handle().clone();
            let mut events = service.subscribe();
            tauri::async_runtime::spawn(async move {
                loop {
                    match events.recv().await {
                        Ok(event) => {
                            let _ = handle.emit("engine://event", &event);
                        }
                        // Lagged just means the UI fell behind a burst of output;
                        // the next event still arrives, so keep going.
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });

            // And what Flux announces, so the board reflects changes made
            // elsewhere.
            let handle = app.handle().clone();
            let mut flux = service.subscribe_flux();
            tauri::async_runtime::spawn(async move {
                loop {
                    match flux.recv().await {
                        Ok(event) => {
                            let _ = handle.emit("flux://event", &event);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });

            // The Flux watcher, the auto loop and the notifier.
            tauri::async_runtime::spawn(Arc::clone(&service).run_background());
            // The remote listener, whenever the settings ask for one.
            tauri::async_runtime::spawn(remote.run());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::save_binding,
            commands::test_connection,
            commands::test_linear_connection,
            commands::list_projects,
            commands::get_board,
            commands::set_epic_auto,
            commands::list_runs,
            commands::start_task,
            commands::stop_run,
            commands::answer_question,
            commands::dismiss_run,
            commands::tick_auto,
            commands::integrate_run,
            commands::discard_run_work,
            commands::run_changed_files,
            commands::run_file_diff,
            commands::run_commits,
            commands::run_commit_diff,
            commands::flux_sign_in,
            commands::flux_sign_out,
            commands::detect_environment,
            commands::probe_host,
            commands::save_host,
            commands::remove_host,
            commands::openai_base,
            commands::platform,
            commands::remote_status,
            commands::rotate_remote_token,
            commands::test_notifications,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Heretic");
}
