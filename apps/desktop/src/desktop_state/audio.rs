use super::{DesktopState, lock_unpoisoned};
use crate::dto::{AppSnapshot, GuiDocumentRequest};

impl DesktopState {
    pub fn load_sequence_audio(&self, request: GuiDocumentRequest) -> AppSnapshot {
        let _authoring = lock_unpoisoned(&self.authoring);
        if request.project_revision != self.snapshot().project_revision
            || self.project_session().is_none()
        {
            return self.snapshot();
        }
        let audio = self.resolve_sequence_audio(&request);
        let sequence_id = self.resolve_sequence_id(&request);
        let project = self.project_session();
        let silent_duration = project
            .as_ref()
            .and_then(|project| {
                sequence_id
                    .as_ref()
                    .and_then(|id| project.project.sequences.get(id))
            })
            .filter(|sequence| {
                matches!(sequence.audio, dawn_language::sequence::SequenceAudio::None)
            })
            .map(|sequence| sequence.duration.as_seconds_f32());
        let audio_transport = match silent_duration {
            Some(duration) => lock_unpoisoned(&self.audio).load_silent_sequence(duration),
            None => lock_unpoisoned(&self.audio).load(audio),
        };
        match (project, sequence_id) {
            (Some(project), Some(sequence_id)) => {
                self.unload_render_session();
                self.schedule_sequence_render_prepare(project, sequence_id);
            }
            _ => {
                self.unload_render_session();
            }
        };
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
            snapshot.render_error = None;
        })
    }

    pub fn unload_audio(&self) -> AppSnapshot {
        let _authoring = lock_unpoisoned(&self.authoring);
        let audio_transport = lock_unpoisoned(&self.audio).unload();
        if self.snapshot().project_health == crate::dto::ProjectHealth::Ready {
            lock_unpoisoned(&self.workspace).render_target = None;
            self.unload_render_session();
        } else {
            self.invalidate_prepared_project();
        }
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
            snapshot.render_error = None;
        })
    }

    pub fn audio_play(&self) -> AppSnapshot {
        let audio_transport = lock_unpoisoned(&self.audio).play();
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn audio_pause(&self) -> AppSnapshot {
        let audio_transport = lock_unpoisoned(&self.audio).pause();
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn audio_stop(&self) -> AppSnapshot {
        let audio_transport = lock_unpoisoned(&self.audio).stop();
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn audio_rewind_to_zero(&self) -> AppSnapshot {
        let audio_transport = lock_unpoisoned(&self.audio).rewind_to_zero();
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn audio_seek(&self, position_seconds: f32) -> AppSnapshot {
        let audio_transport = lock_unpoisoned(&self.audio).seek(position_seconds);
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn render_current_sequence_frame(
        &self,
    ) -> Result<crate::rendering::AudioClockRenderedFrame, crate::rendering::SequenceRenderError>
    {
        self.render_refresh.finish_pending();
        let _authoring = lock_unpoisoned(&self.authoring);
        let audio_transport = self.audio_snapshot();
        lock_unpoisoned(&self.sequence_render).render_current_sequence_frame(&audio_transport)
    }

    pub fn active_preview_render_identity(
        &self,
    ) -> Result<crate::rendering::AudioClockRenderIdentity, crate::rendering::SequenceRenderError>
    {
        self.render_refresh.finish_pending();
        let _authoring = lock_unpoisoned(&self.authoring);
        let audio_transport = self.audio_snapshot();
        lock_unpoisoned(&self.sequence_render).active_render_identity(&audio_transport)
    }

    pub fn preview_scene(&self) -> Result<Option<crate::preview::PreviewScene>, String> {
        let _authoring = lock_unpoisoned(&self.authoring);
        let Some(session) = self.project_session() else {
            return Ok(None);
        };
        crate::preview::PreviewScene::from_project(self.project_revision(), &session.project)
            .map(Some)
    }

    pub fn preview_scene_revision(&self) -> Option<u64> {
        let _authoring = lock_unpoisoned(&self.authoring);
        self.project_session().map(|_| self.project_revision())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::{AudioTransportState, DocumentViewId, GuiDocument};
    use crate::project::{new_project_files, write_new_project_files};
    use camino::Utf8PathBuf;
    use std::{collections::BTreeMap, fs};

    #[test]
    fn dependency_sequence_resolves_and_plays_from_an_editable_copy() {
        let temporary = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
        let rig = root.join("rig");
        write_new_project_files(&root, &new_project_files("Show").unwrap()).unwrap();
        write_new_project_files(&rig, &new_project_files("Rig").unwrap()).unwrap();
        let dependency = dawn_project_io::load_package(&rig).unwrap().session;
        let setup = dependency.project.root.setup.clone();
        let sequence = dependency.project.root.sequences[0].clone();
        let mut manifest = dawn_package::PackageManifest::read(&rig).unwrap();
        manifest.project = None;
        manifest.exports = BTreeMap::from([
            (
                "setup".into(),
                dawn_package::ExportGroup {
                    documents: vec![setup.0.document().to_string()],
                },
            ),
            (
                "sequence".into(),
                dawn_package::ExportGroup {
                    documents: vec![sequence.0.document().to_string()],
                },
            ),
        ]);
        manifest.write(&rig).unwrap();
        fs::write(root.join("project.dawn"), format!("imports:\n- from: {{ dependency: rig, export: setup }}\n  as: setup\n- from: {{ dependency: rig, export: sequence }}\n  as: sequence\nshow:\n  type: project\n  setup: setup.{}\n  sequences: [sequence.{}]\n", setup.0.object(), sequence.0.object())).unwrap();
        let mut manifest = dawn_package::PackageManifest::read(&root).unwrap();
        manifest.dependencies.insert(
            "rig".into(),
            dawn_package::Dependency::Path { path: "rig".into() },
        );
        manifest.write(&root).unwrap();
        dawn_package::Lockfile::from_directory(&manifest, &root, "https://registry.dawn.dev")
            .unwrap()
            .write(&root)
            .unwrap();
        let state = DesktopState::new(|_| {});
        state.open_project_path(root.as_str());
        let loaded = state.project_session().unwrap();
        let imported = loaded.project.root.sequences[0].clone();
        assert_eq!(imported, sequence);
        assert!(!loaded.source.is_project_owned(imported.0.document_id()));
        let request = state
            .resolve_gui_source(
                &imported.0.module_id().to_string(),
                imported.0.document().as_str(),
                imported.0.object(),
            )
            .unwrap();
        let opened = state.open_file_path(&request.path);
        assert!(opened.active_buffer.as_ref().unwrap().read_only);
        assert_eq!(request.view, DocumentViewId::Sequence);
        let buffer = opened.active_buffer.as_ref().unwrap();
        let text_edit = state.update_document(crate::dto::DocumentUpdate {
            project_epoch: opened.project_epoch,
            path: request.path.clone(),
            expected_document_revision: buffer.document_revision,
            text: "invalid replacement".into(),
        });
        assert!(text_edit.unwrap_err().contains("read-only"));
        assert_eq!(state.snapshot().active_buffer.unwrap().text, buffer.text);
        let rejected = state.apply_gui_edit(
            request.clone(),
            crate::dto::GuiEditCommand::Sequence {
                edit: crate::dto::SequenceGuiEdit::SetDuration {
                    duration_seconds: 10.0,
                },
            },
        );
        assert!(
            matches!(rejected.document, GuiDocument::Blocked { ref reason, .. } if reason.contains("read-only"))
        );

        let projected = state.get_gui_document(request.clone()).document;
        assert!(
            matches!(projected, GuiDocument::Sequence { .. }),
            "{projected:?}"
        );
        assert_eq!(state.resolve_sequence_id(&request), Some(imported.clone()));
        let copy = root.parent().unwrap().join("editable");
        dawn_project_io::export_editable_project(&loaded, &copy).unwrap();
        assert_eq!(
            dawn_project_io::load_package(&root)
                .unwrap()
                .session
                .project,
            loaded.project
        );
        state.open_project_path(copy.as_str());
        let loaded = state.project_session().unwrap();
        let copied = loaded.project.root.sequences[0].clone();
        assert_ne!(copied, imported);
        let imported = copied;
        assert!(loaded.source.is_project_owned(imported.0.document_id()));
        let path = loaded
            .source
            .workspace_path_for_document(imported.0.document_id())
            .unwrap();
        state.open_file_path(path.as_str());
        let request = GuiDocumentRequest {
            project_revision: state.snapshot().project_revision,
            path: path.to_string(),
            view: DocumentViewId::Sequence,
            object_key: Some(imported.0.object().into()),
        };
        let projected = state.get_gui_document(request.clone()).document;
        assert!(
            matches!(projected, GuiDocument::Sequence { .. }),
            "{projected:?}"
        );
        assert_eq!(state.resolve_sequence_id(&request), Some(imported.clone()));
        let transport = state.load_sequence_audio(request.clone()).audio_transport;
        assert_eq!(
            transport.duration_seconds,
            loaded.project.sequences[&imported]
                .duration
                .as_seconds_f32()
        );
        assert_ne!(transport.state, AudioTransportState::Unloaded);
        assert!(transport.last_error.is_none());
        assert_eq!(
            state.audio_play().audio_transport.state,
            AudioTransportState::Playing
        );
        assert_eq!(
            state.audio_pause().audio_transport.state,
            AudioTransportState::Paused
        );
        let frame = state.render_current_sequence_frame().unwrap();
        assert_eq!(
            lock_unpoisoned(&state.sequence_render).active_target(),
            Some((loaded.project.root.setup.clone(), imported.clone()))
        );
        assert_eq!(
            frame.frame.frame_rate,
            loaded.project.sequences[&imported].frame_rate
        );
        assert!(state.active_preview_render_identity().unwrap().frame_count > 0);
        state.unload_audio();
        assert!(std::sync::Arc::ptr_eq(
            &loaded,
            &state.project_session().unwrap()
        ));
        let mut wrong_view = request.clone();
        wrong_view.view = DocumentViewId::Setup;
        assert!(state.resolve_sequence_id(&wrong_view).is_none());
        let mut stale = request;
        stale.project_revision += 1;
        assert!(state.resolve_sequence_id(&stale).is_none());
    }
}
