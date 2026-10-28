use super::*;

#[tauri::command]
#[specta::specta]
pub(crate) fn update_app_settings(
    settings: AppSettings,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state.update_app_settings(settings)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn search_project(
    request: ProjectSearchRequest,
    state: State<'_, DesktopState>,
) -> Result<ProjectSearchResponse, String> {
    state.search_project(request)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn plan_workspace_path_change(
    request: WorkspacePathChangeRequest,
    state: State<'_, DesktopState>,
) -> Result<WorkspacePathChangePlan, String> {
    state.plan_workspace_path_change(request)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn apply_workspace_path_change(
    request: WorkspacePathChangeRequest,
    state: State<'_, DesktopState>,
) -> Result<AppSnapshot, String> {
    state.apply_workspace_path_change(request)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn get_restored_view_state(state: State<'_, DesktopState>) -> ProjectRestoreState {
    state.restored_view_state()
}

#[tauri::command]
#[specta::specta]
pub(crate) fn create_file(
    parent: String,
    name: String,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state.create_file(&parent, &name)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn create_directory(
    parent: String,
    name: String,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state.create_directory(&parent, &name)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn delete_path(path: String, state: State<'_, DesktopState>) -> AppSnapshot {
    state.delete_path(&path)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn toggle_project_tree(state: State<'_, DesktopState>) -> AppSnapshot {
    state.update_snapshot(|snapshot| {
        snapshot.workspace_layout.sidebar_collapsed = !snapshot.workspace_layout.sidebar_collapsed;
    })
}
