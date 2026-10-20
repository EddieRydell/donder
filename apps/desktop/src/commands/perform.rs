use super::*;
use donder_sequence_api::PerformSnapshot;

#[tauri::command]
#[specta::specta]
pub(crate) fn get_perform_snapshot(state: State<'_, DesktopState>) -> PerformSnapshot {
    state.perform_snapshot()
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn start_performing(
    project_revision: u32,
    rig: u32,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<PerformSnapshot, String> {
    let owned = state.inner().clone();
    let snapshot =
        tauri::async_runtime::spawn_blocking(move || owned.start_performing(project_revision, rig))
            .await
            .map_err(|error| format!("Perform worker failed: {error}"))??;
    let _ = app.emit("perform_changed", snapshot.clone());
    let _ = app.emit("live_output_changed", state.live_output_snapshot());
    start_live_output_poll(app.clone());
    start_perform_poll(app);
    Ok(snapshot)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn stop_performing(app: AppHandle, state: State<'_, DesktopState>) -> PerformSnapshot {
    let snapshot = state.stop_performing();
    let _ = app.emit("perform_changed", snapshot.clone());
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
    let _ = app.emit("perform_changed", snapshot.clone());
    snapshot
}

#[tauri::command]
#[specta::specta]
pub(crate) fn perform_release_all(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> PerformSnapshot {
    let snapshot = state.perform_release_all();
    let _ = app.emit("perform_changed", snapshot.clone());
    snapshot
}

/// Ask for a MIDI song to perform; `None` when the user cancels.
#[tauri::command]
#[specta::specta]
pub(crate) fn choose_midi_song() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Play a MIDI song")
        .add_filter("MIDI song", &["mid", "midi"])
        .pick_file()
        .and_then(|path| path.to_str().map(ToString::to_string))
}

/// Ask for the SoundFont MIDI songs are heard on; `None` when the user cancels.
#[tauri::command]
#[specta::specta]
pub(crate) fn choose_song_sound_font() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Choose a SoundFont")
        .add_filter("SoundFont", &["sf2"])
        .pick_file()
        .and_then(|path| path.to_str().map(ToString::to_string))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn perform_play_song(
    path: String,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<PerformSnapshot, String> {
    let owned = state.inner().clone();
    let snapshot = tauri::async_runtime::spawn_blocking(move || owned.perform_play_song(&path))
        .await
        .map_err(|error| format!("Perform worker failed: {error}"))??;
    let _ = app.emit("perform_changed", snapshot.clone());
    Ok(snapshot)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn perform_stop_song(app: AppHandle, state: State<'_, DesktopState>) -> PerformSnapshot {
    let snapshot = state.perform_stop_song();
    let _ = app.emit("perform_changed", snapshot.clone());
    snapshot
}

/// Tell the window what changed after the Preview played a key.
pub(crate) fn publish_perform_snapshot(app: &AppHandle) {
    let _ = app.emit(
        "perform_changed",
        app.state::<DesktopState>().perform_snapshot(),
    );
}

/// Publishes the elapsed time while a performance runs.
fn start_perform_poll(app: AppHandle) {
    if !app.state::<DesktopState>().claim_perform_poll() {
        return;
    }
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(100));
            let state = app.state::<DesktopState>();
            let snapshot = state.perform_snapshot();
            let performing = snapshot.session.is_some();
            let _ = app.emit("perform_changed", snapshot);
            if !performing {
                state.release_perform_poll();
                let restarted = state.is_performing() && state.claim_perform_poll();
                if !restarted {
                    break;
                }
            }
        }
    });
}
