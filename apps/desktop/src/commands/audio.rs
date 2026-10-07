use super::*;

#[tauri::command]
#[specta::specta]
pub(crate) fn load_sequence_audio(
    request: GuiDocumentRequest,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    publish_audio_snapshot(&app, state.load_sequence_audio(request))
}

#[tauri::command]
#[specta::specta]
pub(crate) fn unload_audio(app: AppHandle, state: State<'_, DesktopState>) -> AppSnapshot {
    publish_audio_snapshot(&app, state.unload_audio())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn audio_play(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<AppSnapshot, String> {
    let owned = state.inner().clone();
    let snapshot = tauri::async_runtime::spawn_blocking(move || owned.audio_play())
        .await
        .map_err(|error| format!("Audio transport worker failed: {error}"))?;
    let snapshot = publish_audio_snapshot(&app, snapshot);
    if matches!(snapshot.audio_transport.state, AudioTransportState::Playing) {
        start_audio_transport_poll(app);
    }
    Ok(snapshot)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn audio_pause(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<AppSnapshot, String> {
    let owned = state.inner().clone();
    let snapshot = tauri::async_runtime::spawn_blocking(move || owned.audio_pause())
        .await
        .map_err(|error| format!("Audio transport worker failed: {error}"))?;
    Ok(publish_audio_snapshot(&app, snapshot))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn audio_stop(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<AppSnapshot, String> {
    let owned = state.inner().clone();
    let snapshot = tauri::async_runtime::spawn_blocking(move || owned.audio_stop())
        .await
        .map_err(|error| format!("Audio transport worker failed: {error}"))?;
    Ok(publish_audio_snapshot(&app, snapshot))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn audio_rewind_to_zero(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<AppSnapshot, String> {
    let owned = state.inner().clone();
    let snapshot = tauri::async_runtime::spawn_blocking(move || owned.audio_rewind_to_zero())
        .await
        .map_err(|error| format!("Audio transport worker failed: {error}"))?;
    Ok(publish_audio_snapshot(&app, snapshot))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn audio_seek(
    position_seconds: f32,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<AppSnapshot, String> {
    let owned = state.inner().clone();
    let snapshot = tauri::async_runtime::spawn_blocking(move || owned.audio_seek(position_seconds))
        .await
        .map_err(|error| format!("Audio transport worker failed: {error}"))?;
    Ok(publish_audio_snapshot(&app, snapshot))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn audio_set_playback_speed(
    speed: PlaybackSpeed,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<AppSnapshot, String> {
    let rate = donder_runtime::PlaybackRate::try_from(speed)?;
    let owned = state.inner().clone();
    let snapshot =
        tauri::async_runtime::spawn_blocking(move || owned.audio_set_playback_rate(rate))
            .await
            .map_err(|error| format!("Audio transport worker failed: {error}"))?;
    Ok(publish_audio_snapshot(&app, snapshot))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn audio_set_range(
    range: Option<PlaybackRange>,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<AppSnapshot, String> {
    let owned = state.inner().clone();
    let snapshot = tauri::async_runtime::spawn_blocking(move || owned.audio_set_range(range))
        .await
        .map_err(|error| format!("Audio transport worker failed: {error}"))??;
    Ok(publish_audio_snapshot(&app, snapshot))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn audio_set_looping(
    looping: bool,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<AppSnapshot, String> {
    let owned = state.inner().clone();
    let snapshot = tauri::async_runtime::spawn_blocking(move || owned.audio_set_looping(looping))
        .await
        .map_err(|error| format!("Audio transport worker failed: {error}"))?;
    Ok(publish_audio_snapshot(&app, snapshot))
}
