use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use kira::sound::streaming::{StreamingSoundData, StreamingSoundHandle};
use kira::sound::{FromFileError, PlaybackState};
use kira::{AudioManager, AudioManagerSettings, DefaultBackend, Tween};

use crate::dto::SequenceAudio;

type KiraManager = AudioManager<DefaultBackend>;
type KiraStreamingHandle = StreamingSoundHandle<FromFileError>;

pub(super) struct LoadedSource {
    pub(super) audio: SequenceAudio,
    pub(super) canonical_path: String,
    pub(super) duration_seconds: f32,
}

pub(super) struct SourceMetadata {
    pub(super) duration_seconds: f32,
}

pub(super) trait AudioDriver: Send {
    fn load_metadata(&mut self, path: &str) -> Result<SourceMetadata, String>;
    fn play(&mut self, path: &str, position_seconds: f32) -> Result<Box<dyn AudioHandle>, String>;
    fn debug_observe(&mut self) {}
}

pub(super) fn audio_debug(message: std::fmt::Arguments<'_>) {
    if cfg!(debug_assertions) {
        eprintln!(
            "[audio {:?}] {message}",
            SystemTime::now().duration_since(UNIX_EPOCH)
        );
    }
}

pub(super) trait AudioHandle: Send {
    fn observe(&mut self) -> BackendObservation;
    fn pause(&mut self);
    fn resume(&mut self);
    fn seek_to(&mut self, position_seconds: f32);
    fn stop(&mut self);
}

pub(super) struct BackendObservation {
    pub(super) state: BackendPlaybackState,
    pub(super) position_seconds: f32,
    pub(super) error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BackendPlaybackState {
    Advancing,
    Paused,
    Stopped,
}

pub(super) struct KiraAudioDriver {
    manager: KiraManager,
    last_debug_report: Instant,
}

impl KiraAudioDriver {
    pub(super) fn new() -> Result<Self, String> {
        audio_debug(format_args!("creating audio manager"));
        AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())
            .inspect(|_| audio_debug(format_args!("audio manager created")))
            .inspect_err(|error| audio_debug(format_args!("audio manager failed: {error:?}")))
            .map(|manager| Self {
                manager,
                last_debug_report: Instant::now(),
            })
            .map_err(|error| error.to_string())
    }
}

impl AudioDriver for KiraAudioDriver {
    fn debug_observe(&mut self) {
        if !cfg!(debug_assertions) {
            return;
        }
        let backend = self.manager.backend_mut();
        while let Some(error) = backend.pop_error() {
            audio_debug(format_args!("output stream error: {error:?}"));
        }
        if self.last_debug_report.elapsed() >= Duration::from_secs(1) {
            let elapsed = self.last_debug_report.elapsed();
            self.last_debug_report = Instant::now();
            let mut reports = 0;
            while backend.pop_cpu_usage().is_some() {
                reports += 1;
            }
            audio_debug(format_args!(
                "output processing reports={reports} since_last_report={elapsed:?} discarded_stream_errors={:?}",
                backend.num_stream_errors_discarded()
            ));
        }
    }

    fn load_metadata(&mut self, path: &str) -> Result<SourceMetadata, String> {
        audio_debug(format_args!("load metadata path={path:?}"));
        StreamingSoundData::from_file(path)
            .map(|sound| SourceMetadata {
                duration_seconds: sound.duration().as_secs_f32(),
            })
            .map_err(|error| error.to_string())
    }

    fn play(&mut self, path: &str, position_seconds: f32) -> Result<Box<dyn AudioHandle>, String> {
        audio_debug(format_args!(
            "create sound path={path:?} position={position_seconds}"
        ));
        let sound = StreamingSoundData::from_file(path)
            .inspect_err(|error| audio_debug(format_args!("open sound failed: {error:?}")))
            .map_err(|error| error.to_string())?
            .start_position(f64::from(position_seconds));
        self.manager
            .play(sound)
            .inspect(|_| audio_debug(format_args!("sound handle created")))
            .inspect_err(|error| audio_debug(format_args!("play sound failed: {error:?}")))
            .map(|handle| Box::new(KiraAudioHandle { handle }) as Box<dyn AudioHandle>)
            .map_err(|error| error.to_string())
    }
}

pub(super) struct KiraAudioHandle {
    handle: KiraStreamingHandle,
}

impl AudioHandle for KiraAudioHandle {
    fn observe(&mut self) -> BackendObservation {
        let state = match self.handle.state() {
            PlaybackState::Playing
            | PlaybackState::Pausing
            | PlaybackState::WaitingToResume
            | PlaybackState::Resuming
            | PlaybackState::Stopping => BackendPlaybackState::Advancing,
            PlaybackState::Paused => BackendPlaybackState::Paused,
            PlaybackState::Stopped => BackendPlaybackState::Stopped,
        };
        BackendObservation {
            state,
            position_seconds: self.handle.position() as f32,
            error: self.handle.pop_error().map(|error| error.to_string()),
        }
    }

    fn pause(&mut self) {
        self.handle.pause(instant_tween());
    }

    fn resume(&mut self) {
        self.handle.resume(instant_tween());
    }

    fn seek_to(&mut self, position_seconds: f32) {
        self.handle.seek_to(f64::from(position_seconds));
    }

    fn stop(&mut self) {
        self.handle.stop(instant_tween());
    }
}

pub(super) fn canonical_audio_path(path: &str) -> Result<String, std::io::Error> {
    std::fs::canonicalize(path).map(|path| path.to_string_lossy().into_owned())
}

pub(super) fn instant_tween() -> Tween {
    Tween {
        duration: Duration::ZERO,
        ..Tween::default()
    }
}
