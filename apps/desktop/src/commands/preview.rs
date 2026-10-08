use super::*;

#[tauri::command]
#[specta::specta]
pub(crate) fn set_preview_appearance(
    appearance: donder_sequence_api::PreviewAppearance,
    app: AppHandle,
    preview: State<'_, crate::preview::PreviewWindowService>,
    state: State<'_, DesktopState>,
) -> Result<AppSnapshot, String> {
    preview.set_appearance(appearance)?;
    let persisted = state.persistence().preview_window();
    let snapshot = state.snapshot();
    if snapshot.settings.reopen_preview_window && persisted.open && !snapshot.preview_open {
        preview.open_or_focus(app, persisted)?;
        return Ok(state.update_snapshot(|snapshot| {
            snapshot.preview_open = true;
            snapshot.preview_error = None;
        }));
    }
    Ok(state.snapshot())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn set_preview_window_open(
    enabled: bool,
    app: AppHandle,
    preview: State<'_, crate::preview::PreviewWindowService>,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    let restore = state.persistence().preview_window();
    let result = if enabled {
        preview.open_or_focus(
            app.clone(),
            PersistedPreviewWindowState {
                open: true,
                ..restore
            },
        )
    } else {
        preview.close(state.persistence())
    };
    match result {
        Ok(()) => state.update_snapshot(|snapshot| {
            snapshot.preview_open = enabled;
            snapshot.preview_error = None;
        }),
        Err(error) => state.update_snapshot(|snapshot| {
            snapshot.preview_error = Some(format!("Preview failed: {error}"));
        }),
    }
}
