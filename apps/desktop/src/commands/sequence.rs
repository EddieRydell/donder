use super::*;

#[tauri::command]
#[specta::specta]
pub(crate) fn sequence_export_options(
    request: GuiDocumentRequest,
    state: State<'_, DesktopState>,
) -> Result<donder_editor::dto::SequenceExportOptions, String> {
    state.sequence_export_options(&request)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn export_show_file(
    request: GuiDocumentRequest,
    outputs: Vec<u32>,
    state: State<'_, DesktopState>,
) -> Result<Option<String>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = state.prepare_show_export(&request, &outputs)?;
        save_export_file("Export show", "Donder show", "dondershow", &bytes)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn export_fseq_file(
    request: GuiDocumentRequest,
    outputs: Vec<u32>,
    step_millis: u8,
    progress: tauri::ipc::Channel<donder_editor::dto::ExportProgress>,
    state: State<'_, DesktopState>,
) -> Result<Option<String>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Rendering takes a while, so the destination is chosen before it starts.
        let Some(path) = choose_export_path("Export FSEQ sequence", "FSEQ sequence", "fseq")?
        else {
            return Ok(None);
        };
        // A closed dialog must not stop the export.
        let send = |stage| {
            let _ = progress.send(stage);
        };
        let bytes = state.prepare_fseq_export(&request, &outputs, step_millis, send)?;
        send(donder_editor::dto::ExportProgress::Saving);
        write_export_file(&path, &bytes)?;
        Ok(Some(path.to_string()))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn export_video_file(
    request: GuiDocumentRequest,
    appearance: donder_editor::dto::PreviewAppearance,
    frames_per_second: u32,
    progress: tauri::ipc::Channel<donder_editor::dto::ExportProgress>,
    state: State<'_, DesktopState>,
) -> Result<Option<String>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Rendering takes a while, so the destination is chosen before it starts.
        let Some(path) = choose_export_path("Export video", "MP4 video", "mp4")? else {
            return Ok(None);
        };
        // The video streams into a sibling file that replaces the destination
        // only once it is complete, and is removed if the export fails.
        let partial = camino::Utf8PathBuf::from(format!("{path}.part"));
        let file = std::fs::File::create(&partial)
            .map_err(|error| format!("Could not save the video: {error}"))?;
        let mut writer = std::io::BufWriter::new(file);
        let exported = state
            .prepare_video_export(
                &request,
                appearance,
                frames_per_second,
                &mut writer,
                |stage| {
                    // A closed dialog must not stop the export.
                    let _ = progress.send(stage);
                },
            )
            .and_then(|()| {
                std::io::Write::flush(&mut writer)
                    .map_err(|error| format!("Could not save the video: {error}"))
            })
            .and_then(|()| {
                std::fs::rename(&partial, &path)
                    .map_err(|error| format!("Could not save the video: {error}"))
            });
        if let Err(error) = exported {
            let _ = std::fs::remove_file(&partial);
            return Err(error);
        }
        Ok(Some(path.to_string()))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Ask for a destination with `extension` and write `bytes` there.
fn save_export_file(
    title: &str,
    filter: &str,
    extension: &str,
    bytes: &[u8],
) -> Result<Option<String>, String> {
    let Some(path) = choose_export_path(title, filter, extension)? else {
        return Ok(None);
    };
    write_export_file(&path, bytes)?;
    Ok(Some(path.to_string()))
}

/// Ask for a destination with `extension`; `None` when the user cancels.
fn choose_export_path(
    title: &str,
    filter: &str,
    extension: &str,
) -> Result<Option<camino::Utf8PathBuf>, String> {
    let Some(path) = rfd::FileDialog::new()
        .set_title(title)
        .add_filter(filter, &[extension])
        .set_file_name(format!("sequence.{extension}"))
        .save_file()
    else {
        return Ok(None);
    };
    let path = camino::Utf8PathBuf::from_path_buf(path).map_err(|_| "Choose a UTF-8 file path.")?;
    if path.extension() != Some(extension) {
        return Err(format!("Export files must use the .{extension} extension."));
    }
    Ok(Some(path))
}

fn write_export_file(path: &camino::Utf8Path, bytes: &[u8]) -> Result<(), String> {
    donder_project_io::atomic_write(path, bytes)
        .map_err(|error| format!("Could not save the export: {error}"))
}

#[tauri::command(async)]
#[specta::specta]
pub(crate) fn get_gui_document(
    request: GuiDocumentRequest,
    state: State<'_, DesktopState>,
) -> donder_editor::dto::GuiDocumentResult {
    state.get_gui_document(request)
}

#[tauri::command(async)]
#[specta::specta]
pub(crate) fn get_sequence_clip_details(
    request: GuiDocumentRequest,
    clip_ids: Vec<u32>,
    state: State<'_, DesktopState>,
) -> Result<donder_editor::dto::SequenceClipDetailsResult, String> {
    state.get_sequence_clip_details(request, clip_ids)
}

#[tauri::command(async)]
#[specta::specta]
pub(crate) fn match_sequence_clips(
    request: GuiDocumentRequest,
    query: donder_editor::dto::SequenceClipMatchRequest,
    state: State<'_, DesktopState>,
) -> Result<donder_editor::dto::SequenceSelection, String> {
    state.match_sequence_clips(request, query)
}

#[tauri::command(async)]
#[specta::specta]
pub(crate) fn request_sequence_clip_rasters(
    request: SequenceClipRasterRequest,
    state: State<'_, DesktopState>,
) -> SequenceClipRasterResponse {
    state.request_sequence_clip_rasters(request)
}

#[tauri::command(async)]
#[specta::specta]
pub(crate) fn take_sequence_clip_raster_results(
    request: GuiDocumentRequest,
    since_revision: u32,
    state: State<'_, DesktopState>,
) -> SequenceClipRasterResultBatch {
    state.take_sequence_clip_raster_results(request, since_revision)
}

#[tauri::command(async)]
#[specta::specta]
pub(crate) fn apply_gui_edit(
    request: GuiDocumentRequest,
    edit: GuiEditCommand,
    state: State<'_, DesktopState>,
) -> GuiEditUpdate {
    state.edit_gui_document(request, edit)
}

/// Apply the edit that waits for the person to confirm its changes to other
/// objects.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn confirm_gui_edit(
    request: GuiDocumentRequest,
    state: State<'_, DesktopState>,
) -> GuiEditUpdate {
    state.confirm_gui_edit(request)
}

