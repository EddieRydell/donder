use super::*;

#[tauri::command]
#[specta::specta]
pub(crate) fn open_project_dialog() -> Option<String> {
    let path = rfd::FileDialog::new()
        .add_filter("Donder package manifest", &["json"])
        .set_file_name(donder_package::MANIFEST_FILE)
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

#[tauri::command]
#[specta::specta]
pub(crate) fn create_sequence(
    request: NewSequenceRequest,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state.create_sequence(request)
}
