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
    crate::events::send(
        &app,
        crate::events::LiveOutputChanged {
            output: snapshot.live_output.clone(),
        },
    );
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
    crate::events::send(
        &app,
        crate::events::LiveOutputChanged {
            output: snapshot.live_output.clone(),
        },
    );
    start_live_output_poll(app);
    Ok(snapshot)
}

pub(crate) fn start_live_output_poll(app: AppHandle) {
    fn running(state: donder_editor::dto::LiveOutputState) -> bool {
        !matches!(
            state,
            donder_editor::dto::LiveOutputState::Disabled
                | donder_editor::dto::LiveOutputState::Error
        )
    }
    super::start_poll(
        app,
        crate::desktop_state::Poll::LiveOutput,
        Duration::from_millis(50),
        |app, state| {
            let snapshot = state.live_output_snapshot();
            let running = running(snapshot.state.clone());
            crate::events::send(app, crate::events::LiveOutputChanged { output: snapshot });
            running
        },
        |state| running(state.live_output_snapshot().state),
    );
}
