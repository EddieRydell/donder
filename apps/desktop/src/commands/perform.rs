use super::*;
use donder_editor::dto::PerformSnapshot;

#[tauri::command]
#[specta::specta]
pub(crate) fn get_perform_snapshot(state: State<'_, DesktopState>) -> PerformSnapshot {
    state.perform_snapshot()
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn start_performing(
    project_revision: u32,
    console: u32,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<PerformSnapshot, String> {
    let owned = state.inner().clone();
    let snapshot = tauri::async_runtime::spawn_blocking(move || {
        owned.start_performing(project_revision, console)
    })
    .await
    .map_err(|error| format!("Perform worker failed: {error}"))??;
    crate::events::send(
        &app,
        crate::events::PerformChanged {
            perform: snapshot.clone(),
        },
    );
    crate::events::send(
        &app,
        crate::events::LiveOutputChanged {
            output: state.live_output_snapshot(),
        },
    );
    start_live_output_poll(app.clone());
    start_perform_poll(app);
    Ok(snapshot)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn stop_performing(app: AppHandle, state: State<'_, DesktopState>) -> PerformSnapshot {
    let snapshot = state.stop_performing();
    crate::events::send(
        &app,
        crate::events::PerformChanged {
            perform: snapshot.clone(),
        },
    );
    start_live_output_poll(app);
    snapshot
}

#[tauri::command]
#[specta::specta]
pub(crate) fn perform_key(
    code: String,
    pressed: bool,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> PerformSnapshot {
    let snapshot = state.perform_key(&code, pressed);
    crate::events::send(
        &app,
        crate::events::PerformChanged {
            perform: snapshot.clone(),
        },
    );
    snapshot
}

#[tauri::command]
#[specta::specta]
pub(crate) fn perform_release_all(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> PerformSnapshot {
    let snapshot = state.perform_release_all();
    crate::events::send(
        &app,
        crate::events::PerformChanged {
            perform: snapshot.clone(),
        },
    );
    snapshot
}

/// Tell the window what changed after the Preview played a key.
pub(crate) fn publish_perform_snapshot(app: &AppHandle) {
    crate::events::send(
        app,
        crate::events::PerformChanged {
            perform: app.state::<DesktopState>().perform_snapshot(),
        },
    );
}

/// Publishes the elapsed time while a performance runs.
fn start_perform_poll(app: AppHandle) {
    super::start_poll(
        app,
        crate::desktop_state::Poll::Perform,
        Duration::from_millis(100),
        |app, state| {
            let snapshot = state.perform_snapshot();
            let performing = snapshot.session.is_some();
            crate::events::send(app, crate::events::PerformChanged { perform: snapshot });
            performing
        },
        DesktopState::is_performing,
    );
}
