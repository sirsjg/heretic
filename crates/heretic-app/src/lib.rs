//! The desktop shell.
//!
//! Deliberately thin: it owns the window, exposes the engine as Tauri commands,
//! and forwards engine and Flux events to the interface. All behaviour lives in
//! `heretic-core`, which is why it can be tested without a GUI.

mod commands;
mod state;

use state::AppState;
use std::sync::Arc;
use tauri::{Emitter, Manager};

/// How often the auto loop looks for newly-ready work.
///
/// Flux's event stream normally tells us sooner; this is the safety net for
/// changes made while disconnected, or by a CLI writing the data file directly.
const AUTO_POLL: std::time::Duration = std::time::Duration::from_secs(45);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "heretic_app=info,heretic_core=info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = AppState::load();
            let engine = Arc::clone(&state.engine);
            app.manage(state);

            // Relay engine events to the interface.
            let handle = app.handle().clone();
            let mut events = engine.subscribe();
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

            // Watch Flux so the board reflects changes made elsewhere, and so
            // work switched to Auto in the Flux UI is picked up promptly.
            let handle = app.handle().clone();
            let watch_engine = Arc::clone(&engine);
            tauri::async_runtime::spawn(async move {
                let settings = watch_engine.settings().await;
                let watcher = heretic_core::FluxWatcher::start(settings.flux.clone());
                let mut events = watcher.subscribe();

                loop {
                    match events.recv().await {
                        Ok(event) => {
                            let _ = handle.emit("flux://event", &event);
                            if matches!(
                                event,
                                heretic_core::FluxEvent::Changed(_)
                                    | heretic_core::FluxEvent::Invalidated
                            ) {
                                watch_engine.tick_auto().await;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });

            tauri::async_runtime::spawn(Arc::clone(&engine).auto_loop(AUTO_POLL));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::save_binding,
            commands::test_connection,
            commands::list_projects,
            commands::get_board,
            commands::set_epic_auto,
            commands::list_runs,
            commands::start_task,
            commands::stop_run,
            commands::dismiss_run,
            commands::tick_auto,
            commands::integrate_run,
            commands::discard_run_work,
            commands::flux_sign_in,
            commands::flux_sign_out,
            commands::detect_environment,
            commands::probe_host,
            commands::save_host,
            commands::remove_host,
            commands::openai_base,
            commands::platform,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Heretic");
}
