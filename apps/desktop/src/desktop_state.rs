//! Desktop application state aggregate.
//!
//! Workflow implementations live in the sibling modules under
//! `desktop_state/`; this root owns shared state, service construction, and
//! cross-workflow lifecycle coordination.

use std::sync::{
    Arc, Mutex, MutexGuard,
    atomic::{AtomicBool, Ordering},
};

use crate::dto::{
    AppSettings, AppSnapshot, AudioTransportSnapshot, AudioTransportState, PackageReadiness,
    PackageStatus, ProjectHealth, WorkspaceExplorerState, WorkspaceLayoutState,
};
use crate::persistence::PersistenceService;
use crate::state_tasks::{GuiHistory, LatestScheduler, RenderRefreshPayload, WorkingCopyPayload};
use camino::{Utf8Path, Utf8PathBuf};
use dawn_project_io::ProjectSession;
use workspace_state::WorkspaceState;

#[derive(Clone)]
pub(crate) enum LoadedProject {
    Closed,
    Ready(Arc<ProjectSession>),
    Invalid,
    Checking,
}

#[derive(Clone)]
pub(crate) struct DesktopState(Arc<DesktopServices>);

impl std::ops::Deref for DesktopState {
    type Target = DesktopServices;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub(crate) struct DesktopServices {
    workspace: Mutex<WorkspaceState>,
    authoring: Mutex<()>,
    on_snapshot: Box<dyn Fn(AppSnapshot) + Send + Sync>,
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
    weak: std::sync::Weak<DesktopServices>,
    gui_history: Mutex<GuiHistory>,
    working_copy: LatestScheduler<WorkingCopyPayload>,
    external_reconcile: LatestScheduler<(u32, Result<(), String>)>,
    render_refresh: LatestScheduler<RenderRefreshPayload>,
    audio: Arc<Mutex<crate::audio::AudioEngine>>,
    sequence_render: Arc<Mutex<crate::rendering::SequenceRenderService>>,
    live_output: Mutex<crate::output::LiveOutputService>,
    sequence_clip_raster: Mutex<crate::sequence_clip_raster::SequenceClipRasterService>,
    sequence_clipboard: Mutex<Option<crate::gui::SequenceClipboard>>,
    filesystem: Arc<Mutex<()>>,
    persistence: PersistenceService,
    preview_wake: crate::preview::PreviewWake,
    autosave_generation: std::sync::atomic::AtomicU32,
    audio_poll_running: AtomicBool,
    live_output_poll_running: AtomicBool,
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl DesktopState {
    pub(crate) fn new(on_snapshot: impl Fn(AppSnapshot) + Send + Sync + 'static) -> Self {
        let audio = Arc::new(Mutex::new(crate::audio::AudioEngine::new()));
        let sequence_render = Arc::new(Mutex::new(crate::rendering::SequenceRenderService::new()));
        let output = crate::output::LiveOutputService::new(audio.clone(), sequence_render.clone());
        let preview_wake = crate::preview::PreviewWake::new();
        Self(Arc::new_cyclic(
            |weak: &std::sync::Weak<DesktopServices>| {
                let analysis_state = weak.clone();
                let render_state = weak.clone();
                let external_state = weak.clone();
                DesktopServices {
                    workspace: Mutex::new(WorkspaceState::new(empty_snapshot())),
                    authoring: Mutex::new(()),
                    on_snapshot: Box::new(on_snapshot),
                    watcher: Mutex::new(None),
                    weak: weak.clone(),
                    gui_history: Mutex::new(GuiHistory::new(100)),
                    working_copy: LatestScheduler::new(move |request| {
                        let result = crate::state_tasks::analyze_working_copy(&request);
                        if let Some(state) = analysis_state.upgrade() {
                            DesktopState(state).complete_working_copy(request, result);
                        }
                    }),
                    external_reconcile: LatestScheduler::new(
                        move |(epoch, event): (u32, Result<(), String>)| {
                            if let Some(inner) = external_state.upgrade() {
                                let state = DesktopState(inner);
                                let _authoring = lock_unpoisoned(&state.authoring);
                                if state.snapshot().project_epoch != epoch {
                                    return;
                                }
                                if let Err(error) = event.and_then(|()| {
                                    state.reconcile_external_files_locked().map(|_| ())
                                }) {
                                    state.snapshot_with_error("project.watch", "", &error);
                                }
                            }
                        },
                    ),
                    render_refresh: LatestScheduler::new(move |request: RenderRefreshPayload| {
                        let result = crate::rendering::prepare_sequence_output(
                            &request.project.project,
                            &request.setup_id,
                            &request.sequence_id,
                        );
                        if let Some(state) = render_state.upgrade() {
                            DesktopState(state).complete_render_refresh(request, result);
                        }
                    }),
                    audio,
                    sequence_render,
                    live_output: Mutex::new(output),
                    sequence_clip_raster: Mutex::new(
                        crate::sequence_clip_raster::SequenceClipRasterService::new(),
                    ),
                    sequence_clipboard: Mutex::new(None),
                    filesystem: Arc::new(Mutex::new(())),
                    persistence: PersistenceService::new(),
                    preview_wake,
                    autosave_generation: std::sync::atomic::AtomicU32::new(0),
                    audio_poll_running: AtomicBool::new(false),
                    live_output_poll_running: AtomicBool::new(false),
                }
            },
        ))
    }

    pub fn persistence(&self) -> &PersistenceService {
        &self.persistence
    }

    pub(crate) fn preview_wake(&self) -> crate::preview::PreviewWake {
        self.preview_wake.clone()
    }

    pub fn apply_persisted_settings(&self) -> AppSnapshot {
        let settings = sanitize_app_settings(self.persistence.settings());
        let workspace_layout = sanitize_workspace_layout(self.persistence.workspace_layout());
        self.update_snapshot(|snapshot| {
            snapshot.settings = settings;
            snapshot.workspace_layout = workspace_layout;
        })
    }

    pub fn snapshot(&self) -> AppSnapshot {
        lock_unpoisoned(&self.workspace).snapshot()
    }

    pub fn audio_snapshot(&self) -> AudioTransportSnapshot {
        lock_unpoisoned(&self.audio).snapshot()
    }

    pub fn live_output_snapshot(&self) -> crate::dto::LiveOutputSnapshot {
        self.resume_live_output_if_ready();
        lock_unpoisoned(&self.live_output).snapshot()
    }

    pub(crate) fn claim_audio_poll(&self) -> bool {
        !self.audio_poll_running.swap(true, Ordering::AcqRel)
    }

    pub(crate) fn release_audio_poll(&self) {
        self.audio_poll_running.store(false, Ordering::Release);
    }

    pub(crate) fn claim_live_output_poll(&self) -> bool {
        !self.live_output_poll_running.swap(true, Ordering::AcqRel)
    }

    pub(crate) fn release_live_output_poll(&self) {
        self.live_output_poll_running
            .store(false, Ordering::Release);
    }

    fn merged_audio_snapshot(&self, previous: &AudioTransportSnapshot) -> AudioTransportSnapshot {
        let mut audio_transport = self.audio_snapshot();
        if matches!(audio_transport.state, AudioTransportState::Unloaded) {
            audio_transport.position_seconds = previous.position_seconds;
            audio_transport.home_seconds = previous.home_seconds;
        }
        audio_transport
    }

    pub fn update_snapshot(&self, update: impl FnOnce(&mut AppSnapshot)) -> AppSnapshot {
        let snapshot = {
            let mut workspace = lock_unpoisoned(&self.workspace);
            let mut snapshot = workspace.snapshot();
            update(&mut snapshot);
            snapshot.state_revision = snapshot.state_revision.saturating_add(1);
            snapshot.audio_transport = self.merged_audio_snapshot(&snapshot.audio_transport);
            if let Err(error) = self.persistence.record_snapshot(&snapshot) {
                snapshot.status = format!("Desktop state was not saved: {error}");
            }
            workspace.apply_view(snapshot.clone());
            workspace.refresh_gui_projection();
            workspace.snapshot()
        };
        self.preview_wake.notify();
        (self.on_snapshot)(snapshot.clone());
        snapshot
    }

    pub fn set_persistence_error(&self, message: String) -> AppSnapshot {
        self.update_snapshot(|snapshot| {
            snapshot.status = message;
        })
    }

    pub fn update_app_settings(&self, settings: AppSettings) -> AppSnapshot {
        let _authoring = lock_unpoisoned(&self.authoring);
        self.update_app_settings_locked(settings)
    }

    fn update_app_settings_locked(&self, settings: AppSettings) -> AppSnapshot {
        let settings = sanitize_app_settings(settings);
        let changed_autosave =
            self.snapshot().settings.autosave_project_edits != settings.autosave_project_edits;
        if changed_autosave {
            self.autosave_generation.fetch_add(1, Ordering::AcqRel);
        }
        if let Err(error) = self.persistence.record_settings(settings.clone()) {
            return self.set_persistence_error(format!("Settings were not saved: {error}"));
        }
        let enabled = settings.autosave_project_edits;
        let snapshot = self.update_snapshot(|snapshot| snapshot.settings = settings);
        if changed_autosave && enabled {
            self.schedule_working_copy(false);
        }
        snapshot
    }

    pub fn save_workspace_layout_state(&self, state: WorkspaceLayoutState) -> AppSnapshot {
        let state = sanitize_workspace_layout(state);
        match self.persistence.record_workspace_layout(state.clone()) {
            Ok(()) => self.update_snapshot(|snapshot| {
                snapshot.workspace_layout = state;
            }),
            Err(error) => {
                self.set_persistence_error(format!("Workspace layout was not saved: {error}"))
            }
        }
    }

    pub fn set_render_error_if_changed(&self, message: String) {
        let current = lock_unpoisoned(&self.workspace).view.render_error.clone();
        if current.as_deref() != Some(message.as_str()) {
            self.update_snapshot(|snapshot| {
                snapshot.render_error = Some(message);
            });
        }
    }

    pub fn clear_render_error_if_set(&self) {
        let current = lock_unpoisoned(&self.workspace).view.render_error.is_some();
        if current {
            self.update_snapshot(|snapshot| {
                snapshot.render_error = None;
            });
        }
    }

    pub fn set_live_output_active(&self, active: bool) -> Result<AppSnapshot, String> {
        let output = if active {
            let project = self.project_session().ok_or("No project is loaded.")?;
            let active = project
                .project
                .setups
                .get(&project.project.root.setup)
                .map(|setup| setup.controllers.clone());
            let render_ready = lock_unpoisoned(&self.sequence_render)
                .active_target()
                .is_some();
            let Some(active) = active.filter(|active| render_ready && !active.is_empty()) else {
                return Err(
                    "Live output requires a prepared sequence and at least one active controller."
                        .into(),
                );
            };
            let mut service = lock_unpoisoned(&self.live_output);
            if service.snapshot().state == crate::dto::LiveOutputState::Stopping {
                return Err("Wait for output to finish stopping before starting it again.".into());
            }
            service.enable(project.project.controllers.clone(), active)
        } else {
            lock_unpoisoned(&self.live_output).disable()
        };
        Ok(self.update_snapshot(|snapshot| snapshot.live_output = output))
    }

    pub(super) fn suspend_live_output(&self) {
        let output = lock_unpoisoned(&self.live_output).suspend();
        lock_unpoisoned(&self.workspace).view.live_output = output;
    }

    pub(super) fn disable_live_output(&self) {
        let output = lock_unpoisoned(&self.live_output).disable();
        lock_unpoisoned(&self.workspace).view.live_output = output;
    }

    pub(super) fn resume_live_output_after_prepare(&self) {
        lock_unpoisoned(&self.live_output).mark_prepared();
        self.resume_live_output_if_ready();
    }

    fn resume_live_output_if_ready(&self) {
        if lock_unpoisoned(&self.live_output).take_resume_after_prepare() {
            let _ = self.set_live_output_active(true);
        }
    }

    pub(crate) fn shutdown_live_output(&self) {
        lock_unpoisoned(&self.live_output).shutdown();
    }
}

mod audio;
mod diagnostics;
pub(super) use diagnostics::project_diagnostics;
mod editor_projection;
pub(super) use editor_projection::{descriptor_for_path, generated_source_texts};
mod filesystem;
mod gui_editing;
mod packages;
pub(crate) use packages::package_status;
mod output_test;
mod project_lifecycle;
mod rendering;
mod search;
mod sequence_export;
mod settings;
pub(super) use settings::{sanitize_app_settings, sanitize_workspace_layout};
mod transitions;
mod working_copy;
mod workspace;
mod workspace_projection;
mod workspace_state;
pub(super) use workspace_projection::{FsEntryKind, recovery_workspace_entries, workspace_entries};

fn empty_snapshot() -> AppSnapshot {
    AppSnapshot {
        gui_projection: None,
        state_revision: 0,
        project_epoch: 0,
        settings: AppSettings::default(),
        workspace_layout: WorkspaceLayoutState::default(),
        workspace_explorer: WorkspaceExplorerState::default(),
        project_root: None,
        project_health: ProjectHealth::Closed,
        project_revision: 0,
        project_entries: Vec::new(),
        tabs: Vec::new(),
        active_file: None,
        active_buffer: None,
        pending_saves: Vec::new(),
        active_document_descriptor: None,
        diagnostics: Vec::new(),
        status: "Ready".to_string(),
        render_error: None,
        preview_error: None,
        preview_open: false,
        audio_transport: crate::audio::AudioEngine::empty_snapshot(),
        live_output: crate::output::disabled_snapshot(0),
        package: PackageStatus {
            readiness: PackageReadiness::NoProject,
            root: None,
            manifest_valid: false,
            lock_present: false,
            lock_current: false,
            registry: None,
            update_checked: false,
            dependencies: Vec::new(),
            modules: Vec::new(),
            warnings: Vec::new(),
            message: None,
        },
    }
}

fn absolute_project_path(
    session: &ProjectSession,
    relative_path: &Utf8Path,
) -> Option<Utf8PathBuf> {
    absolute_root_path(session.source.project_root(), relative_path)
}

fn absolute_root_path(root: &Utf8Path, relative_path: &Utf8Path) -> Option<Utf8PathBuf> {
    if relative_path.as_str().is_empty()
        || relative_path.is_absolute()
        || relative_path.as_str().contains('\\')
        || !relative_path
            .components()
            .all(|component| matches!(component, camino::Utf8Component::Normal(_)))
    {
        return None;
    }
    Some(root.join(relative_path))
}

fn valid_child_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains('\\') && name != "." && name != ".."
}

fn path_matches_or_is_child(candidate: &str, parent: &str) -> bool {
    candidate == parent
        || candidate
            .strip_prefix(parent)
            .is_some_and(|suffix| suffix.starts_with('/') || suffix.starts_with('\\'))
}

#[cfg(test)]
mod authoring_acceptance;
#[cfg(test)]
mod fixture_copy_acceptance;
#[cfg(test)]
mod project_copy_acceptance;

#[cfg(test)]
mod control_output_acceptance;

#[cfg(test)]
mod advanced_patch_acceptance;
#[cfg(test)]
mod color_prop_acceptance;
#[cfg(test)]
mod fixture_color_acceptance;
