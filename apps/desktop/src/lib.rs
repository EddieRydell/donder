#![cfg_attr(not(windows), deny(unsafe_code))]
#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unwrap_used
    )
)]

use tauri::{Emitter, Manager};

mod audio;
pub mod bindings;
mod commands;
mod desktop_foundation_tests;
mod desktop_state;
mod device;
mod dto;
mod gui;
mod language_server;
mod output;
mod persistence;
mod preview;
mod project;
mod rendering;
mod sequence_clip_raster;
mod source_documents;
mod state_tasks;

pub fn run() -> Result<(), String> {
    if let Some(startup) = preview::startup_from_arguments()? {
        return preview::run_host(startup);
    }
    run_desktop().map_err(|error| error.to_string())
}

fn run_desktop() -> Result<(), tauri::Error> {
    let bindings = bindings::builder();

    tauri::Builder::default()
        .register_uri_scheme_protocol("donder-raster", |context, request| {
            raster_protocol_response(
                context.app_handle().state::<desktop_state::DesktopState>().inner(),
                request,
            )
        })
        .setup(|app| {
            let handle = app.handle().clone();
            let analysis_app = handle.clone();
            let state = desktop_state::DesktopState::new(move |snapshot| {
                let _ = analysis_app.emit("app_snapshot_changed", snapshot);
            });
            let preview = preview::PreviewWindowService::new(state.preview_wake());
            app.manage(state);
            app.manage(preview);
            let working = app.state::<desktop_state::DesktopState>().inner().clone();
            app.manage(language_server::LanguageServerHost::start(
                handle.clone(),
                working,
            )?);
            let state = app.state::<desktop_state::DesktopState>();
            if let Some(window) = app.get_window("main") {
                let close_app = handle.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = close_app.emit("close_requested", ());
                    }
                });
            }
            match state.persistence().load(&handle) {
                Ok(last_project) => {
                    let settings_snapshot = state.apply_persisted_settings();
                    if let Some(window_state) = state.persistence().main_window()
                        && let Some(window) = app.get_window("main")
                    {
                        persistence::apply_window_state(&window, &window_state);
                    }
                    if settings_snapshot.settings.reopen_last_project
                        && let Some(project) = last_project
                    {
                        state.open_project_path(&project);
                    }
                }
                Err(error) => {
                    state.set_persistence_error(format!(
                        "Desktop state could not be initialized: {error}. Saving will be retried on changes."
                    ));
                }
            }
            Ok(())
        })
        .invoke_handler(bindings.invoke_handler())
        .build(tauri::generate_context!())?
        .run(|app, event| {
            // Quitting from the macOS app menu or Dock requests an exit without a code; route it
            // through the same unsaved-changes flow as closing the window. `complete_close`
            // exits with an explicit code once that flow finishes.
            if let tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                api.prevent_exit();
                let _ = app.emit("close_requested", ());
            }
        });
    Ok(())
}

fn raster_protocol_response(
    state: &desktop_state::DesktopState,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let token = request.uri().path().trim_start_matches('/');
    if token.is_empty() {
        return response_with_status(tauri::http::StatusCode::NOT_FOUND, Vec::new());
    }
    match state.sequence_clip_raster_pixels(token) {
        Some(bytes) => response_with_status(tauri::http::StatusCode::OK, bytes),
        None => response_with_status(tauri::http::StatusCode::NOT_FOUND, Vec::new()),
    }
}

fn response_with_status(
    status: tauri::http::StatusCode,
    body: Vec<u8>,
) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(status)
        .header(
            tauri::http::header::CONTENT_TYPE,
            "application/octet-stream",
        )
        .header(tauri::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(body)
        .unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
}
