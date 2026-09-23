use super::*;

#[tauri::command]
#[specta::specta]
pub(crate) fn sequence_export_ports(
    request: GuiDocumentRequest,
    state: State<'_, DesktopState>,
) -> Result<Vec<crate::dto::SequenceExportPort>, String> {
    state.sequence_export_ports(&request)
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
        let Some(path) = rfd::FileDialog::new()
            .set_title("Export compiled sequence")
            .add_filter("Donder compiled sequence", &["donderseq"])
            .set_file_name("sequence.donderseq")
            .save_file()
        else {
            return Ok(None);
        };
        let path =
            camino::Utf8PathBuf::from_path_buf(path).map_err(|_| "Choose a UTF-8 file path.")?;
        if path.extension() != Some("donderseq") {
            return Err("Export files must use the .donderseq extension.".into());
        }
        donder_package::atomic_write(&path, &bytes)
            .map_err(|error| format!("Could not save exported sequence: {error}"))?;
        Ok(Some(path.to_string()))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
#[specta::specta]
pub(crate) fn get_gui_document(
    request: GuiDocumentRequest,
    state: State<'_, DesktopState>,
) -> crate::dto::GuiDocumentResult {
    state.get_gui_document(request)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn request_sequence_clip_rasters(
    request: SequenceClipRasterRequest,
    state: State<'_, DesktopState>,
) -> SequenceClipRasterResponse {
    state.request_sequence_clip_rasters(request)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn take_sequence_clip_raster_results(
    request: GuiDocumentRequest,
    request_id: u32,
    state: State<'_, DesktopState>,
) -> SequenceClipRasterResultBatch {
    state.take_sequence_clip_raster_results(request, request_id)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn apply_gui_edit(
    request: GuiDocumentRequest,
    edit: GuiEditCommand,
    state: State<'_, DesktopState>,
) -> GuiEditResult {
    state.apply_gui_edit(request, edit)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn finish_composition_graph_editing(state: State<'_, DesktopState>) -> AppSnapshot {
    state.finish_composition_graph_editing()
}

#[tauri::command]
#[specta::specta]
pub(crate) fn rebind_detached_automation(
    request: GuiDocumentRequest,
    clip_id: u32,
    detached_index: u32,
    target: SequenceAutomationTarget,
    mapping: SequenceAutomationMapping,
    state: State<'_, DesktopState>,
) -> GuiEditResult {
    state.apply_gui_edit(
        request,
        GuiEditCommand::Sequence {
            edit: SequenceGuiEdit::RebindDetachedAutomation {
                clip_id,
                detached_index,
                target,
                mapping,
            },
        },
    )
}

#[tauri::command]
#[specta::specta]
pub(crate) fn discard_detached_automation(
    request: GuiDocumentRequest,
    clip_id: u32,
    detached_index: u32,
    state: State<'_, DesktopState>,
) -> GuiEditResult {
    state.apply_gui_edit(
        request,
        GuiEditCommand::Sequence {
            edit: SequenceGuiEdit::DiscardDetachedAutomation {
                clip_id,
                detached_index,
            },
        },
    )
}

#[tauri::command]
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
) -> GuiEditResult {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("Audio", &["mp3", "wav", "ogg", "flac"])
        .pick_file()
    else {
        return GuiEditResult {
            snapshot: state.snapshot(),
            document: state.get_gui_document(request).document,
        };
    };
    let snapshot = state.snapshot();
    let import_path = match super::import_external_audio(&snapshot, &path) {
        Ok(path) => path,
        Err(message) => {
            let snapshot = state.update_snapshot(|snapshot| {
                snapshot.status = message;
            });
            return GuiEditResult {
                snapshot,
                document: state.get_gui_document(request).document,
            };
        }
    };
    if !matches!(request.view, DocumentViewId::Sequence) {
        let snapshot = state.update_snapshot(|snapshot| {
            snapshot.status = "Audio can only be associated with a sequence.".to_string();
        });
        return GuiEditResult {
            snapshot,
            document: state.get_gui_document(request).document,
        };
    }
    state.apply_gui_edit(
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
pub(crate) async fn device_capabilities(
    address: String,
    token: String,
) -> Result<crate::dto::DeviceCapabilities, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::device::DeviceClient::new(&address, &token)?.capabilities()
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn device_transport(
    address: String,
    token: String,
    mode: Option<crate::dto::DevicePlaybackMode>,
) -> Result<crate::dto::DeviceTransportStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::device::DeviceClient::new(&address, &token)?.transport(mode)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn upload_sequence_device(
    request: GuiDocumentRequest,
    outputs: Vec<u32>,
    address: String,
    token: String,
    state: State<'_, DesktopState>,
) -> Result<String, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let client = crate::device::DeviceClient::new(&address, &token)?;
        let available = state.sequence_export_ports(&request)?;
        let widths = outputs
            .iter()
            .map(|index| {
                available
                    .iter()
                    .find(|port| port.index == *index)
                    .map(|port| port.channels)
                    .ok_or("Selected output is unavailable.")
            })
            .collect::<Result<Vec<_>, _>>()?;
        let bytes = state.prepare_sequence_export(&request, &outputs)?;
        client.upload(bytes, &widths)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn device_serial_ports() -> Result<Vec<crate::dto::DeviceSerialPort>, String> {
    tauri::async_runtime::spawn_blocking(crate::device::provisioning::ports)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
#[specta::specta]
pub(crate) fn device_firmware_info() -> Result<crate::dto::DeviceFirmwareInfo, String> {
    crate::device::firmware::info()
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn install_device_firmware(
    port: String,
    progress: tauri::ipc::Channel<crate::dto::DeviceInstallProgress>,
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

#[tauri::command]
#[specta::specta]
pub(crate) async fn provision_device(
    port: String,
    ssid: String,
    password: String,
) -> Result<crate::dto::ProvisionedDevice, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::device::provisioning::provision(&port, &ssid, &password)
    })
    .await
    .map_err(|error| error.to_string())?
}
