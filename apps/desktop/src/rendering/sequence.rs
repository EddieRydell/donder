use donder_elaboration::{PrepareOutputs, prepare};
use donder_language::controller::ControllerPortId;
use donder_language::model::DonderProject;
use donder_language::sequence::SequenceId;
use donder_language::setup::SetupId;
use donder_output::ControllerPortFrame;
use donder_runtime::sequence::{SequenceError, SequencePlayback};
use donder_runtime::signal::RenderedFixture;
use donder_runtime::values::{SampleTime, SampleTimeError, sample_time_from_seconds_f32};

use crate::dto::{AudioTransportSnapshot, AudioTransportState};

pub(crate) struct SequenceRenderService {
    session: Option<PreparedRenderSession>,
    session_generation: u64,
}

/// Desktop ownership around portable playback: document identities, network
/// output buffers, and an audio-generation cache are not elaboration concerns.
pub(crate) struct PreparedRenderSession {
    setup_id: SetupId,
    sequence_id: SequenceId,
    playback: SequencePlayback,
    controller_frames: Vec<ControllerPortFrame>,
    cached: Option<AudioClockRenderedFrame>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderedSequenceFrame {
    pub frame_index: u32,
    pub frame_rate: u32,
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
    Playback(donder_runtime::wire::LoadError),
}

impl std::fmt::Display for RenderSessionPrepareError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SelectionUnavailable => formatter.write_str("sequence selection is unavailable"),
            Self::Playback(error) => write!(formatter, "playback admission failed: {error:?}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SequenceRenderError {
    NoSequenceRenderSession,
    ClockUnavailable { state: AudioTransportState },
    InvalidClock(SampleTimeError),
    Render(SequenceError),
}

impl SequenceRenderService {
    pub(crate) fn new() -> Self {
        Self {
            session: None,
            session_generation: 0,
        }
    }

    pub(crate) fn prepare(
        &mut self,
        project: &DonderProject,
        sequence_id: &SequenceId,
    ) -> Result<(), RenderSessionPrepareError> {
        self.apply_prepared(prepare_render_session(project, sequence_id)?);
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
        let signals = session.playback.sequence().signals();
        let frame_rate = signals.frame_rate;
        let frame_index =
            frame_index_for_audio_seconds(audio.position_seconds, frame_rate, signals.frame_count);
        if let Some(cached) = &session.cached
            && cached.audio_generation == audio.generation
            && cached.frame.frame_index == frame_index
        {
            return Ok(cached.clone());
        }
        let sample_time = sample_time_from_seconds_f32(audio.position_seconds)
            .map_err(SequenceRenderError::InvalidClock)?;
        session
            .playback
            .evaluate(sample_time, &mut session.controller_frames)
            .map_err(SequenceRenderError::Render)?;
        let frame = AudioClockRenderedFrame {
            audio_generation: audio.generation,
            frame: RenderedSequenceFrame {
                frame_index,
                frame_rate,
                sample_time,
                fixtures: session
                    .playback
                    .rendered_fixtures()
                    .map_err(SequenceRenderError::Render)?,
                controller_frames: session.controller_frames.clone(),
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
        let signals = session.playback.sequence().signals();
        Ok(AudioClockRenderIdentity {
            session_generation: self.session_generation,
            audio_generation: audio.generation,
            audio_state: audio.state.clone(),
            position_seconds: audio.position_seconds,
            frame_rate: signals.frame_rate,
            frame_count: signals.frame_count,
            frame_index: frame_index_for_audio_seconds(
                audio.position_seconds,
                signals.frame_rate,
                signals.frame_count,
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
                    bytes: donder_runtime::wire::encode_sequence(sequence)
                        .map_err(|error| format!("{error:?}"))?,
                    fixtures: sequence
                        .signals()
                        .fixtures
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

/// Preparation uses the active setup in this same project snapshot. Build its
/// playback workspace on the background worker before installing the session.
pub(crate) fn prepare_render_session(
    project: &DonderProject,
    sequence_id: &SequenceId,
) -> Result<PreparedRenderSession, RenderSessionPrepareError> {
    let sequence = prepare(project, sequence_id, PrepareOutputs::All)
        .ok_or(RenderSessionPrepareError::SelectionUnavailable)?;
    let setup = project
        .setup(project.root.setup.id())
        .ok_or(RenderSessionPrepareError::SelectionUnavailable)?;
    let controller_frames = sequence
        .outputs()
        .iter()
        .map(|output| ControllerPortFrame {
            controller: setup.controllers[output.controller_index].id().clone(),
            port: ControllerPortId(output.port),
            slots: vec![0; output.width as usize],
        })
        .collect();
    Ok(PreparedRenderSession {
        setup_id: setup.id.clone(),
        sequence_id: sequence_id.clone(),
        playback: sequence
            .into_playback()
            .map_err(RenderSessionPrepareError::Playback)?,
        controller_frames,
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

fn frame_index_for_audio_seconds(audio_seconds: f32, frame_rate: u32, frame_count: u32) -> u32 {
    ((audio_seconds * frame_rate as f32).floor() as u32).min(frame_count.saturating_sub(1))
}
