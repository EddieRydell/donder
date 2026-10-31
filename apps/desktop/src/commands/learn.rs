use super::*;
use donder_editor::dto::{GuiInputDriver, LearnedInput};

/// Wait for the next note, knob or OSC message `driver` sends and return the input it reads;
/// `None` when learning is cancelled or replaced. `token` names this request for
/// `cancel_input_learn`.
#[tauri::command]
#[specta::specta]
pub(crate) async fn learn_input(
    token: u32,
    driver: GuiInputDriver,
    state: State<'_, DesktopState>,
) -> Result<Option<LearnedInput>, String> {
    let receiver = state.begin_learning(token, &driver)?;
    tauri::async_runtime::spawn_blocking(move || receiver.recv().unwrap_or(None))
        .await
        .map_err(|error| format!("Input learning failed: {error}"))
}

#[tauri::command]
#[specta::specta]
pub(crate) fn cancel_input_learn(token: u32, state: State<'_, DesktopState>) {
    state.end_learning(Some(token));
}
