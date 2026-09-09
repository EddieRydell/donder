use super::*;

#[tauri::command]
#[specta::specta]
pub(crate) fn update_document(
    update: crate::dto::DocumentUpdate,
    state: State<'_, DesktopState>,
) -> Result<AppSnapshot, String> {
    state.update_document(update)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn save_all(state: State<'_, DesktopState>) -> Result<AppSnapshot, String> {
    state.save_all()
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn request_transition(
    request: crate::dto::TransitionRequest,
    state: State<'_, DesktopState>,
) -> Result<crate::dto::TransitionResult, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.request_transition(request))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
#[specta::specta]
pub(crate) fn reconcile_external_files(
    state: State<'_, DesktopState>,
) -> Result<AppSnapshot, String> {
    state.reconcile_external_files()
}

#[tauri::command]
#[specta::specta]
pub(crate) fn resolve_external_conflict(
    epoch: u32,
    path: String,
    revision: u32,
    decision: crate::dto::ExternalConflictDecision,
    state: State<'_, DesktopState>,
) -> Result<AppSnapshot, String> {
    state.resolve_external_conflict(epoch, path, revision, decision)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn complete_close(
    epoch: u32,
    revision: u32,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<(), String> {
    state.finish_close(epoch, revision, || {
        let window = app
            .get_window("main")
            .ok_or("The main window is unavailable")?;
        if let Some(geometry) = crate::persistence::read_window_state(&window) {
            state.persistence().record_main_window(geometry)?;
        }
        let preview = app.state::<crate::preview::PreviewWindowService>();
        preview
            .close_for_main_shutdown(&app, state.persistence())
            .map_err(|error| error.to_string())?;
        state.shutdown_live_output();
        window.destroy().map_err(|error| error.to_string())
    })
}

#[tauri::command]
#[specta::specta]
pub(crate) fn open_file(path: String, state: State<'_, DesktopState>) -> AppSnapshot {
    state.set_active_file_path(&path)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn resolve_gui_source(
    module_id: String,
    path: String,
    object_key: String,
    state: State<'_, DesktopState>,
) -> Result<GuiDocumentRequest, String> {
    state.resolve_gui_source(&module_id, &path, &object_key)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn set_active_file(path: String, state: State<'_, DesktopState>) -> AppSnapshot {
    state.set_active_file_path(&path)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn set_editor_view_mode(
    mode: EditorViewMode,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state.set_editor_view_mode(mode)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn save_editor_view_state(
    update: PersistedEditorViewStateUpdate,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state.save_editor_view_state(update)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn save_sequence_viewport_state(
    update: PersistedSequenceViewportStateUpdate,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state.save_sequence_viewport_state(update)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn undo_active_edit(state: State<'_, DesktopState>) -> AppSnapshot {
    state.undo_active_edit()
}

#[tauri::command]
#[specta::specta]
pub(crate) fn redo_active_edit(state: State<'_, DesktopState>) -> AppSnapshot {
    state.redo_active_edit()
}
