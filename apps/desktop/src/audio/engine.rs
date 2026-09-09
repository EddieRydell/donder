use crate::dto::{AudioTransportSnapshot, AudioTransportState, SequenceAudio};
use std::time::Instant;

use super::backend::{
    AudioDriver, AudioHandle, BackendPlaybackState, KiraAudioDriver, LoadedSource,
    canonical_audio_path,
};

enum TransportSource {
    Audio(LoadedSource),
    Silent {
        duration_seconds: f32,
        anchor: Option<(Instant, f32)>,
    },
}

pub(crate) struct AudioEngine {
    driver: Option<Box<dyn AudioDriver>>,
    handle: Option<Box<dyn AudioHandle>>,
    source: Option<TransportSource>,
    home_seconds: f32,
    position_seconds: f32,
    state: AudioTransportState,
    generation: u32,
    last_error: Option<String>,
    can_resume_handle: bool,
}

impl AudioEngine {
    pub(crate) fn new() -> Self {
        match KiraAudioDriver::new() {
            Ok(driver) => Self::with_optional_driver(Some(Box::new(driver)), None),
            Err(error) => Self::with_optional_driver(None, Some(error)),
        }
    }

    fn with_optional_driver(driver: Option<Box<dyn AudioDriver>>, error: Option<String>) -> Self {
        Self {
            driver,
            handle: None,
            source: None,
            home_seconds: 0.0,
            position_seconds: 0.0,
            state: if error.is_some() {
                AudioTransportState::Error
            } else {
                AudioTransportState::Unloaded
            },
            generation: 0,
            last_error: error,
            can_resume_handle: false,
        }
    }

    pub fn empty_snapshot() -> AudioTransportSnapshot {
        AudioTransportSnapshot {
            state: AudioTransportState::Unloaded,
            source: None,
            generation: 0,
            position_seconds: 0.0,
            home_seconds: 0.0,
            duration_seconds: 0.0,
            last_error: None,
        }
    }

    pub fn snapshot(&mut self) -> AudioTransportSnapshot {
        self.observe_backend();
        self.current_snapshot()
    }

