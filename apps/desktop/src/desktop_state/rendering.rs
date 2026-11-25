use std::sync::Arc;

use donder_project_io::ProjectSession;

use super::workspace_state::RenderTarget;
use super::{DesktopState, lock_unpoisoned};
use crate::state_tasks::RenderRefreshPayload;

impl DesktopState {
    pub(super) fn schedule_render_refresh(&self, project: Arc<ProjectSession>) {
        let Some(target) = lock_unpoisoned(&self.workspace).render_target.clone() else {
            return;
        };
        let Some(sequence_id) = target.resolve(&project) else {
            self.unload_render_session();
            return;
        };
        let snapshot = self.snapshot();
        let request = RenderRefreshPayload {
            project_epoch: snapshot.project_epoch,
            project_revision: snapshot.project_revision,
            project,
            sequence_id,
        };
        self.enqueue_render_refresh(request);
    }

    pub(super) fn schedule_sequence_render_prepare(
        &self,
        project: Arc<ProjectSession>,
        sequence_id: donder_model::SequenceId,
    ) {
        let target = RenderTarget::new(&project, sequence_id);
        let missing = target.is_none();
        lock_unpoisoned(&self.workspace).render_target = target;
        if missing {
            self.unload_render_session();
            return;
        }
        let snapshot = self.snapshot();
        let request = RenderRefreshPayload {
            project_epoch: snapshot.project_epoch,
            project_revision: snapshot.project_revision,
            project,
            sequence_id,
        };
        self.enqueue_render_refresh(request);
    }

    fn enqueue_render_refresh(&self, request: RenderRefreshPayload) {
        let failed = !self.render_refresh.schedule(request);
        if failed {
            self.set_render_error_if_changed("Render refresh worker is unavailable.".to_string());
        }
    }

    pub(super) fn complete_render_refresh(
        &self,
        request: RenderRefreshPayload,
        show: Option<donder_runtime::PreparedShow>,
    ) {
        let _authoring = lock_unpoisoned(&self.authoring);
        let snapshot = self.snapshot();
        if snapshot.project_epoch != request.project_epoch
            || snapshot.project_revision != request.project_revision
            || snapshot.project_health != donder_editor::dto::ProjectHealth::Ready
        {
            return;
        }
        let target = lock_unpoisoned(&self.workspace).render_target.clone();
        if target.and_then(|target| target.resolve(&request.project)) != Some(request.sequence_id) {
            return;
        }
        match show {
            Some(show) => {
                *lock_unpoisoned(&self.sequence) = Some(crate::playing::Playing {
                    id: donder_model::ShowId::Sequence(request.sequence_id),
                    show,
                    clock: crate::playing::PlayClock::Audio,
                    events: Vec::new(),
                    controllers: crate::playing::project_controllers(request.project.project()),
                    generation: self.next_playing_generation(),
                });
                self.clear_render_error_if_set();
                // A performance owns live output while it runs.
                if !self.is_performing() {
                    self.restart_live_output();
                }
                self.schedule_device_reconcile();
            }
            None => {
                self.stop_live_output();
                self.set_render_error_if_changed(
                    "Render refresh failed: sequence selection is unavailable".to_string(),
                );
            }
        }
        self.preview_wake.notify();
    }

    pub(super) fn unload_render_session(&self) {
        self.stop_live_output();
        *lock_unpoisoned(&self.sequence) = None;
        self.schedule_device_reconcile();
    }
}
