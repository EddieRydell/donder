use std::sync::Arc;

use donder_project_io::ProjectSession;

use super::{DesktopState, lock_unpoisoned};
use crate::state_tasks::RenderRefreshPayload;

impl DesktopState {
    pub(super) fn refresh_render_session(
        &self,
        project: &donder_language::model::DonderProject,
    ) -> Option<donder_elaboration::SequenceOutputPrepareError> {
        let mut rendering = lock_unpoisoned(&self.sequence_render);
        let result = rendering.refresh_project(project);
        if result.is_err() {
            rendering.unload();
        }
        result.err()
    }

    pub(super) fn schedule_render_refresh(&self, project: Arc<ProjectSession>) {
        let target = lock_unpoisoned(&self.workspace).render_target.clone();
        let Some((setup_id, sequence_id)) = target else {
            return;
        };
        if !project.project.sequences.contains_key(&sequence_id) {
            self.unload_render_session();
            return;
        }
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
        sequence_id: donder_language::sequence::SequenceId,
    ) {
        lock_unpoisoned(&self.workspace).render_target =
            Some((project.project.root.setup.clone(), sequence_id.clone()));
        if !project.project.sequences.contains_key(&sequence_id) {
            self.unload_render_session();
            return;
        }
        let snapshot = self.snapshot();
        let request = RenderRefreshPayload {
            project_epoch: snapshot.project_epoch,
            project_revision: snapshot.project_revision,
            setup_id: project.project.root.setup.clone(),
            project,
            sequence_id,
        };
        self.enqueue_render_refresh(request);
    }

    pub(super) fn enqueue_render_refresh(&self, request: RenderRefreshPayload) {
        let failed = !self.render_refresh.schedule(request);
        if failed {
            self.set_render_error_if_changed("Render refresh worker is unavailable.".to_string());
        }
    }

    pub(super) fn complete_render_refresh(
        &self,
        request: RenderRefreshPayload,
        result: Result<
            crate::rendering::PreparedSequenceOutput,
            donder_elaboration::SequenceOutputPrepareError,
        >,
    ) {
        let _authoring = lock_unpoisoned(&self.authoring);
        let snapshot = self.snapshot();
        if snapshot.project_epoch != request.project_epoch
            || snapshot.project_revision != request.project_revision
            || snapshot.project_health != crate::dto::ProjectHealth::Ready
        {
            return;
        }
        if lock_unpoisoned(&self.workspace).render_target.as_ref()
            != Some(&(request.setup_id, request.sequence_id))
        {
            return;
        }
        match result {
            Ok(session) => {
                lock_unpoisoned(&self.sequence_render).apply_prepared(session);
                self.clear_render_error_if_set();
                self.resume_live_output_after_prepare();
            }
            Err(error) => {
                self.suspend_live_output();
                self.set_render_error_if_changed(format!("Render refresh failed: {error:?}"));
            }
        }
    }

    pub(super) fn unload_render_session(&self) {
        self.disable_live_output();
        lock_unpoisoned(&self.sequence_render).unload();
    }
}
