use super::*;

#[tauri::command]
#[specta::specta]
pub(crate) fn start_output_test(
    request: GuiDocumentRequest,
    test: donder_editor::dto::ControllerOutputTest,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<AppSnapshot, String> {
    let snapshot = state.start_output_test(&request, test)?;
    let _ = app.emit("live_output_changed", snapshot.live_output.clone());
    start_live_output_poll(app);
    Ok(snapshot)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn set_live_output_active(
    active: bool,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<AppSnapshot, String> {
    let snapshot = state.set_live_output_active(active)?;
    let _ = app.emit("live_output_changed", snapshot.live_output.clone());
    start_live_output_poll(app);
    Ok(snapshot)
}

pub(crate) fn start_live_output_poll(app: AppHandle) {
    if !app.state::<DesktopState>().claim_live_output_poll() {
        return;
    }
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(50));
            let state = app.state::<DesktopState>();
            let snapshot = state.live_output_snapshot();
            let _ = app.emit("live_output_changed", snapshot.clone());
            if matches!(
                snapshot.state,
                donder_editor::dto::LiveOutputState::Disabled
                    | donder_editor::dto::LiveOutputState::Error
            ) {
                state.release_live_output_poll();
                let restarted = !matches!(
                    state.live_output_snapshot().state,
                    donder_editor::dto::LiveOutputState::Disabled
                        | donder_editor::dto::LiveOutputState::Error
                ) && state.claim_live_output_poll();
                if !restarted {
                    break;
                }
            }
        }
    });
}