/// Rename a member of the open project or setup, and its file to match.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn rename_gui_member(
    request: GuiDocumentRequest,
    source: GuiObjectRef,
    name: String,
    state: State<'_, DesktopState>,
) -> GuiEditUpdate {
    state.rename_gui_member(request, source, name)
}

/// Delete a member of the open project or setup once the person confirms.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn delete_gui_member(
    request: GuiDocumentRequest,
    source: GuiObjectRef,
    state: State<'_, DesktopState>,
) -> GuiEditUpdate {
    state.delete_gui_member(request, source)
}

/// What would refuse `edit` of the open object, a draft form's edit; none
/// when it would be accepted. Nothing changes.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn check_gui_edit(
    request: GuiDocumentRequest,
    edit: GuiEditCommand,
    state: State<'_, DesktopState>,
) -> Option<donder_editor::dto::GuiProblem> {
    state.check_gui_edit(&request, edit)
}

#[tauri::command(async)]
#[specta::specta]
pub(crate) fn finish_composition_graph_editing(state: State<'_, DesktopState>) -> AppSnapshot {
    state.finish_composition_graph_editing()
}

#[tauri::command(async)]
#[specta::specta]
pub(crate) fn copy_sequence_selection(
    request: GuiDocumentRequest,
    selection: SequenceSelection,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> GuiEditUpdate {
    let (update, text) = state.copy_sequence_selection(request, selection);
    write_clipboard(&app, &state, update, text)
}

/// Copy a sequence selection to the system clipboard and remove it.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn cut_sequence_selection(
    request: GuiDocumentRequest,
    selection: SequenceSelection,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> GuiEditUpdate {
    let (update, text) = state.cut_sequence_selection(request, selection);
    write_clipboard(&app, &state, update, text)
}

/// Paste the system clipboard into the open sequence; the pasted items become
/// the selection. A clipboard without Donder clips or marks pastes nothing.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn paste_sequence(
    request: GuiDocumentRequest,
    anchor: donder_editor::dto::SequencePasteAnchor,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> GuiEditUpdate {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    match app.clipboard().read_text() {
        Ok(text) if donder_project_io::holds_sequence_clipboard(&text) => {
            state.paste_sequence(request, anchor, &text)
        }
        _ => state.unchanged_gui_update(&request),
    }
}

/// `update`, once `text` is on the system clipboard; the status bar says
/// when the clipboard refused it.
fn write_clipboard(
    app: &AppHandle,
    state: &DesktopState,
    update: GuiEditUpdate,
    text: Option<String>,
) -> GuiEditUpdate {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    match text.map(|text| app.clipboard().write_text(text)) {
        Some(Err(_)) => GuiEditUpdate {
            snapshot: state.update_snapshot(|snapshot| {
                snapshot.status = "Couldn't copy to the clipboard.".to_string();
            }),
            ..update
        },
        _ => update,
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) fn choose_sequence_audio(
    request: GuiDocumentRequest,
    state: State<'_, DesktopState>,
) -> GuiEditUpdate {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("Audio", crate::audio::AUDIO_EXTENSIONS)
        .pick_file()
    else {
        return crate::desktop_state::gui_update(
            state.snapshot(),
            GuiDocumentChange::Document {
                document: state.get_gui_document(request).document,
            },
        );
    };
    let snapshot = state.snapshot();
    let import_path = match super::import_external_audio(&snapshot, &path) {
        Ok(path) => path,
        Err(message) => {
            let snapshot = state.update_snapshot(|snapshot| {
                snapshot.status = message;
            });
            return crate::desktop_state::gui_update(
                snapshot,
                GuiDocumentChange::Document {
                    document: state.get_gui_document(request).document,
                },
            );
        }
    };
    state.edit_gui_document(
        request,
        GuiEditCommand::Sequence {
            edit: SequenceGuiEdit::SetAudio {
                import_path: Some(import_path),
            },
        },
    )
}

/// A view that lists Donder controllers opened (`true`) or closed (`false`);
/// discovery runs while one is open.
#[tauri::command]
#[specta::specta]
pub(crate) fn watch_devices(watching: bool, state: State<'_, DesktopState>) {
    state.watch_devices(watching);
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn rename_device(
    id: String,
    name: String,
    state: State<'_, DesktopState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.rename_device(&id, &name))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn set_device_network(
    id: String,
    network: Option<donder_editor::dto::DonderDeviceNetworkRequest>,
    state: State<'_, DesktopState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.set_device_network(&id, network))
        .await
        .map_err(|error| error.to_string())?
}
