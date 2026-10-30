use super::*;

#[tauri::command]
#[specta::specta]
pub(crate) fn open_project_dialog() -> Option<String> {
    let path = rfd::FileDialog::new()
        .add_filter("Donder project", &["donder"])
        .set_file_name(donder_project_io::PROJECT_ROOT_FILE)
        .pick_file()?;
    path.to_str().map(ToString::to_string)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn choose_new_project_parent_directory() -> Option<String> {
    rfd::FileDialog::new()
        .pick_folder()
        .and_then(|path| path.to_str().map(ToString::to_string))
}

/// The folder of an xLights show, holding `xlights_rgbeffects.xml`.
#[tauri::command]
#[specta::specta]
pub(crate) fn choose_xlights_show_directory() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Choose the xLights show folder")
        .pick_folder()
        .and_then(|path| path.to_str().map(ToString::to_string))
}

#[tauri::command]
#[specta::specta]
pub(crate) fn create_sequence(
    request: NewSequenceRequest,
    state: State<'_, DesktopState>,
) -> Result<donder_editor::dto::NewSequenceResult, String> {
    state.create_sequence(request)
}
