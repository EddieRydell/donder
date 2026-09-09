use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, State};
use tauri_specta::{Builder, collect_commands};

use crate::desktop_state::DesktopState;
use crate::dto::{
    AppSettings, AppSnapshot, AudioTransportState, DocumentViewId, EditorViewMode,
    GuiDocumentRequest, GuiEditCommand, GuiEditResult, NewSequenceRequest, ProjectSearchRequest,
    ProjectSearchResponse, SequenceAutomationMapping, SequenceAutomationTarget,
    SequenceClipRasterRequest, SequenceClipRasterResponse, SequenceClipRasterResultBatch,
    SequenceGuiEdit, SequenceSelectionEdit, SequenceSelectionEditResult, WorkspaceExplorerState,
    WorkspaceLayoutState, WorkspacePathChangePlan, WorkspacePathChangeRequest,
};
use crate::persistence::{
    PersistedEditorViewStateUpdate, PersistedPreviewWindowState,
    PersistedSequenceViewportStateUpdate, ProjectRestoreState,
};

mod app;
mod audio;
mod editor;
mod output;
mod packages;
mod preview;
mod project;
mod sequence;
mod workspace;

pub(crate) use app::*;
pub(crate) use audio::*;
pub(crate) use editor::*;
pub(crate) use output::*;
pub(crate) use packages::*;
pub(crate) use preview::*;
pub(crate) use project::*;
pub(crate) use sequence::*;
pub(crate) use workspace::*;

pub(crate) fn register(builder: Builder<tauri::Wry>) -> Builder<tauri::Wry> {
    builder.commands(collect_commands![
        get_snapshot,
        sync_packages,
        check_package_updates,
        update_packages,
        remove_package_dependency,
        fork_package_dependency,
        open_package_page,
        update_app_settings,
        save_workspace_layout_state,
        save_workspace_explorer_state,
        search_project,
        plan_workspace_path_change,
        apply_workspace_path_change,
        get_restored_view_state,
        open_project_dialog,
        choose_new_project_parent_directory,
        create_sequence,
        open_file,
        resolve_gui_source,
        set_active_file,
        update_document,
        save_all,
        request_transition,
        complete_close,
        reconcile_external_files,
        resolve_external_conflict,
        set_editor_view_mode,
        save_editor_view_state,
        save_sequence_viewport_state,
        undo_active_edit,
        redo_active_edit,
        get_gui_document,
        sequence_export_ports,
        export_sequence_file,
        device_capabilities,
        device_transport,
        device_serial_ports,
        device_firmware_info,
        install_device_firmware,
        provision_device,
        erase_device_saved_data,
        upload_sequence_device,
        request_sequence_clip_rasters,
        take_sequence_clip_raster_results,
        apply_gui_edit,
        finish_composition_graph_editing,
        rebind_detached_automation,
        discard_detached_automation,
        apply_sequence_selection_edit,
        choose_sequence_audio,
        create_file,
        create_directory,
        delete_path,
        toggle_project_tree,
        load_sequence_audio,
        unload_audio,
        audio_play,
        audio_pause,
        audio_stop,
        audio_rewind_to_zero,
        audio_seek,
        set_live_output_active,
        start_output_test,
        set_preview_window_open,
    ])
}

fn publish_audio_snapshot(app: &AppHandle, snapshot: AppSnapshot) -> AppSnapshot {
    let _ = app.emit("audio_transport_changed", snapshot.audio_transport.clone());
    snapshot
}

fn start_audio_transport_poll(app: AppHandle) {
    if !app.state::<DesktopState>().claim_audio_poll() {
        return;
    }
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(50));
            let state = app.state::<DesktopState>();
            let snapshot = state.audio_snapshot();
            let _ = app.emit("audio_transport_changed", snapshot.clone());
            if !matches!(snapshot.state, AudioTransportState::Playing) {
                state.release_audio_poll();
                let restarted =
                    matches!(state.audio_snapshot().state, AudioTransportState::Playing)
                        && state.claim_audio_poll();
                if !restarted {
                    break;
                }
            }
        }
    });
}

fn import_external_audio(snapshot: &AppSnapshot, selected_path: &Path) -> Result<String, String> {
    let selected = selected_path
        .canonicalize()
        .map_err(|error| format!("Could not read selected audio file: {error}"))?;
    let project_root = snapshot
        .project_root
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "No project is loaded.".to_string())?
        .canonicalize()
        .map_err(|error| format!("Could not resolve project directory: {error}"))?;

    if selected.starts_with(&project_root) {
        return selected
            .strip_prefix(&project_root)
            .ok()
            .and_then(|path| path.to_str())
            .map(|path| path.replace('\\', "/"))
            .ok_or_else(|| "Selected audio path is not valid UTF-8".to_string());
    }

    let file_name = selected
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Selected audio file name is not valid UTF-8".to_string())?;
    let audio_root = project_root.join("audio");
    fs::create_dir_all(&audio_root)
        .map_err(|error| format!("Could not create the project audio directory: {error}"))?;

    let destination = unique_audio_destination(&audio_root, file_name, &selected)?;
    if destination != selected {
        fs::copy(&selected, &destination)
            .map_err(|error| format!("Could not copy audio into the project: {error}"))?;
    }

    let relative_path = destination
        .strip_prefix(&project_root)
        .map_err(|_| "Copied audio is outside the project directory.".to_string())?
        .to_str()
        .ok_or_else(|| "Copied audio path is not valid UTF-8".to_string())?
        .replace('\\', "/");

    let project_root = camino::Utf8Path::from_path(&project_root)
        .ok_or_else(|| "Project path is not valid UTF-8".to_string())?;
    let mut manifest = dawn_package::PackageManifest::read(project_root)
        .map_err(|error| format!("Could not read project manifest: {error}"))?;
    manifest.assets.insert(
        relative_path.clone(),
        dawn_package::AssetDeclaration {
            kind: dawn_package::AssetKind::Audio,
        },
    );
    manifest
        .write(project_root)
        .map_err(|error| format!("Could not update project manifest: {error}"))?;
    Ok(relative_path)
}

fn unique_audio_destination(
    audio_root: &Path,
    file_name: &str,
    selected: &Path,
) -> Result<PathBuf, String> {
    let candidate = audio_root.join(file_name);
    if !candidate.exists() {
        return Ok(candidate);
    }
    if candidate
        .canonicalize()
        .is_ok_and(|existing| existing == selected)
    {
        return Ok(candidate);
    }

    let source = Path::new(file_name);
    let stem = source
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Selected audio file name is not valid UTF-8".to_string())?;
    let extension = source.extension().and_then(|value| value.to_str());
    for index in 1.. {
        let name = match extension {
            Some(extension) => format!("{stem}-{index}.{extension}"),
            None => format!("{stem}-{index}"),
        };
        let candidate = audio_root.join(name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("Could not choose a destination for the imported audio file.".to_string())
}