    pub fn load_silent_sequence(&mut self, duration_seconds: f32) -> AudioTransportSnapshot {
        self.observe_backend();
        if let Some(TransportSource::Silent {
            duration_seconds: current,
            ..
        }) = &self.source
            && *current == duration_seconds
        {
            return self.current_snapshot();
        }
        self.reset_loaded_source();
        if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
            self.state = AudioTransportState::Error;
            self.last_error = Some("Sequence duration must be positive and finite.".into());
        } else {
            self.source = Some(TransportSource::Silent {
                duration_seconds,
                anchor: None,
            });
            self.state = AudioTransportState::Stopped;
            self.last_error = None;
        }
        self.bump_generation();
        self.current_snapshot()
    }

    pub fn load(&mut self, audio: Option<SequenceAudio>) -> AudioTransportSnapshot {
        self.observe_backend();
        let Some(audio) = audio else {
            return self.unload();
        };
        if !audio.exists {
            self.reset_loaded_source();
            self.state = AudioTransportState::Error;
            self.last_error = Some("Audio source does not exist.".to_string());
            self.bump_generation();
            return self.current_snapshot();
        }
        let canonical_path = match canonical_audio_path(&audio.resolved_path) {
            Ok(canonical_path) => canonical_path,
            Err(error) => {
                self.reset_loaded_source();
                self.state = AudioTransportState::Error;
                self.last_error = Some(error.to_string());
                self.bump_generation();
                return self.current_snapshot();
            }
        };
        if self
            .source
            .as_ref()
            .is_some_and(|source| matches!(source, TransportSource::Audio(source) if source.canonical_path == canonical_path))
        {
            return self.current_snapshot();
        }
        let Some(driver) = self.driver.as_mut() else {
            self.reset_loaded_source();
            self.state = AudioTransportState::Error;
            self.last_error = Some("Audio manager is not available.".to_string());
            self.bump_generation();
            return self.current_snapshot();
        };
        match driver.load_metadata(&audio.resolved_path) {
            Ok(metadata) => {
                self.reset_loaded_source();
                self.source = Some(TransportSource::Audio(LoadedSource {
                    audio,
                    canonical_path,
                    duration_seconds: metadata.duration_seconds,
                }));
                self.home_seconds = 0.0;
                self.position_seconds = 0.0;
                self.state = AudioTransportState::Stopped;
                self.last_error = None;
                self.can_resume_handle = false;
                self.bump_generation();
            }
            Err(error) => {
                self.reset_loaded_source();
                self.state = AudioTransportState::Error;
                self.last_error = Some(error);
                self.can_resume_handle = false;
                self.bump_generation();
            }
        }
        self.current_snapshot()
    }

    pub fn unload(&mut self) -> AudioTransportSnapshot {
        self.observe_backend();
        if self.source.is_none()
            && self.handle.is_none()
            && matches!(self.state, AudioTransportState::Unloaded)
            && self.last_error.is_none()
        {
            return self.current_snapshot();
        }
        self.reset_loaded_source();
        self.state = AudioTransportState::Unloaded;
        self.last_error = None;
        self.can_resume_handle = false;
        self.bump_generation();
        self.current_snapshot()
    }

    pub fn play(&mut self) -> AudioTransportSnapshot {
        self.observe_backend();
        if self.source.is_none() {
            return self.current_snapshot();
        }
        if matches!(self.state, AudioTransportState::Playing) {
            return self.current_snapshot();
        }
        if matches!(self.state, AudioTransportState::Ended) {
            self.position_seconds = self.home_seconds;
        }
        if let Some(TransportSource::Silent { anchor, .. }) = &mut self.source {
            *anchor = Some((Instant::now(), self.position_seconds));
            self.state = AudioTransportState::Playing;
            self.last_error = None;
            self.bump_generation();
            return self.current_snapshot();
        }
        if matches!(self.state, AudioTransportState::Paused)
            && self.can_resume_handle
            && let Some(handle) = self.handle.as_mut()
        {
            handle.resume();
            self.state = AudioTransportState::Playing;
            self.last_error = None;
            self.can_resume_handle = false;
            self.bump_generation();
            return self.current_snapshot();
        }
        self.lifecycle_stop_handle();
        self.start_stream_at_position();
        self.current_snapshot()
    }

    pub fn pause(&mut self) -> AudioTransportSnapshot {
        self.observe_backend();
        if !matches!(self.state, AudioTransportState::Playing) {
            return self.current_snapshot();
        }
        self.sample_handle_position();
        if let Some(handle) = self.handle.as_mut() {
            handle.pause();
        }
        self.state = AudioTransportState::Paused;
        self.can_resume_handle = true;
        self.bump_generation();
        self.current_snapshot()
    }

    pub fn stop(&mut self) -> AudioTransportSnapshot {
        self.observe_backend();
        if self.source.is_none() {
            return self.current_snapshot();
        }
        self.position_seconds = self.home_seconds;
        if let Some(mut handle) = self.handle.take() {
            handle.pause();
            handle.seek_to(self.position_seconds);
        }
        self.state = AudioTransportState::Stopped;
        self.can_resume_handle = false;
        self.bump_generation();
        self.current_snapshot()
    }

    pub fn rewind_to_zero(&mut self) -> AudioTransportSnapshot {
        self.seek_to(0.0, AudioTransportState::Paused)
    }

    pub fn seek(&mut self, position_seconds: f32) -> AudioTransportSnapshot {
        let position_seconds = self.clamp_position(position_seconds);
        self.seek_to(position_seconds, AudioTransportState::Paused)
    }

    fn seek_to(
        &mut self,
        position_seconds: f32,
        final_state: AudioTransportState,
    ) -> AudioTransportSnapshot {
        self.observe_backend();
        if self.source.is_none() {
            return self.current_snapshot();
        }
        let position_seconds = self.clamp_position(position_seconds);
        self.home_seconds = position_seconds;
        self.position_seconds = position_seconds;
        if let Some(handle) = self.handle.as_mut() {
            handle.pause();
            handle.seek_to(position_seconds);
        }
        self.state = final_state;
        self.can_resume_handle = false;
        self.bump_generation();
        self.current_snapshot()
    }

    fn start_stream_at_position(&mut self) {
        let Some(driver) = self.driver.as_mut() else {
            self.handle = None;
            self.state = AudioTransportState::Error;
            self.last_error = Some("Audio manager is not available.".to_string());
            self.bump_generation();
            return;
        };
        let Some(TransportSource::Audio(source)) = self.source.as_ref() else {
            return;
        };
        match driver.play(&source.audio.resolved_path, self.position_seconds) {
            Ok(handle) => {
                self.handle = Some(handle);
                self.state = AudioTransportState::Playing;
                self.last_error = None;
                self.can_resume_handle = false;
                self.bump_generation();
            }
            Err(error) => {
                self.handle = None;
                self.state = AudioTransportState::Error;
                self.last_error = Some(error);
                self.can_resume_handle = false;
                self.bump_generation();
            }
        }
    }

    fn observe_backend(&mut self) {
        if let Some(TransportSource::Silent {
            duration_seconds,
            anchor,
        }) = &self.source
        {
            if matches!(self.state, AudioTransportState::Playing)
                && let Some((started, position)) = anchor
            {
                self.position_seconds =
                    (position + started.elapsed().as_secs_f32()).min(*duration_seconds);
                if self.position_seconds >= *duration_seconds {
                    self.state = AudioTransportState::Ended;
                    self.bump_generation();
                }
            }
            return;
        }
        let Some(handle) = self.handle.as_mut() else {
            return;
        };
        let observation = handle.observe();
        if let Some(error) = observation.error {
            self.state = AudioTransportState::Error;
            self.last_error = Some(error);
            self.can_resume_handle = false;
            self.bump_generation();
            return;
        }
        if !matches!(self.state, AudioTransportState::Playing) {
            return;
        }
        match observation.state {
            BackendPlaybackState::Advancing => {
                self.position_seconds = self.clamp_position(observation.position_seconds);
            }
            BackendPlaybackState::Paused => {}
            BackendPlaybackState::Stopped => {
                self.position_seconds = self.duration_seconds();
                self.handle = None;
                self.state = AudioTransportState::Ended;
                self.can_resume_handle = false;
                self.bump_generation();
            }
        }
    }

    fn sample_handle_position(&mut self) {
        if let Some(handle) = self.handle.as_mut() {
            let position_seconds = handle.observe().position_seconds;
            self.position_seconds = self.clamp_position(position_seconds);
        }
    }

    fn reset_loaded_source(&mut self) {
        self.lifecycle_stop_handle();
        self.source = None;
        self.home_seconds = 0.0;
        self.position_seconds = 0.0;
        self.can_resume_handle = false;
    }

    fn lifecycle_stop_handle(&mut self) {
        if let Some(mut handle) = self.handle.take() {
            handle.stop();
        }
    }

    fn current_snapshot(&self) -> AudioTransportSnapshot {
        AudioTransportSnapshot {
            state: self.state.clone(),
            source: match &self.source {
                Some(TransportSource::Audio(source)) => Some(source.audio.clone()),
                Some(TransportSource::Silent { .. }) | None => None,
            },
            generation: self.generation,
            position_seconds: self.position_seconds,
            home_seconds: self.home_seconds,
            duration_seconds: self.duration_seconds(),
            last_error: self.last_error.clone(),
        }
    }

    fn clamp_position(&self, position_seconds: f32) -> f32 {
        if !position_seconds.is_finite() {
            return 0.0;
        }
        position_seconds.clamp(0.0, self.duration_seconds())
    }

    fn duration_seconds(&self) -> f32 {
        match &self.source {
            Some(TransportSource::Audio(source)) => source.duration_seconds,
            Some(TransportSource::Silent {
                duration_seconds, ..
            }) => *duration_seconds,
            None => 0.0,
        }
    }

    fn bump_generation(&mut self) {
        self.generation = self.generation.saturating_add(1);
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.lifecycle_stop_handle();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::super::backend::{
        AudioDriver, AudioHandle, BackendObservation, BackendPlaybackState, SourceMetadata,
        instant_tween,
    };
    use super::*;

    #[test]
    fn transport_commands_use_instant_tween() {
        assert_eq!(instant_tween().duration, Duration::ZERO);
    }

    #[test]
    fn load_valid_source_initializes_stopped_without_handle() {
        let fixture = AudioFixture::new(12.0);
        let mut engine = fixture.engine();

        let snapshot = engine.load(Some(test_audio()));

        assert_eq!(snapshot.state, AudioTransportState::Stopped);
        assert_eq!(snapshot.home_seconds, 0.0);
        assert_eq!(snapshot.position_seconds, 0.0);
        assert_eq!(snapshot.duration_seconds, 12.0);
        assert_eq!(snapshot.generation, 1);
        assert!(snapshot.source.is_some());
        fixture.assert_actions(&[DriverAction::LoadMetadata]);
    }

    #[test]
    fn play_from_stopped_creates_handle_and_reports_playing() {
        let fixture = AudioFixture::new(12.0);
        let mut engine = loaded_engine(&fixture);

        let snapshot = engine.play();

        assert_eq!(snapshot.state, AudioTransportState::Playing);
        assert_eq!(snapshot.position_seconds, 0.0);
        assert_eq!(snapshot.generation, 2);
        fixture.assert_actions(&[DriverAction::LoadMetadata, DriverAction::Play(0.0)]);
    }

    #[test]
    fn pause_while_playing_samples_position_and_preserves_home() {
        let fixture = AudioFixture::new(12.0);
        let mut engine = playing_engine(&fixture);
        fixture.set_handle_position(3.25);

        let snapshot = engine.pause();

        assert_eq!(snapshot.state, AudioTransportState::Paused);
        assert_eq!(snapshot.position_seconds, 3.25);
        assert_eq!(snapshot.home_seconds, 0.0);
        assert_eq!(snapshot.generation, 3);
        fixture.assert_handle_actions(&[HandleAction::Pause]);
    }

    #[test]
    fn play_after_pause_resumes_existing_handle_without_seek() {
        let fixture = AudioFixture::new(12.0);
        let mut engine = playing_engine(&fixture);
        fixture.set_handle_position(3.25);
        engine.pause();
        fixture.clear_driver_actions();
        fixture.clear_handle_actions();

        let snapshot = engine.play();

        assert_eq!(snapshot.state, AudioTransportState::Playing);
        assert_eq!(snapshot.position_seconds, 3.25);
        fixture.assert_actions(&[]);
        fixture.assert_handle_actions(&[HandleAction::Resume]);
    }

    #[test]
    fn stop_while_playing_pauses_and_seeks_home_without_backend_stop() {
        let fixture = AudioFixture::new(12.0);
        let mut engine = playing_engine(&fixture);
        engine.seek(4.0);
        engine.play();
        fixture.clear_handle_actions();

        let snapshot = engine.stop();

        assert_eq!(snapshot.state, AudioTransportState::Stopped);
        assert_eq!(snapshot.position_seconds, 4.0);
        assert_eq!(snapshot.home_seconds, 4.0);
        fixture.assert_handle_actions(&[HandleAction::Pause, HandleAction::Seek(4.0)]);
        fixture.assert_no_handle_stop();
    }

    #[test]
    fn play_after_stop_recreates_stream_at_home_instead_of_seek_then_resume() {
        let fixture = AudioFixture::new(12.0);
        let mut engine = playing_engine(&fixture);
        engine.seek(4.0);
        engine.play();
        engine.stop();
        fixture.clear_driver_actions();
        fixture.clear_handle_actions();

        let snapshot = engine.play();

        assert_eq!(snapshot.state, AudioTransportState::Playing);
        assert_eq!(snapshot.position_seconds, 4.0);
        fixture.assert_actions(&[DriverAction::Play(4.0)]);
        fixture.assert_handle_actions(&[]);
    }

    #[test]
    fn seek_while_playing_pauses_sets_home_and_seeks_handle() {
        let fixture = AudioFixture::new(12.0);
        let mut engine = playing_engine(&fixture);
        fixture.clear_handle_actions();

        let snapshot = engine.seek(5.5);

        assert_eq!(snapshot.state, AudioTransportState::Paused);
        assert_eq!(snapshot.position_seconds, 5.5);
        assert_eq!(snapshot.home_seconds, 5.5);
        fixture.assert_handle_actions(&[HandleAction::Pause, HandleAction::Seek(5.5)]);
    }

    #[test]
    fn play_after_seek_recreates_stream_at_seek_position_instead_of_resuming() {
        let fixture = AudioFixture::new(12.0);
        let mut engine = playing_engine(&fixture);
        engine.seek(5.5);
        fixture.clear_driver_actions();
        fixture.clear_handle_actions();

        let snapshot = engine.play();

        assert_eq!(snapshot.state, AudioTransportState::Playing);
        assert_eq!(snapshot.position_seconds, 5.5);
        fixture.assert_actions(&[DriverAction::Play(5.5)]);
        fixture.assert_handle_actions(&[HandleAction::Stop]);
    }

    #[test]
    fn rewind_while_playing_pauses_at_zero_and_sets_home_to_zero() {
        let fixture = AudioFixture::new(12.0);
        let mut engine = playing_engine(&fixture);
        fixture.clear_handle_actions();

        let snapshot = engine.rewind_to_zero();

        assert_eq!(snapshot.state, AudioTransportState::Paused);
        assert_eq!(snapshot.position_seconds, 0.0);
        assert_eq!(snapshot.home_seconds, 0.0);
        fixture.assert_handle_actions(&[HandleAction::Pause, HandleAction::Seek(0.0)]);
    }

    #[test]
    fn natural_end_while_playing_reports_ended_at_duration() {
        let fixture = AudioFixture::new(12.0);
        let mut engine = playing_engine(&fixture);
        fixture.set_handle_state(BackendPlaybackState::Stopped);

        let snapshot = engine.snapshot();

        assert_eq!(snapshot.state, AudioTransportState::Ended);
        assert_eq!(snapshot.position_seconds, 12.0);
        assert_eq!(snapshot.generation, 3);
    }

    #[test]
    fn play_from_ended_restarts_from_home() {
        let fixture = AudioFixture::new(12.0);
        let mut engine = playing_engine(&fixture);
        engine.seek(4.0);
        engine.play();
        fixture.set_handle_state(BackendPlaybackState::Stopped);
        let ended = engine.snapshot();
        assert_eq!(ended.state, AudioTransportState::Ended);
        fixture.clear_driver_actions();

        let snapshot = engine.play();

        assert_eq!(snapshot.state, AudioTransportState::Playing);
        assert_eq!(snapshot.position_seconds, 4.0);
        fixture.assert_actions(&[DriverAction::Play(4.0)]);
    }

    #[test]
    fn unload_and_source_replacement_call_lifecycle_stop() {
        let fixture = AudioFixture::new(12.0);
        let mut engine = playing_engine(&fixture);
        fixture.clear_handle_actions();

        let unloaded = engine.unload();

        assert_eq!(unloaded.state, AudioTransportState::Unloaded);
        fixture.assert_handle_actions(&[HandleAction::Stop]);

        let mut engine = playing_engine(&fixture);
        fixture.clear_handle_actions();
        let mut replacement = test_audio();
        replacement.resolved_path = std::env::current_dir()
            .expect("test working directory")
            .to_string_lossy()
            .into_owned();
        let snapshot = engine.load(Some(replacement));

        assert_eq!(snapshot.state, AudioTransportState::Stopped);
        fixture.assert_handle_actions(&[HandleAction::Stop]);
    }

    fn loaded_engine(fixture: &AudioFixture) -> AudioEngine {
        let mut engine = fixture.engine();
        engine.load(Some(test_audio()));
        engine
    }

    fn playing_engine(fixture: &AudioFixture) -> AudioEngine {
        let mut engine = loaded_engine(fixture);
        engine.play();
        engine
    }

    fn test_audio() -> SequenceAudio {
        SequenceAudio {
            import_path: "audio.wav".to_string(),
            resolved_path: std::env::current_exe()
                .expect("test executable path")
                .to_string_lossy()
                .into_owned(),
            file_name: "audio.wav".to_string(),
            exists: true,
        }
    }

    struct AudioFixture {
        shared: Arc<Mutex<FakeShared>>,
    }

    impl AudioFixture {
        fn new(duration_seconds: f32) -> Self {
            Self {
                shared: Arc::new(Mutex::new(FakeShared {
                    duration_seconds,
                    driver_actions: Vec::new(),
                    handle_actions: Vec::new(),
                    handle_state: BackendPlaybackState::Advancing,
                    handle_position: 0.0,
                })),
            }
        }

        fn engine(&self) -> AudioEngine {
            AudioEngine::with_optional_driver(
                Some(Box::new(FakeAudioDriver {
                    shared: Arc::clone(&self.shared),
                })),
                None,
            )
        }

        fn set_handle_state(&self, state: BackendPlaybackState) {
            self.shared.lock().expect("fake shared").handle_state = state;
        }

        fn set_handle_position(&self, position_seconds: f32) {
            self.shared.lock().expect("fake shared").handle_position = position_seconds;
        }

        fn clear_driver_actions(&self) {
            self.shared
                .lock()
                .expect("fake shared")
                .driver_actions
                .clear();
        }

        fn clear_handle_actions(&self) {
            self.shared
                .lock()
                .expect("fake shared")
                .handle_actions
                .clear();
        }

        fn assert_actions(&self, expected: &[DriverAction]) {
            assert_eq!(
                self.shared.lock().expect("fake shared").driver_actions,
                expected
            );
        }

        fn assert_handle_actions(&self, expected: &[HandleAction]) {
            assert_eq!(
                self.shared.lock().expect("fake shared").handle_actions,
                expected
            );
        }

        fn assert_no_handle_stop(&self) {
            assert!(
                !self
                    .shared
                    .lock()
                    .expect("fake shared")
                    .handle_actions
                    .contains(&HandleAction::Stop)
            );
        }
    }

    struct FakeShared {
        duration_seconds: f32,
        driver_actions: Vec<DriverAction>,
        handle_actions: Vec<HandleAction>,
        handle_state: BackendPlaybackState,
        handle_position: f32,
    }

    #[derive(Debug, PartialEq)]
    enum DriverAction {
        LoadMetadata,
        Play(f32),
    }

    #[derive(Debug, PartialEq)]
    enum HandleAction {
        Pause,
        Resume,
        Seek(f32),
        Stop,
    }

    struct FakeAudioDriver {
        shared: Arc<Mutex<FakeShared>>,
    }

    impl AudioDriver for FakeAudioDriver {
        fn load_metadata(&mut self, _path: &str) -> Result<SourceMetadata, String> {
            let mut shared = self.shared.lock().expect("fake shared");
            shared.driver_actions.push(DriverAction::LoadMetadata);
            Ok(SourceMetadata {
                duration_seconds: shared.duration_seconds,
            })
        }

        fn play(
            &mut self,
            _path: &str,
            position_seconds: f32,
        ) -> Result<Box<dyn AudioHandle>, String> {
            let mut shared = self.shared.lock().expect("fake shared");
            shared
                .driver_actions
                .push(DriverAction::Play(position_seconds));
            shared.handle_position = position_seconds;
            shared.handle_state = BackendPlaybackState::Advancing;
            Ok(Box::new(FakeAudioHandle {
                shared: Arc::clone(&self.shared),
            }))
        }
    }

    struct FakeAudioHandle {
        shared: Arc<Mutex<FakeShared>>,
    }

    impl AudioHandle for FakeAudioHandle {
        fn observe(&mut self) -> BackendObservation {
            let shared = self.shared.lock().expect("fake shared");
            BackendObservation {
                state: shared.handle_state,
                position_seconds: shared.handle_position,
                error: None,
            }
        }

        fn pause(&mut self) {
            let mut shared = self.shared.lock().expect("fake shared");
            shared.handle_actions.push(HandleAction::Pause);
            shared.handle_state = BackendPlaybackState::Paused;
        }

        fn resume(&mut self) {
            let mut shared = self.shared.lock().expect("fake shared");
            shared.handle_actions.push(HandleAction::Resume);
            shared.handle_state = BackendPlaybackState::Advancing;
        }

        fn seek_to(&mut self, position_seconds: f32) {
            let mut shared = self.shared.lock().expect("fake shared");
            shared
                .handle_actions
                .push(HandleAction::Seek(position_seconds));
            shared.handle_position = position_seconds;
        }

        fn stop(&mut self) {
            let mut shared = self.shared.lock().expect("fake shared");
            shared.handle_actions.push(HandleAction::Stop);
            shared.handle_state = BackendPlaybackState::Stopped;
        }
    }
}
