use super::{DesktopState, lock_unpoisoned};
use crate::persistence::{PersistedSequenceTransport, sequence_viewport_key};
use donder_editor::dto::{
    AppSnapshot, AudioTransportSnapshot, AudioTransportState, GuiDocumentRequest, PlaybackRange,
};

impl DesktopState {
    pub(crate) fn load_sequence_audio(&self, request: GuiDocumentRequest) -> AppSnapshot {
        let _operation = lock_unpoisoned(&self.transport_operation);
        let _authoring = lock_unpoisoned(&self.authoring);
        if request.project_revision != self.snapshot().project_revision
            || self.project_session().is_none()
        {
            return self.snapshot();
        }
        self.record_playhead();
        let audio = self.resolve_sequence_audio(&request);
        let sequence_id = self.resolve_sequence_id(&request);
        let sequence_address = sequence_id.and_then(|id| {
            self.project_session()
                .and_then(|session| session.source_identity(id.into()))
        });
        // Donder controllers playing a performance keep playing it.
        if !self.is_performing()
            && let Err(error) = self
                .device_playback
                .stop_for_source(self.snapshot().project_epoch, sequence_address.as_ref())
        {
            return self.device_transport_error(error);
        }
        let project = self.project_session();
        let silent_duration = project.as_ref().and_then(|project| {
            let sequence = sequence_id
                .as_ref()
                .and_then(|id| project.project().get::<donder_model::Sequence>(*id))?;
            (matches!(sequence.audio, donder_model::SequenceAudio::None)
                || project
                    .audio_asset(&sequence.audio)
                    .is_some_and(|asset| !asset.is_available()))
            .then(|| sequence.duration.as_seconds_f64())
        });
        let audio_transport = match silent_duration {
            Some(duration) => lock_unpoisoned(&self.audio).load_silent_sequence(duration),
            None => lock_unpoisoned(&self.audio).load(audio),
        };
        let view_key = project
            .as_ref()
            .filter(|_| sequence_id.is_some())
            .and_then(|project| donder_editor::resolve_request(project, &request).ok())
            .map(|resolved| {
                let reference = resolved.source_ref();
                sequence_viewport_key(&reference.path, &reference.object_key)
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

    pub(crate) fn unload_audio(&self) -> AppSnapshot {
        let _operation = lock_unpoisoned(&self.transport_operation);
        let _authoring = lock_unpoisoned(&self.authoring);
        if !self.is_performing()
            && let Err(error) = self
                .device_playback
                .stop_for_source(self.snapshot().project_epoch, None)
        {
            return self.device_transport_error(error);
        }
        self.record_playhead();
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

    pub(crate) fn audio_play(&self) -> AppSnapshot {
        // A console never plays on top of a sequence.
        if self.is_performing() {
            return self.update_snapshot(|snapshot| {
                snapshot.status = "Stop performing to play a sequence.".into();
            });
        }
        let audio_transport = lock_unpoisoned(&self.audio).play();
        self.follow_on_devices();
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    /// Transport commands act on the editor at once; Donder controllers that
    /// live output drives follow a lead time later.
    fn follow_on_devices(&self) {
        if self.devices_live() {
            self.schedule_device_reconcile();
        }
    }

    pub(crate) fn audio_pause(&self) -> AppSnapshot {
        let audio_transport = lock_unpoisoned(&self.audio).pause();
        self.follow_on_devices();
        self.playhead_moved(audio_transport)
    }

    pub(crate) fn audio_stop(&self) -> AppSnapshot {
        let audio_transport = lock_unpoisoned(&self.audio).stop();
        self.follow_on_devices();
        self.playhead_moved(audio_transport)
    }

    pub(crate) fn audio_rewind_to_zero(&self) -> AppSnapshot {
        let audio_transport = lock_unpoisoned(&self.audio).rewind_to_zero();
        self.follow_on_devices();
        self.playhead_moved(audio_transport)
    }

    pub(crate) fn audio_seek(&self, position_seconds: f64) -> AppSnapshot {
        let audio_transport = lock_unpoisoned(&self.audio).seek(position_seconds);
        self.follow_on_devices();
        self.playhead_moved(audio_transport)
    }

    /// The playhead came to rest at a new place, which its sequence keeps.
    fn playhead_moved(&self, audio_transport: AudioTransportSnapshot) -> AppSnapshot {
        self.record_sequence_transport(&audio_transport);
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    /// Keep where the loaded sequence's playhead is, for when it opens again.
    pub(crate) fn record_playhead(&self) {
        self.record_sequence_transport(&self.audio_snapshot());
    }

    pub(crate) fn audio_set_playback_rate(
        &self,
        rate: donder_runtime::PlaybackRate,
    ) -> AppSnapshot {
        let audio_transport = lock_unpoisoned(&self.audio).set_playback_rate(rate);
        self.follow_on_devices();
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub(crate) fn audio_set_range(
        &self,
        range: Option<PlaybackRange>,
    ) -> Result<AppSnapshot, String> {
        let audio_transport = lock_unpoisoned(&self.audio).set_range(range)?;
        self.record_sequence_transport(&audio_transport);
        self.follow_on_devices();
        Ok(self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        }))
    }

    pub(crate) fn audio_set_looping(&self, looping: bool) -> AppSnapshot {
        let audio_transport = lock_unpoisoned(&self.audio).set_looping(looping);
        self.record_sequence_transport(&audio_transport);
        self.follow_on_devices();
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    /// Observes the transport for the playback poll. Past the window end the
    /// controllers were scheduled for, they are scheduled for the next one.
    pub(crate) fn poll_audio_transport(&self) -> AudioTransportSnapshot {
        if lock_unpoisoned(&self.device_window_end)
            .take_if(|end| std::time::Instant::now() >= *end)
            .is_some()
        {
            self.schedule_device_reconcile();
        }
        self.audio_snapshot()
    }

    /// Each sequence keeps its own playhead, playback range and loop toggle.
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
        let audio_transport = match audio.set_range(saved.range) {
            Ok(audio_transport) => audio_transport,
            Err(_) => audio.set_range(None).unwrap_or(audio_transport),
        };
        if saved.position_seconds == audio_transport.position_seconds {
            return audio_transport;
        }
        audio.seek(saved.position_seconds)
    }

    fn record_sequence_transport(&self, audio_transport: &AudioTransportSnapshot) {
        let Some(key) = lock_unpoisoned(&self.workspace).transport_view_key.clone() else {
            return;
        };
        let Some(project_root) = self.snapshot().project_root else {
            return;
        };
        if matches!(
            audio_transport.state,
            AudioTransportState::Unloaded | AudioTransportState::Error
        ) {
            return;
        }
        let transport = PersistedSequenceTransport {
            position_seconds: audio_transport.position_seconds,
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

    pub(crate) fn preview_content_identity(&self) -> crate::preview::PreviewContentIdentity {
        self.render_refresh.finish_pending();
        let _authoring = lock_unpoisoned(&self.authoring);
        let snapshot = self.snapshot();
        crate::preview::PreviewContentIdentity {
            project_epoch: snapshot.project_epoch,
            project_revision: self.project_revision(),
            project_loaded: self.project_session().is_some(),
            playing: self.with_playing(|playing| playing.map(|playing| playing.generation)),
        }
    }

    /// The clock the Preview plays on: the performance's while performing.
    pub(crate) fn preview_clock(&self) -> crate::preview::PreviewClock {
        let audio = self.audio_snapshot();
        self.with_playing(|playing| match playing {
            Some(playing) => playing.preview_clock(audio),
            None => audio.into(),
        })
    }

    pub(crate) fn preview_content(&self) -> Result<crate::preview::PreviewContent, String> {
        self.render_refresh.finish_pending();
        let _authoring = lock_unpoisoned(&self.authoring);
        let Some(session) = self.project_session() else {
            return Ok(crate::preview::PreviewContent {
                instances: Vec::new(),
                show: None,
                performance: None,
            });
        };
        let geometry = crate::preview::PreviewGeometry::from_project(session.project());
        self.with_playing(|playing| {
            if let Some(playing) = playing
                && playing.fixtures() != geometry.fixtures
            {
                return Err("The prepared show does not match the active layout.".to_string());
            }
            Ok(crate::preview::PreviewContent {
                instances: geometry.instances,
                show: playing.map(|playing| playing.show.encode()),
                performance: playing
                    .filter(|playing| {
                        matches!(playing.clock, crate::playing::PlayClock::Session(_))
                    })
                    .map(|playing| playing.generation),
            })
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
        let session =
            donder_project_io::check_project(&root, &Default::default(), &mut Default::default())
                .into_session()
                .unwrap();
        donder_project_io::save_project(&session).unwrap();
        let state = DesktopState::new(|_| {});
        state.open_project_path(root.as_str());
        let loaded = state.project_session().unwrap();
        let imported = *loaded
            .project()
            .reusable::<donder_model::Sequence>()
            .get_index(0)
            .unwrap()
            .0;
        let address = loaded.address_of(imported);
        let request = state
            .resolve_gui_source(address.document().as_str(), address.object())
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
            loaded.project().reusable::<donder_model::Sequence>()[&imported]
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
        state.render_refresh.finish_pending();
        let (id, frame_rate, frame_count) = state
            .with_playing(|playing| {
                playing.map(|playing| {
                    (
                        playing.id,
                        playing.show.frame_rate(),
                        playing.show.frame_count(),
                    )
                })
            })
            .unwrap();
        assert_eq!(id, donder_model::ShowId::Sequence(imported));
        assert_eq!(
            frame_rate,
            loaded.project().reusable::<donder_model::Sequence>()[&imported].frame_rate
        );
        assert!(frame_count > 0);
        state.unload_audio();
        assert!(std::sync::Arc::ptr_eq(
            &loaded,
            &state.project_session().unwrap()
        ));
        let setup = loaded.address_of(loaded.project().root().id);
        let mut wrong_view = request.clone();
        wrong_view.path = setup.document().to_string();
        wrong_view.object_key = setup.object().into();
        assert!(state.resolve_sequence_id(&wrong_view).is_none());
        let mut stale = request;
        stale.project_revision += 1;
        assert!(state.resolve_sequence_id(&stale).is_none());
    }
}
