use super::*;

#[tauri::command]
#[specta::specta]
pub(crate) fn sequence_export_options(
    request: GuiDocumentRequest,
    state: State<'_, DesktopState>,
) -> Result<donder_sequence_api::SequenceExportOptions, String> {
    state.sequence_export_options(&request)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn export_sequence_file(
    request: GuiDocumentRequest,
    outputs: Vec<u32>,
    state: State<'_, DesktopState>,
) -> Result<Option<String>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = state.prepare_sequence_export(&request, &outputs)?;
        save_export_file(
            "Export compiled sequence",
            "Donder compiled sequence",
            "donderseq",
            &bytes,
        )
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
    state: State<'_, DesktopState>,
) -> Result<Option<String>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = state.prepare_fseq_export(&request, &outputs, step_millis)?;
        save_export_file("Export FSEQ sequence", "FSEQ sequence", "fseq", &bytes)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn export_video_file(
    request: GuiDocumentRequest,
    appearance: donder_sequence_api::PreviewAppearance,
    state: State<'_, DesktopState>,
) -> Result<Option<String>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = state.prepare_video_export(&request, appearance)?;
        save_export_file("Export video", "MP4 video", "mp4", &bytes)
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
    donder_project_io::atomic_write(&path, bytes)
        .map_err(|error| format!("Could not save exported sequence: {error}"))?;
    Ok(Some(path.to_string()))
}

#[tauri::command(async)]
#[specta::specta]
pub(crate) fn get_gui_document(
    request: GuiDocumentRequest,
    state: State<'_, DesktopState>,
) -> donder_sequence_api::GuiDocumentResult {
    state.get_gui_document(request)
}

#[tauri::command(async)]
#[specta::specta]
pub(crate) fn get_sequence_effect_details(
    request: GuiDocumentRequest,
    effect_ids: Vec<u32>,
    state: State<'_, DesktopState>,
) -> Result<donder_sequence_api::SequenceEffectDetailsResult, String> {
    state.get_sequence_effect_details(request, effect_ids)
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

#[tauri::command(async)]
#[specta::specta]
pub(crate) fn finish_composition_graph_editing(state: State<'_, DesktopState>) -> AppSnapshot {
    state.finish_composition_graph_editing()
}

#[tauri::command(async)]
#[specta::specta]
pub(crate) fn rebind_detached_automation(
    request: GuiDocumentRequest,
    clip_id: u32,
    detached_index: u32,
    target: SequenceAutomationTarget,
    state: State<'_, DesktopState>,
) -> GuiEditUpdate {
    state.edit_gui_document(
        request,
        GuiEditCommand::Sequence {
            edit: SequenceGuiEdit::RebindDetachedAutomation {
                clip_id,
                detached_index,
                target,
            },
        },
    )
}

#[tauri::command(async)]
#[specta::specta]
pub(crate) fn discard_detached_automation(
    request: GuiDocumentRequest,
    clip_id: u32,
    detached_index: u32,
    state: State<'_, DesktopState>,
) -> GuiEditUpdate {
    state.edit_gui_document(
        request,
        GuiEditCommand::Sequence {
            edit: SequenceGuiEdit::DiscardDetachedAutomation {
                clip_id,
                detached_index,
            },
        },
    )
}

#[tauri::command(async)]
#[specta::specta]
pub(crate) fn apply_sequence_selection_edit(
    request: GuiDocumentRequest,
    edit: SequenceSelectionEdit,
    state: State<'_, DesktopState>,
) -> SequenceSelectionEditResult {
    state.apply_sequence_selection_edit(request, edit)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn choose_sequence_audio(
    request: GuiDocumentRequest,
    state: State<'_, DesktopState>,
) -> GuiEditUpdate {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("Audio", &["mp3", "wav", "ogg", "flac"])
        .pick_file()
    else {
        return GuiEditUpdate {
            snapshot: state.snapshot(),
            change: GuiDocumentChange::Document {
                document: state.get_gui_document(request).document,
            },
        };
    };
    let snapshot = state.snapshot();
    let import_path = match super::import_external_audio(&snapshot, &path) {
        Ok(path) => path,
        Err(message) => {
            let snapshot = state.update_snapshot(|snapshot| {
                snapshot.status = message;
            });
            return GuiEditUpdate {
                snapshot,
                change: GuiDocumentChange::Document {
                    document: state.get_gui_document(request).document,
                },
            };
        }
    };
    if !matches!(request.view, DocumentViewId::Sequence) {
        let snapshot = state.update_snapshot(|snapshot| {
            snapshot.status = "Audio can only be associated with a sequence.".to_string();
        });
        return GuiEditUpdate {
            snapshot,
            change: GuiDocumentChange::Document {
                document: state.get_gui_document(request).document,
            },
        };
    }
    state.edit_gui_document(
        request,
        GuiEditCommand::Sequence {
            edit: SequenceGuiEdit::SetAudio {
                import_path: Some(import_path),
            },
        },
    )
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn claim_device(
    id: String,
    state: State<'_, DesktopState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.claim_device(&id))
        .await
        .map_err(|error| error.to_string())?
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
pub(crate) async fn set_device_standalone(
    id: String,
    playing: bool,
    state: State<'_, DesktopState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.set_device_standalone(&id, playing))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn set_device_network(
    id: String,
    network: Option<donder_sequence_api::DonderDeviceNetworkRequest>,
    state: State<'_, DesktopState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.set_device_network(&id, network))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn device_serial_ports()
-> Result<Vec<donder_sequence_api::DeviceSerialPort>, String> {
    tauri::async_runtime::spawn_blocking(crate::device::provisioning::ports)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
#[specta::specta]
pub(crate) fn device_firmware_info() -> Result<donder_sequence_api::DeviceFirmwareInfo, String> {
    crate::device::firmware::info()
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn install_device_firmware(
    port: String,
    progress: tauri::ipc::Channel<donder_sequence_api::DeviceInstallProgress>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::device::firmware::install(&port, |state| {
            // A disconnected UI must not interrupt a flash write.
            let _ = progress.send(state);
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn erase_device_saved_data(port: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::device::provisioning::erase_saved_data(&port)
    })
    .await
    .map_err(|error| error.to_string())?
}
