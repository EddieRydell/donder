use fs_err as fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tauri::{AppHandle, Manager, State};
use tauri_specta::{Builder, collect_commands};

use crate::desktop_state::DesktopState;
use crate::persistence::{PersistedPreviewWindowState, ProjectRestoreState};
use donder_editor::dto::{
    AppSettings, AppSnapshot, AudioTransportState, EditorViewMode, GuiDocumentChange,
    GuiDocumentRequest, GuiEditCommand, GuiEditUpdate, GuiObjectRef, PlaybackRange, PlaybackSpeed,
    ProjectSearchRequest, ProjectSearchResponse, SequenceClipRasterRequest,
    SequenceClipRasterResponse, SequenceClipRasterResultBatch, SequenceGuiEdit, SequenceSelection,
    ViewStateUpdate, WorkspacePathChangePlan, WorkspacePathChangeRequest,
};

mod app;
mod audio;
mod capture;
mod editor;
mod learn;
mod output;
mod perform;
mod preview;
mod project;
mod sequence;
mod workspace;

pub(crate) use app::*;
pub(crate) use audio::*;
pub(crate) use capture::*;
pub(crate) use editor::*;
pub(crate) use learn::*;
pub(crate) use output::*;
pub(crate) use perform::*;
pub(crate) use preview::*;
pub(crate) use project::*;
pub(crate) use sequence::*;
pub(crate) use workspace::*;

pub(crate) fn register(builder: Builder<tauri::Wry>) -> Builder<tauri::Wry> {
    builder.commands(collect_commands![
        get_snapshot,
        update_app_settings,
        search_project,
        plan_workspace_path_change,
        apply_workspace_path_change,
        get_restored_view_state,
        open_project_dialog,
        choose_new_project_parent_directory,
        choose_profile_directory,
        open_file,
        move_tab,
        resolve_gui_source,
        update_document,
        document_text,
        language_server_send,
        apply_text_edits,
        include_document,
        save_all,
        request_transition,
        complete_close,
        reconcile_external_files,
        resolve_external_conflict,
        set_editor_view_mode,
        save_view_state,
        undo_active_edit,
        redo_active_edit,
        get_gui_document,
        get_sequence_clip_details,
        match_sequence_clips,
        sequence_export_options,
        export_show_file,
        export_fseq_file,
        export_video_file,
        watch_devices,
        rename_device,
        set_device_network,
        request_sequence_clip_rasters,
        take_sequence_clip_raster_results,
        apply_gui_edit,
        confirm_gui_edit,
        rename_gui_member,
        delete_gui_member,
        check_gui_edit,
        finish_composition_graph_editing,
        copy_sequence_selection,
        cut_sequence_selection,
        paste_sequence,
        choose_sequence_audio,
        choose_font_file,
        choose_image_file,
        create_file,
        create_directory,
        delete_path,
        plan_delete_path,
        toggle_project_tree,
        load_sequence_audio,
        unload_audio,
        detect_sequence_beats,
        audio_play,
        audio_pause,
        audio_stop,
        audio_rewind_to_zero,
        audio_seek,
        audio_set_playback_speed,
        audio_set_range,
        audio_set_looping,
        set_preview_appearance,
        set_live_output_active,
        start_output_test,
        start_fixture_capture,
        choose_capture_video,
        capture_fixture,
        set_preview_window_open,
        get_perform_snapshot,
        start_performing,
        stop_performing,
        perform_key,
        perform_release_all,
        learn_input,
        cancel_input_learn,
    ])
}

fn publish_audio_snapshot(app: &AppHandle, snapshot: AppSnapshot) -> AppSnapshot {
    crate::events::send(
        app,
        crate::events::AudioTransportChanged {
            transport: snapshot.audio_transport.clone(),
        },
    );
    snapshot
}

/// Every `period`, run `tick`, which publishes and says whether to keep
/// polling; once it stops, poll again if `active` became true meanwhile.
pub(crate) fn start_poll(
    app: AppHandle,
    poll: crate::desktop_state::Poll,
    period: Duration,
    tick: fn(&AppHandle, &DesktopState) -> bool,
    active: fn(&DesktopState) -> bool,
) {
    if !app.state::<DesktopState>().claim_poll(poll) {
        return;
    }
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(period);
            let state = app.state::<DesktopState>();
            if !tick(&app, &state) {
                state.release_poll(poll);
                if !(active(&state) && state.claim_poll(poll)) {
                    break;
                }
            }
        }
    });
}

fn start_audio_transport_poll(app: AppHandle) {
    start_poll(
        app,
        crate::desktop_state::Poll::Audio,
        Duration::from_millis(50),
        |app, state| {
            let snapshot = state.poll_audio_transport();
            let playing = matches!(snapshot.state, AudioTransportState::Playing);
            crate::events::send(
                app,
                crate::events::AudioTransportChanged {
                    transport: snapshot,
                },
            );
            playing
        },
        |state| matches!(state.audio_snapshot().state, AudioTransportState::Playing),
    );
}

/// The project-relative path of the file at `selected_path`, copied into the
/// project's `directory` when it is outside the project.
fn import_external_asset(
    snapshot: &AppSnapshot,
    selected_path: &Path,
    directory: &str,
) -> Result<String, String> {
    let selected = fs::canonicalize(selected_path)
        .map_err(|error| format!("Could not read the selected file: {error}"))?;
    let project_root = snapshot
        .project_root
        .as_deref()
        .map(fs::canonicalize)
        .ok_or_else(|| "No project is loaded.".to_string())?
        .map_err(|error| format!("Could not resolve project directory: {error}"))?;

    if selected.starts_with(&project_root) {
        return selected
            .strip_prefix(&project_root)
            .ok()
            .and_then(|path| path.to_str())
            .map(|path| path.replace('\\', "/"))
            .ok_or_else(|| "The selected path is not valid UTF-8".to_string());
    }

    let file_name = selected
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "The selected file name is not valid UTF-8".to_string())?;
    let asset_root = project_root.join(directory);
    fs::create_dir_all(&asset_root)
        .map_err(|error| format!("Could not create the project {directory} directory: {error}"))?;

    let destination = unique_asset_destination(&asset_root, file_name, &selected)?;
    if destination != selected {
        fs::copy(&selected, &destination)
            .map_err(|error| format!("Could not copy the file into the project: {error}"))?;
    }

    let relative_path = destination
        .strip_prefix(&project_root)
        .map_err(|_| "The copied file is outside the project directory.".to_string())?
        .to_str()
        .ok_or_else(|| "The copied path is not valid UTF-8".to_string())?
        .replace('\\', "/");

    Ok(relative_path)
}

fn unique_asset_destination(
    asset_root: &Path,
    file_name: &str,
    selected: &Path,
) -> Result<PathBuf, String> {
    let candidate = asset_root.join(file_name);
    if !candidate.exists() {
        return Ok(candidate);
    }
    if fs::canonicalize(&candidate).is_ok_and(|existing| existing == selected) {
        return Ok(candidate);
    }

    let source = Path::new(file_name);
    let stem = source
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "The selected file name is not valid UTF-8".to_string())?;
    let extension = source.extension().and_then(|value| value.to_str());
    for index in 1.. {
        let name = match extension {
            Some(extension) => format!("{stem}-{index}.{extension}"),
            None => format!("{stem}-{index}"),
        };
        let candidate = asset_root.join(name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("Could not choose a destination for the imported file.".to_string())
}
