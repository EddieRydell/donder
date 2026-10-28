use super::{DesktopState, lock_unpoisoned};
use crate::persistence::{PersistedSequenceTransport, sequence_viewport_key};
use donder_editor::dto::{
    AppSnapshot, AudioTransportSnapshot, AudioTransportState, GuiDocumentRequest, PlaybackRange,
};

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
        let sequence_address = sequence_id.and_then(|id| {
            self.project_session()
                .and_then(|session| session.address_of(id.into()))
        });
        if let Err(error) = self
            .device_playback
            .stop_for_source(self.snapshot().project_epoch, sequence_address.as_ref())
        {
            return self.device_transport_error(error);
        }
        let project = self.project_session();
        let silent_duration = project
            .as_ref()
            .and_then(|project| {
                sequence_id
                    .as_ref()
                    .and_then(|id| project.project.get::<donder_model::Sequence>(*id))
            })
            .filter(|sequence| matches!(sequence.audio, donder_model::SequenceAudio::None))
            .map(|sequence| sequence.duration.as_seconds_f64());
        let audio_transport = match silent_duration {
            Some(duration) => lock_unpoisoned(&self.audio).load_silent_sequence(duration),
            None => lock_unpoisoned(&self.audio).load(audio),
        };
        let view_key = project
            .as_ref()
            .filter(|_| sequence_id.is_some())
            .and_then(|project| donder_editor::resolve_request(project, &request).ok())
            .and_then(|resolved| {
                let reference = resolved.source_ref();
                sequence_viewport_key(
                    &reference.path,
                    &reference.object_key,
                    &reference.owned_path,
                )
                .ok()
            });
        let audio_transport = self.restore_sequence_transport(view_key, audio_transport);
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
        lock_unpoisoned(&self.workspace).transport_view_key = None;
        if self.snapshot().project_health == donder_editor::dto::ProjectHealth::Ready {
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
        // A rig never plays on top of a sequence.
        if self.is_performing() {
            return self.update_snapshot(|snapshot| {
                snapshot.status = "Stop performing to play a sequence.".into();
            });
        }
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
                .device_audio_hold(donder_editor::dto::DevicePlaybackMode::Paused, None, false)
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
                    donder_editor::dto::DevicePlaybackMode::Stopped,
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
                .device_audio_hold(
                    donder_editor::dto::DevicePlaybackMode::Paused,
                    Some(0.0),
                    true,
                )
                .unwrap_or_else(|error| self.device_transport_error(error));
        }
        let audio_transport = lock_unpoisoned(&self.audio).rewind_to_zero();
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn audio_seek(&self, position_seconds: f64) -> AppSnapshot {
        if self.device_playback.has_devices() {
            let duration = self.audio_snapshot().duration_seconds;
            if !position_seconds.is_finite() {
                return self.device_transport_error("Seek position must be finite".into());
            }
            return self
                .device_audio_hold(
                    donder_editor::dto::DevicePlaybackMode::Paused,
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

    pub fn audio_set_range(&self, range: Option<PlaybackRange>) -> Result<AppSnapshot, String> {
        let audio_transport = lock_unpoisoned(&self.audio).set_range(range)?;
        self.record_sequence_transport(&audio_transport);
        // The engine moved a stopped playhead to the range start; held devices follow it.
        if let Some(range) = audio_transport.range
            && self.device_playback.has_devices()
            && !matches!(audio_transport.state, AudioTransportState::Playing)
        {
            return Ok(self.audio_seek(range.start_seconds));
        }
        Ok(self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        }))
    }

    pub fn audio_set_looping(&self, looping: bool) -> AppSnapshot {
        let audio_transport = lock_unpoisoned(&self.audio).set_looping(looping);
        self.record_sequence_transport(&audio_transport);
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    /// Observes the transport for the playback poll, rescheduling devices at the window end.
    pub(crate) fn poll_audio_transport(&self) -> AudioTransportSnapshot {
        if lock_unpoisoned(&self.audio).device_boundary().is_some()
            && let Err(error) = self.device_audio_cross_boundary()
        {
            self.device_transport_error(error);
        }
        self.audio_snapshot()
    }

    /// Each sequence keeps its own playback range and loop toggle.
    fn restore_sequence_transport(
        &self,
        view_key: Option<String>,
        audio_transport: AudioTransportSnapshot,
    ) -> AudioTransportSnapshot {
        let saved = match (&view_key, self.snapshot().project_root) {
            (Some(key), Some(root)) => self.persistence.sequence_transport(&root, key),
            _ => PersistedSequenceTransport::default(),
        };
        lock_unpoisoned(&self.workspace).transport_view_key = view_key;
        if matches!(
            audio_transport.state,
            AudioTransportState::Unloaded | AudioTransportState::Error
        ) {
            return audio_transport;
        }
        let mut audio = lock_unpoisoned(&self.audio);
        audio.set_looping(saved.looping);
        // A sequence that has since gotten shorter may no longer contain its saved range.
        match audio.set_range(saved.range) {
            Ok(audio_transport) => audio_transport,
            Err(_) => audio.set_range(None).unwrap_or(audio_transport),
        }
    }

    fn record_sequence_transport(&self, audio_transport: &AudioTransportSnapshot) {
        let Some(key) = lock_unpoisoned(&self.workspace).transport_view_key.clone() else {
            return;
        };
        let Some(project_root) = self.snapshot().project_root else {
            return;
        };
        let transport = PersistedSequenceTransport {
            range: audio_transport.range,
            looping: audio_transport.looping,
        };
        if let Err(error) =
            self.persistence
                .record_sequence_transport(&project_root, key, transport)
        {
            self.set_persistence_error(format!("Playback range was not saved: {error}"));
        }
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
            performance: self.performance_generation(),
        }
    }

    /// The clock the Preview plays on: the performance's while performing.
    pub(crate) fn preview_clock(&self) -> crate::preview::PreviewClock {
        self.performance_clock()
            .unwrap_or_else(|| self.audio_snapshot().into())
    }

    pub(crate) fn preview_content(&self) -> Result<crate::preview::PreviewContent, String> {
        self.render_refresh.finish_pending();
        let _authoring = lock_unpoisoned(&self.authoring);
        let Some(session) = self.project_session() else {
            return Ok(crate::preview::PreviewContent {
                instances: Vec::new(),
                archive: None,
                performance: None,
            });
        };
        let geometry = crate::preview::PreviewGeometry::from_project(&session.project)?;
        if let Some(performance) = self.preview_performance() {
            if performance.fixtures != geometry.fixtures {
                return Err("The performed rig does not match the active layout.".to_string());
            }
            return Ok(crate::preview::PreviewContent {
                instances: geometry.instances,
                archive: Some(performance.archive),
                performance: Some(performance.generation),
            });
        }
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
            archive: sequence.map(|sequence| sequence.bytes),
            performance: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{new_test_project_files, write_new_project_files};
    use camino::Utf8PathBuf;
    use donder_editor::dto::{AudioTransportState, GuiDocument};

    #[test]
    fn local_sequence_resolves_and_plays_without_copying() {
        let temporary = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
        write_new_project_files(&root, &new_test_project_files("Show").unwrap()).unwrap();
        let mut session =
            donder_project_io::check_project(&root, &Default::default(), &mut Default::default())
                .into_session()
                .unwrap();
        for (site, path) in [
            (
                donder_model::OwnershipSite::Member {
                    owner: session.project.root().id.into(),
                    member: (*session.project.root().setup.id()).into(),
                },
                "setups/main.data.donder",
            ),
            (
                donder_model::OwnershipSite::Member {
                    owner: session.project.root().id.into(),
                    member: (*session.project.root().sequences[0].id()).into(),
                },
                "sequences/main.data.donder",
            ),
        ] {
            let id = donder_model::make_reusable(&mut session.project, &site).unwrap();
            session
                .source
                .add_data_document(
                    path.into(),
                    vec![(id, donder_language::object_name("main"))],
                )
                .unwrap();
        }
        donder_project_io::maintain_ownership_sources(&mut session).unwrap();
        donder_project_io::save_project(&session).unwrap();
        let state = DesktopState::new(|_| {});
        state.open_project_path(root.as_str());
        let loaded = state.project_session().unwrap();
        let imported = *loaded.project.root().sequences[0].id();
        let address = loaded.address_of(imported.into()).unwrap();
        let request = state
            .resolve_gui_source(address.document().as_str(), address.root_source().object())
            .unwrap();
        state.open_file_path(&request.path);
        let projected = state.get_gui_document(request.clone()).document;
        assert!(
            matches!(projected, GuiDocument::Sequence { .. }),
            "{projected:?}"
        );
        assert_eq!(state.resolve_sequence_id(&request), Some(imported));
        let transport = state.load_sequence_audio(request.clone()).audio_transport;
        assert_eq!(
            transport.duration_seconds,
            loaded.project.reusable::<donder_model::Sequence>()[&imported]
                .duration
                .as_seconds_f64()
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
            Some((*loaded.project.root().setup.id(), imported))
        );
        assert_eq!(
            frame.frame.frame_rate,
            loaded.project.reusable::<donder_model::Sequence>()[&imported].frame_rate
        );
        assert!(state.active_preview_render_identity().unwrap().frame_count > 0);
        state.unload_audio();
        assert!(std::sync::Arc::ptr_eq(
            &loaded,
            &state.project_session().unwrap()
        ));
        let setup = loaded
            .address_of((*loaded.project.root().setup.id()).into())
            .unwrap();
        let mut wrong_view = request.clone();
        wrong_view.path = setup.document().to_string();
        wrong_view.object_key = setup.root_source().object().into();
        assert!(state.resolve_sequence_id(&wrong_view).is_none());
        let mut stale = request;
        stale.project_revision += 1;
        assert!(state.resolve_sequence_id(&stale).is_none());
    }
}
