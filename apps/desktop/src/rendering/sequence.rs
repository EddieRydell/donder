use donder_elaboration::{PreparationCache, PrepareOutputs, prepare_cached};
use donder_model::DonderProject;
use donder_model::SequenceId;
use donder_model::SetupId;
use donder_model::{ControllerId, ControllerPortId};
use donder_output::ControllerPortFrame;
use donder_runtime::{PlaybackRate, SequencePlayback};
use donder_runtime_types::{Color, SampleTime, SampleTimeError, sample_time_from_seconds_f32};
use std::time::Duration;

use donder_sequence_api::{AudioTransportSnapshot, AudioTransportState};

pub(crate) struct SequenceRenderService {
    session: Option<PreparedRenderSession>,
    session_generation: u64,
    preparation: PreparationCache,
}

/// Desktop ownership around portable playback: document identities, network
/// output identities and an audio-generation cache are not elaboration concerns.
pub(crate) struct PreparedRenderSession {
    setup_id: SetupId,
    sequence_id: SequenceId,
    playback: SequencePlayback,
    controllers: Vec<ControllerId>,
    cached: Option<AudioClockRenderedFrame>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderedFixture {
    pub fixture_id: u32,
    pub pixels: Vec<Color>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderedSequenceFrame {
    /// Index on the playback frame grid, which follows the playback speed.
    pub frame_index: u32,
    pub frame_rate: u32,
    /// Wall time between playback frames.
    pub frame_interval: Duration,
    pub sample_time: SampleTime,
    pub fixtures: Vec<RenderedFixture>,
    pub controller_frames: Vec<ControllerPortFrame>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AudioClockRenderedFrame {
    pub audio_generation: u32,
    pub frame: RenderedSequenceFrame,
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AudioClockRenderIdentity {
    pub session_generation: u64,
    pub audio_generation: u32,
    pub audio_state: AudioTransportState,
    pub position_seconds: f32,
    pub frame_rate: u32,
    pub frame_count: u32,
    pub frame_index: u32,
}

pub(crate) struct EncodedPreviewSequence {
    pub(crate) bytes: Vec<u8>,
    pub(crate) fixtures: Vec<(u32, usize)>,
}

#[derive(Debug)]
pub(crate) enum RenderSessionPrepareError {
    SelectionUnavailable,
}

impl std::fmt::Display for RenderSessionPrepareError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SelectionUnavailable => formatter.write_str("sequence selection is unavailable"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SequenceRenderError {
    NoSequenceRenderSession,
    ClockUnavailable { state: AudioTransportState },
    InvalidClock(SampleTimeError),
    InvalidPlaybackSpeed(String),
}

impl SequenceRenderService {
    pub(crate) fn new() -> Self {
        Self {
            session: None,
            session_generation: 0,
            preparation: PreparationCache::default(),
        }
    }

    pub(crate) fn prepare(
        &mut self,
        project: &DonderProject,
        sequence_id: &SequenceId,
    ) -> Result<(), RenderSessionPrepareError> {
        let session = prepare_render_session_cached(project, sequence_id, &mut self.preparation)?;
        self.apply_prepared(session);
        Ok(())
    }

    pub(crate) fn unload(&mut self) {
        if self.session.take().is_some() {
            self.session_generation = self.session_generation.saturating_add(1);
        }
    }

    pub(crate) fn refresh_project(
        &mut self,
        project: &DonderProject,
    ) -> Result<(), RenderSessionPrepareError> {
        let Some(sequence_id) = self
            .session
            .as_ref()
            .map(|session| session.sequence_id.clone())
        else {
            return Ok(());
        };
        if project.sequence(&sequence_id).is_none() {
            self.unload();
            return Ok(());
        }
        self.prepare(project, &sequence_id)
    }

    pub(crate) fn render_current_sequence_frame(
        &mut self,
        audio: &AudioTransportSnapshot,
    ) -> Result<AudioClockRenderedFrame, SequenceRenderError> {
        let session = self
            .session
            .as_mut()
            .ok_or(SequenceRenderError::NoSequenceRenderSession)?;
        require_audio_clock(audio)?;
        let rate = playback_rate(audio)?;
        let frame_rate = session.playback.sequence().frame_rate();
        let sample_time = sample_time_from_seconds_f32(audio.position_seconds)
            .map_err(SequenceRenderError::InvalidClock)?;
        let frame_index = playback_frame_index(rate, sample_time, frame_rate);
        if let Some(cached) = &session.cached
            && cached.audio_generation == audio.generation
            && cached.frame.frame_index == frame_index
        {
            return Ok(cached.clone());
        }
        let rendered = session.playback.evaluate(sample_time);
        let frame = AudioClockRenderedFrame {
            audio_generation: audio.generation,
            frame: RenderedSequenceFrame {
                frame_index,
                frame_rate,
                frame_interval: Duration::from_micros(
                    rate.wall_elapsed(rate.frame_start(1, frame_rate)),
                ),
                sample_time,
                fixtures: rendered
                    .fixtures()
                    .map(|fixture| RenderedFixture {
                        fixture_id: fixture.fixture_id,
                        pixels: fixture.pixels.to_vec(),
                    })
                    .collect(),
                controller_frames: rendered
                    .outputs()
                    .map(|output| ControllerPortFrame {
                        controller: session.controllers[output.output.controller_index as usize]
                            .clone(),
                        port: ControllerPortId(output.output.port),
                        slots: output.bytes.to_vec(),
                    })
                    .collect(),
            },
        };
        session.cached = Some(frame.clone());
        Ok(frame)
    }

    #[cfg(test)]
    pub(crate) fn active_render_identity(
        &self,
        audio: &AudioTransportSnapshot,
    ) -> Result<AudioClockRenderIdentity, SequenceRenderError> {
        let session = self
            .session
            .as_ref()
            .ok_or(SequenceRenderError::NoSequenceRenderSession)?;
        require_audio_clock(audio)?;
        let sequence = session.playback.sequence();
        Ok(AudioClockRenderIdentity {
            session_generation: self.session_generation,
            audio_generation: audio.generation,
            audio_state: audio.state.clone(),
            position_seconds: audio.position_seconds,
            frame_rate: sequence.frame_rate(),
            frame_count: sequence.frame_count(),
            frame_index: playback_frame_index(
                playback_rate(audio)?,
                sample_time_from_seconds_f32(audio.position_seconds)
                    .map_err(SequenceRenderError::InvalidClock)?,
                sequence.frame_rate(),
            ),
        })
    }

    pub(crate) fn active_target(&self) -> Option<(SetupId, SequenceId)> {
        self.session
            .as_ref()
            .map(|session| (session.setup_id.clone(), session.sequence_id.clone()))
    }

    pub(crate) fn session_generation(&self) -> u64 {
        self.session_generation
    }

    pub(crate) fn encode_preview_sequence(&self) -> Result<Option<EncodedPreviewSequence>, String> {
        self.session
            .as_ref()
            .map(|session| {
                let sequence = session.playback.sequence();
                Ok(EncodedPreviewSequence {
                    bytes: donder_runtime::encode_sequence(sequence)
                        .map_err(|error| format!("{error:?}"))?,
                    fixtures: sequence
                        .fixtures()
                        .iter()
                        .map(|fixture| (fixture.id, fixture.pixel_count))
                        .collect(),
                })
            })
            .transpose()
    }

    pub(crate) fn apply_prepared(&mut self, session: PreparedRenderSession) {
        self.session_generation = self.session_generation.saturating_add(1);
        self.session = Some(session);
    }
}

/// A render session prepared without reusing earlier clip programs.
#[cfg(test)]
pub(crate) fn prepare_render_session(
    project: &DonderProject,
    sequence_id: &SequenceId,
) -> Result<PreparedRenderSession, RenderSessionPrepareError> {
    prepare_render_session_cached(project, sequence_id, &mut PreparationCache::default())
}

/// Preparation uses the active setup in this same project snapshot. Build its
/// playback workspace on the background worker before installing the session.
/// `cache` keeps unchanged clips lowered between preparations.
pub(crate) fn prepare_render_session_cached(
    project: &DonderProject,
    sequence_id: &SequenceId,
    cache: &mut PreparationCache,
) -> Result<PreparedRenderSession, RenderSessionPrepareError> {
    let sequence = prepare_cached(project, sequence_id, PrepareOutputs::All, cache)
        .ok_or(RenderSessionPrepareError::SelectionUnavailable)?;
    let setup = project
        .setup(project.root().setup.id())
        .ok_or(RenderSessionPrepareError::SelectionUnavailable)?;
    let controllers = setup
        .controllers
        .iter()
        .map(|source| source.id().clone())
        .collect();
    Ok(PreparedRenderSession {
        setup_id: setup.id.clone(),
        sequence_id: sequence_id.clone(),
        playback: sequence.into_playback(),
        controllers,
        cached: None,
    })
}

fn require_audio_clock(audio: &AudioTransportSnapshot) -> Result<(), SequenceRenderError> {
    match audio.state {
        AudioTransportState::Stopped
        | AudioTransportState::Paused
        | AudioTransportState::Playing
        | AudioTransportState::Ended => Ok(()),
        AudioTransportState::Unloaded | AudioTransportState::Error => {
            Err(SequenceRenderError::ClockUnavailable {
                state: audio.state.clone(),
            })
        }
    }
}

fn playback_rate(audio: &AudioTransportSnapshot) -> Result<PlaybackRate, SequenceRenderError> {
    PlaybackRate::try_from(audio.playback_speed).map_err(SequenceRenderError::InvalidPlaybackSpeed)
}

fn playback_frame_index(rate: PlaybackRate, sample_time: SampleTime, frame_rate: u32) -> u32 {
    rate.frame_at(u64::from(sample_time.as_ticks()), frame_rate)
        .min(u64::from(u32::MAX)) as u32
}
