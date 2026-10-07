use super::{DesktopState, lock_unpoisoned};
use crate::dto::{AppSnapshot, GuiDocumentRequest};

impl DesktopState {
    pub fn load_sequence_audio(&self, request: GuiDocumentRequest) -> AppSnapshot {
        let _operation = lock_unpoisoned(&self.transport_operation);
        let _authoring = lock_unpoisoned(&self.authoring);
        if request.project_revision != self.snapshot().project_revision
            || self.project_session().is_none()
        {
            return self.snapshot();
        }
        let audio = self.resolve_sequence_audio(&request);
        let sequence_id = self.resolve_sequence_id(&request);
        if let Err(error) = self
            .device_playback
            .stop_for_source(self.snapshot().project_epoch, sequence_id.as_ref())
        {
            return self.device_transport_error(error);
        }
        let project = self.project_session();
        let silent_duration = project
            .as_ref()
            .and_then(|project| {
                sequence_id
                    .as_ref()
                    .and_then(|id| project.project.sequence(id))
            })
            .filter(|sequence| {
                matches!(
                    sequence.audio,
                    donder_language::sequence::SequenceAudio::None
                )
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
        let _operation = lock_unpoisoned(&self.transport_operation);
        let _authoring = lock_unpoisoned(&self.authoring);
        if let Err(error) = self
            .device_playback
            .stop_for_source(self.snapshot().project_epoch, None)
        {
            return self.device_transport_error(error);
        }
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
        if self.device_playback.has_devices() || self.has_setup_devices() {
            return self
                .device_audio_play()
                .unwrap_or_else(|error| self.device_transport_error(error));
        }
        let audio_transport = lock_unpoisoned(&self.audio).play();
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn audio_pause(&self) -> AppSnapshot {
        if self.device_playback.has_devices() {
            return self
                .device_audio_hold(crate::dto::DevicePlaybackMode::Paused, None, false)
                .unwrap_or_else(|error| self.device_transport_error(error));
        }
        let audio_transport = lock_unpoisoned(&self.audio).pause();
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn audio_stop(&self) -> AppSnapshot {
        if self.device_playback.has_devices() {
            return self
                .device_audio_hold(
                    crate::dto::DevicePlaybackMode::Stopped,
                    Some(self.audio_snapshot().home_seconds),
                    false,
                )
                .unwrap_or_else(|error| self.device_transport_error(error));
        }
        let audio_transport = lock_unpoisoned(&self.audio).stop();
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn audio_rewind_to_zero(&self) -> AppSnapshot {
        if self.device_playback.has_devices() {
            return self
                .device_audio_hold(crate::dto::DevicePlaybackMode::Paused, Some(0.0), true)
                .unwrap_or_else(|error| self.device_transport_error(error));
        }
        let audio_transport = lock_unpoisoned(&self.audio).rewind_to_zero();
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn audio_seek(&self, position_seconds: f32) -> AppSnapshot {
        if self.device_playback.has_devices() {
            let duration = self.audio_snapshot().duration_seconds;
            if !position_seconds.is_finite() {
                return self.device_transport_error("Seek position must be finite".into());
            }
            return self
                .device_audio_hold(
                    crate::dto::DevicePlaybackMode::Paused,
                    Some(position_seconds.clamp(0.0, duration)),
                    true,
                )
                .unwrap_or_else(|error| self.device_transport_error(error));
        }
        let audio_transport = lock_unpoisoned(&self.audio).seek(position_seconds);
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn audio_set_playback_rate(&self, rate: donder_runtime::PlaybackRate) -> AppSnapshot {
        if self.device_playback.has_devices() {
            return self
                .device_audio_set_rate(rate)
                .unwrap_or_else(|error| self.device_transport_error(error));
        }
        let audio_transport = lock_unpoisoned(&self.audio).set_playback_rate(rate);
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    #[cfg(test)]
    pub fn render_current_sequence_frame(
        &self,
    ) -> Result<crate::rendering::AudioClockRenderedFrame, crate::rendering::SequenceRenderError>
    {
        self.render_refresh.finish_pending();
        let _authoring = lock_unpoisoned(&self.authoring);
        let audio_transport = self.audio_snapshot();
        lock_unpoisoned(&self.sequence_render).render_current_sequence_frame(&audio_transport)
    }

    #[cfg(test)]
    pub fn active_preview_render_identity(
        &self,
    ) -> Result<crate::rendering::AudioClockRenderIdentity, crate::rendering::SequenceRenderError>
    {
        self.render_refresh.finish_pending();
        let _authoring = lock_unpoisoned(&self.authoring);
        let audio_transport = self.audio_snapshot();
        lock_unpoisoned(&self.sequence_render).active_render_identity(&audio_transport)
    }

    pub(crate) fn preview_content_identity(&self) -> crate::preview::PreviewContentIdentity {
        self.render_refresh.finish_pending();
        let _authoring = lock_unpoisoned(&self.authoring);
        let snapshot = self.snapshot();
        crate::preview::PreviewContentIdentity {
            project_epoch: snapshot.project_epoch,
            project_revision: self.project_revision(),
            sequence_generation: lock_unpoisoned(&self.sequence_render).session_generation(),
            project_loaded: self.project_session().is_some(),
        }
    }

    pub(crate) fn preview_content(&self) -> Result<crate::preview::PreviewContent, String> {
        self.render_refresh.finish_pending();
        let _authoring = lock_unpoisoned(&self.authoring);
        let Some(session) = self.project_session() else {
            return Ok(crate::preview::PreviewContent {
                instances: Vec::new(),
                sequence: None,
            });
        };
        let geometry = crate::preview::PreviewGeometry::from_project(&session.project)?;
        let sequence = lock_unpoisoned(&self.sequence_render)
            .encode_preview_sequence()
            .map_err(|error| format!("Cannot encode prepared Preview sequence: {error}"))?;
        if let Some(sequence) = sequence.as_ref()
            && sequence.fixtures != geometry.fixtures
        {
            return Err("Prepared Preview sequence does not match the active layout.".to_string());
        }

        Ok(crate::preview::PreviewContent {
            instances: geometry.instances,
            sequence: sequence.map(|sequence| sequence.bytes),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::{AudioTransportState, DocumentViewId, GuiDocument};
    use crate::project::{new_test_project_files, write_new_project_files};
    use camino::Utf8PathBuf;

    #[test]
    fn local_sequence_resolves_and_plays_without_copying() {
        let temporary = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
        write_new_project_files(&root, &new_test_project_files("Show").unwrap()).unwrap();
        let mut session = donder_project_io::load_project(&root).unwrap();
        for (site, kind, path) in [
            (
                donder_language::ownership::edit::OwnershipSite::ProjectSetup,
                donder_project_io::SourceObjectKind::Setup,
                "setups/main.data.donder",
            ),
            (
                donder_language::ownership::edit::OwnershipSite::ProjectSequence(0),
                donder_project_io::SourceObjectKind::Sequence,
                "sequences/main.data.donder",
            ),
        ] {
            let source = session
                .source
                .add_data_document(path.into(), vec![(kind, "main".into())])
                .unwrap()
                .remove(0);
            donder_language::ownership::edit::make_reusable(&mut session.project, &site, source)
                .unwrap();
        }
        donder_project_io::maintain_ownership_sources(&mut session).unwrap();
        donder_project_io::save_project(&session).unwrap();
        let state = DesktopState::new(|_| {});
        state.open_project_path(root.as_str());
        let loaded = state.project_session().unwrap();
        let imported = loaded.project.root().sequences[0].id().clone();
        assert!(loaded.source.is_project_owned(imported.0.document_id()));
        let request = state
            .resolve_gui_source(
                &imported.0.module_id().to_string(),
                imported.0.document().as_str(),
                imported.0.root_source().object(),
            )
            .unwrap();
        let opened = state.open_file_path(&request.path);
        assert!(!opened.active_buffer.as_ref().unwrap().read_only);
        assert_eq!(request.view, DocumentViewId::Sequence);
        let projected = state.get_gui_document(request.clone()).document;
        assert!(
            matches!(projected, GuiDocument::Sequence { .. }),
            "{projected:?}"
        );
        assert_eq!(state.resolve_sequence_id(&request), Some(imported.clone()));
        let transport = state.load_sequence_audio(request.clone()).audio_transport;
        assert_eq!(
            transport.duration_seconds,
            loaded.project.reusable_sequences()[&imported]
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
            Some((loaded.project.root().setup.id().clone(), imported.clone()))
        );
        assert_eq!(
            frame.frame.frame_rate,
            loaded.project.reusable_sequences()[&imported].frame_rate
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
