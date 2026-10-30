use std::sync::Arc;

use donder_project_io::ProjectSession;

use super::workspace_state::RenderTarget;
use super::{DesktopState, lock_unpoisoned};
use crate::state_tasks::RenderRefreshPayload;

impl DesktopState {
    pub(super) fn refresh_render_session(
        &self,
        project: &donder_model::DonderProject,
    ) -> Option<crate::rendering::RenderSessionPrepareError> {
        let mut rendering = lock_unpoisoned(&self.sequence_render);
        let result = rendering.refresh_project(project);
        if result.is_err() {
            rendering.unload();
        }
        result.err()
    }

    pub(super) fn schedule_render_refresh(&self, project: Arc<ProjectSession>) {
        let Some(target) = lock_unpoisoned(&self.workspace).render_target.clone() else {
            return;
        };
        let Some((setup_id, sequence_id)) = target.resolve(&project).filter(|(_, sequence)| {
            project
                .project
                .get::<donder_model::Sequence>(*sequence)
                .is_some()
        }) else {
            self.unload_render_session();
            return;
        };
        let snapshot = self.snapshot();
        let request = RenderRefreshPayload {
            project_epoch: snapshot.project_epoch,
            project_revision: snapshot.project_revision,
            project,
            setup_id,
            sequence_id,
        };
        self.enqueue_render_refresh(request);
    }

    pub(super) fn schedule_sequence_render_prepare(
        &self,
        project: Arc<ProjectSession>,
        sequence_id: donder_model::SequenceId,
    ) {
        let setup_id = *project.project.root().setup.id();
        let target = RenderTarget::new(&project, setup_id, sequence_id);
        let missing = target.is_none()
            || project
                .project
                .get::<donder_model::Sequence>(sequence_id)
                .is_none();
        lock_unpoisoned(&self.workspace).render_target = target;
        if missing {
            self.unload_render_session();
            return;
        }
        let snapshot = self.snapshot();
        let request = RenderRefreshPayload {
            project_epoch: snapshot.project_epoch,
            project_revision: snapshot.project_revision,
            setup_id,
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
        result: Result<
            crate::rendering::PreparedRenderSession,
            crate::rendering::RenderSessionPrepareError,
        >,
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
        if target.and_then(|target| target.resolve(&request.project))
            != Some((request.setup_id, request.sequence_id))
        {
            return;
        }
        let result =
            result.map(|session| lock_unpoisoned(&self.sequence_render).apply_prepared(session));
        match result {
            Ok(()) => {
                self.clear_render_error_if_set();
                self.resume_live_output_after_prepare();
                self.schedule_device_reconcile();
            }
            Err(error) => {
                self.suspend_live_output();
                self.set_render_error_if_changed(format!("Render refresh failed: {error:?}"));
            }
        }
        self.preview_wake.notify();
    }

    pub(super) fn unload_render_session(&self) {
        self.disable_live_output();
        lock_unpoisoned(&self.sequence_render).unload();
        self.schedule_device_reconcile();
    }
}
