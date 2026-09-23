use donder_elaboration::{
    OutputEvaluationWorkspace, PreparedSequenceOutput as RuntimeSequenceOutput,
    RenderedSequenceFrame, SequenceOutputPrepareError as RuntimePrepareError,
    SequenceOutputRenderError as RuntimeRenderError,
};
use donder_language::model::DonderProject;
use donder_language::sequence::SequenceId;
use donder_language::setup::SetupId;

use crate::dto::{AudioTransportSnapshot, AudioTransportState};

pub(crate) struct SequenceRenderService {
    session: Option<SequenceRenderSession>,
    session_generation: u64,
}

pub struct PreparedSequenceOutput {
    setup_id: SetupId,
    sequence_id: SequenceId,
    renderer: RuntimeSequenceOutput,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioClockRenderedFrame {
    pub audio_generation: u32,
    pub frame: RenderedSequenceFrame,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioClockRenderIdentity {
    pub session_generation: u64,
    pub audio_generation: u32,
    pub audio_state: AudioTransportState,
    pub position_seconds: f32,
    pub frame_rate: u32,
    pub frame_count: u32,
    pub frame_index: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SequenceRenderError {
    NoSequenceRenderSession,
    ClockUnavailable { state: AudioTransportState },
    Render(RuntimeRenderError),
}

impl SequenceRenderService {
    pub(crate) fn new() -> Self {
        Self {
            session: None,
            session_generation: 0,
        }
    }

    pub fn prepare(
        &mut self,
        project: &DonderProject,
        setup_id: &SetupId,
        sequence_id: &SequenceId,
    ) -> Result<(), RuntimePrepareError> {
        let session = prepare_sequence_output(project, setup_id, sequence_id)?;
        self.apply_prepared(session);
        Ok(())
    }

    pub fn unload(&mut self) {
        if self.session.is_some() {
            self.session_generation = self.session_generation.saturating_add(1);
            self.session = None;
        }
    }

    pub fn refresh_project(&mut self, project: &DonderProject) -> Result<(), RuntimePrepareError> {
        let Some(session) = self.session.as_ref() else {
            return Ok(());
        };
        let setup_id = session.setup_id.clone();
        let sequence_id = session.sequence_id.clone();
        if !project.sequences.contains_key(&sequence_id) {
            self.unload();
            return Ok(());
        }
        self.prepare(project, &setup_id, &sequence_id)
    }

    pub fn render_current_sequence_frame(
        &mut self,
        audio: &AudioTransportSnapshot,
    ) -> Result<AudioClockRenderedFrame, SequenceRenderError> {
        let session = self
            .session
            .as_mut()
            .ok_or(SequenceRenderError::NoSequenceRenderSession)?;
        match audio.state {
            AudioTransportState::Stopped
            | AudioTransportState::Paused
            | AudioTransportState::Playing
            | AudioTransportState::Ended => {}
            AudioTransportState::Unloaded | AudioTransportState::Error => {
                return Err(SequenceRenderError::ClockUnavailable {
                    state: audio.state.clone(),
                });
            }
        }
        let frame_index = frame_index_for_audio_seconds(
            audio.position_seconds,
            session.renderer.frame_rate(),
            session.renderer.frame_count(),
        );
        let frame = if session.cached.as_ref().is_some_and(|cached| {
            cached.audio_generation == audio.generation && cached.frame.frame_index == frame_index
        }) {
            session
                .cached
                .as_ref()
                .map(|cached| cached.frame.clone())
                .ok_or(SequenceRenderError::NoSequenceRenderSession)?
        } else {
            let frame = session
                .renderer
                .render_seconds_with_workspace(audio.position_seconds, &mut session.workspace)
                .map_err(SequenceRenderError::Render)?;
            session.cached = Some(AudioClockRenderedFrame {
                audio_generation: audio.generation,
                frame: frame.clone(),
            });
            frame
        };
        Ok(AudioClockRenderedFrame {
            audio_generation: audio.generation,
            frame,
        })
    }

    pub fn active_render_identity(
        &self,
        audio: &AudioTransportSnapshot,
    ) -> Result<AudioClockRenderIdentity, SequenceRenderError> {
        let session = self
            .session
            .as_ref()
            .ok_or(SequenceRenderError::NoSequenceRenderSession)?;
        match audio.state {
            AudioTransportState::Stopped
            | AudioTransportState::Paused
            | AudioTransportState::Playing
            | AudioTransportState::Ended => {}
            AudioTransportState::Unloaded | AudioTransportState::Error => {
                return Err(SequenceRenderError::ClockUnavailable {
                    state: audio.state.clone(),
                });
            }
        }
        Ok(AudioClockRenderIdentity {
            session_generation: self.session_generation,
            audio_generation: audio.generation,
            audio_state: audio.state.clone(),
            position_seconds: audio.position_seconds,
            frame_rate: session.renderer.frame_rate(),
            frame_count: session.renderer.frame_count(),
            frame_index: frame_index_for_audio_seconds(
                audio.position_seconds,
                session.renderer.frame_rate(),
                session.renderer.frame_count(),
            ),
        })
    }

    pub fn active_target(&self) -> Option<(SetupId, SequenceId)> {
        self.session
            .as_ref()
            .map(|session| (session.setup_id.clone(), session.sequence_id.clone()))
    }

    pub fn apply_prepared(&mut self, session: PreparedSequenceOutput) {
        self.session_generation = self.session_generation.saturating_add(1);
        let workspace = session.renderer.workspace();
        self.session = Some(SequenceRenderSession {
            setup_id: session.setup_id,
            sequence_id: session.sequence_id,
            renderer: session.renderer,
            workspace,
            cached: None,
        });
    }
}

struct SequenceRenderSession {
    setup_id: SetupId,
    sequence_id: SequenceId,
    renderer: RuntimeSequenceOutput,
    workspace: OutputEvaluationWorkspace,
    cached: Option<AudioClockRenderedFrame>,
}

pub fn prepare_sequence_output(
    project: &DonderProject,
    setup_id: &SetupId,
    sequence_id: &SequenceId,
) -> Result<PreparedSequenceOutput, RuntimePrepareError> {
    let renderer = RuntimeSequenceOutput::prepare(project, setup_id, sequence_id)?;
    Ok(PreparedSequenceOutput {
        setup_id: setup_id.clone(),
        sequence_id: sequence_id.clone(),
        renderer,
    })
}

fn frame_index_for_audio_seconds(audio_seconds: f32, frame_rate: u32, frame_count: u32) -> u32 {
    let max_frame = frame_count.saturating_sub(1);
    let frame_index = (audio_seconds * frame_rate as f32).floor();
    if frame_index < 0.0 {
        0
    } else if frame_index > max_frame as f32 {
        max_frame
    } else {
        frame_index as u32
    }
}
