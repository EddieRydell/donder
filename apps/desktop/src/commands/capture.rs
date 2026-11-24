use super::*;

#[tauri::command]
#[specta::specta]
pub(crate) fn start_fixture_capture(
    request: GuiDocumentRequest,
    controller: GuiObjectRef,
    start_channel: u32,
    pixels: u32,
    state: State<'_, DesktopState>,
) -> Result<AppSnapshot, String> {
    state.start_fixture_capture(&request, &controller, start_channel, pixels)
}

/// A phone video of the capture pattern; `None` when the user cancels.
#[tauri::command]
#[specta::specta]
pub(crate) fn choose_capture_video() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Choose the video of the pattern")
        .add_filter("Video", &["mp4", "mov", "m4v"])
        .pick_file()
        .and_then(|path| path.to_str().map(ToString::to_string))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn capture_fixture(
    request: GuiDocumentRequest,
    path: String,
    pixels: u32,
    state: State<'_, DesktopState>,
) -> Result<donder_editor::dto::GuiFixtureCapture, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.capture_fixture(&request, &path, pixels))
        .await
        .map_err(|error| format!("Fixture capture worker failed: {error}"))?
}
