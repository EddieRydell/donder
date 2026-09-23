use std::sync::atomic::{AtomicU32, Ordering};

use donder_language::controller::{ControllerId, ControllerPortId};
use donder_language::model::DonderProject;
use donder_language::sequence::SequenceId;
use donder_language::setup::SetupId;
use donder_language::validation::validate_project;
use donder_language::values::{SampleTime, sample_time_from_seconds_f32};

use super::errors::{SequenceOutputPrepareError, SequenceOutputRenderError};
use super::frame::{ControllerPortFrame, RenderedSequenceFrame};
use super::patch::prepare_patch;
use crate::RenderError;
use crate::sequence::elaboration::prepare_validated_sequence;
use crate::sequence::timeline::{frame_at_or_before, sample_time_for_frame};
use donder_runtime::sequence::{PreparedSequence, SequenceWorkspace};

static NEXT_OUTPUT_ID: AtomicU32 = AtomicU32::new(1);

pub struct PreparedSequenceOutput {
    pub sequence: PreparedSequence,
    controller_ports: Box<[ControllerPortFrame]>,
}

#[derive(Debug)]
pub struct OutputEvaluationWorkspace {
    sequence: SequenceWorkspace,
    controller_frames: Vec<ControllerPortFrame>,
}

impl PreparedSequenceOutput {
    /// Encode the prepared output selection in the portable controller format.
    pub fn encode(&self) -> Result<Vec<u8>, donder_runtime::wire::LoadError> {
        donder_runtime::wire::encode_sequence(&self.sequence)
    }

    pub fn prepare(
        project: &DonderProject,
        setup_id: &SetupId,
        sequence_id: &SequenceId,
    ) -> Result<Self, SequenceOutputPrepareError> {
        Self::prepare_outputs(project, setup_id, sequence_id, None)
    }

    /// Prepare only these ports, in the requested order. An empty selection
    /// produces an empty fragment. Pixel contexts keep their authored coordinates.
    pub fn prepare_selected(
        project: &DonderProject,
        setup_id: &SetupId,
        sequence_id: &SequenceId,
        outputs: &[(ControllerId, ControllerPortId)],
    ) -> Result<Self, SequenceOutputPrepareError> {
        Self::prepare_outputs(project, setup_id, sequence_id, Some(outputs))
    }

    fn prepare_outputs(
        project: &DonderProject,
        setup_id: &SetupId,
        sequence_id: &SequenceId,
        outputs: Option<&[(ControllerId, ControllerPortId)]>,
    ) -> Result<Self, SequenceOutputPrepareError> {
        validate_project(project)
            .map_err(|error| SequenceOutputPrepareError::ProjectValidation(format!("{error:?}")))?;
        let setup = project
            .setups
            .get(setup_id)
            .ok_or(SequenceOutputPrepareError::MissingSetup)?;
        let layout = project
            .layouts
            .get(&setup.layout)
            .ok_or(SequenceOutputPrepareError::MissingLayout)?
            .clone();
        let sequence_definition = project
            .sequences
            .get(sequence_id)
            .ok_or(SequenceOutputPrepareError::MissingSequence)?;
        if sequence_definition
            .effects
            .iter()
            .any(|effect| effect.target.layout != layout.id)
        {
            return Err(SequenceOutputPrepareError::InvalidEffectLayout);
        }
        let sequence = prepare_validated_sequence(project, setup_id, sequence_definition)
            .map_err(SequenceOutputPrepareError::Render)?;
        let patch = project.patches.get(&setup.patch).ok_or_else(|| {
            SequenceOutputPrepareError::InvalidPatch("setup patch is missing".to_string())
        })?;
        let mut controller_ports = Vec::new();
        for controller_id in &setup.controllers {
            let controller = project.controllers.get(controller_id).ok_or_else(|| {
                SequenceOutputPrepareError::InvalidPatch("setup controller is missing".to_string())
            })?;
            for port in &controller.ports {
                controller_ports.push(ControllerPortFrame {
                    controller: controller_id.clone(),
                    port: port.id,
                    slots: vec![0; usize::from(port.slot_count)],
                });
            }
        }
        if let Some(outputs) = outputs {
            let mut selected = Vec::with_capacity(outputs.len());
            for (index, (controller, port)) in outputs.iter().enumerate() {
                if outputs[..index]
                    .iter()
                    .any(|(previous_controller, previous_port)| {
                        previous_controller == controller && previous_port == port
                    })
                {
                    return Err(SequenceOutputPrepareError::DuplicateOutput {
                        controller: controller.clone(),
                        port: *port,
                    });
                }
                let frame = controller_ports
                    .iter()
                    .find(|frame| frame.controller == *controller && frame.port == *port)
                    .ok_or_else(|| SequenceOutputPrepareError::UnknownOutput {
                        controller: controller.clone(),
                        port: *port,
                    })?;
                selected.push(frame.clone());
            }
            controller_ports = selected;
        }
        let patch = prepare_patch(&layout, patch, &sequence, &controller_ports)?;
        let mut output = Self {
            sequence: PreparedSequence {
                workspace_key: NEXT_OUTPUT_ID.fetch_add(1, Ordering::Relaxed),
                signals: sequence,
                patch,
                output_widths: controller_ports
                    .iter()
                    .map(|port| port.slots.len() as u32)
                    .collect(),
            },
            controller_ports: controller_ports.into_boxed_slice(),
        };
        if outputs.is_some() {
            super::fragment::compact(&mut output.sequence)
                .map_err(SequenceOutputPrepareError::Render)?;
        }
        Ok(output)
    }

    pub fn render_seconds(
        &self,
        seconds: f32,
    ) -> Result<RenderedSequenceFrame, SequenceOutputRenderError> {
        self.render_seconds_with_workspace(seconds, &mut self.workspace())
    }

    pub fn render_seconds_with_workspace(
        &self,
        seconds: f32,
        workspace: &mut OutputEvaluationWorkspace,
    ) -> Result<RenderedSequenceFrame, SequenceOutputRenderError> {
        if !seconds.is_finite() {
            return Err(SequenceOutputRenderError::Render(
                RenderError::InvalidTiming {
                    reason: "audio seconds must be finite".to_string(),
                },
            ));
        }
        let sample_time = sample_time_from_seconds_f32(seconds).map_err(|_| {
            SequenceOutputRenderError::Render(RenderError::InvalidTiming {
                reason: "audio seconds exceed the runtime clock range".to_string(),
            })
        })?;
        let frame = frame_at_or_before(sample_time, self.frame_rate());
        self.render_at(frame, sample_time, workspace)
    }

    pub fn render_frame(
        &self,
        frame: u32,
    ) -> Result<RenderedSequenceFrame, SequenceOutputRenderError> {
        let mut workspace = self.workspace();
        let sample_time = sample_time_for_frame(frame, self.frame_rate())
            .map_err(SequenceOutputRenderError::Render)?;
        self.render_at(frame, sample_time, &mut workspace)
    }

    pub fn frame_rate(&self) -> u32 {
        self.sequence.signals.frame_rate()
    }
    pub fn frame_count(&self) -> u32 {
        self.sequence.signals.frame_count()
    }

    pub fn workspace(&self) -> OutputEvaluationWorkspace {
        OutputEvaluationWorkspace {
            sequence: self.sequence.workspace(),
            controller_frames: self.controller_ports.to_vec(),
        }
    }

    /// Samples controller port bytes into buffers owned by `workspace`.
    /// Repeated calls for the same prepared output reuse all output-stage
    /// fixture, patch, and controller storage.
    pub fn sample_into<'a>(
        &self,
        sample_time: SampleTime,
        workspace: &'a mut OutputEvaluationWorkspace,
    ) -> Result<&'a [ControllerPortFrame], SequenceOutputRenderError> {
        self.sequence
            .evaluate(
                sample_time,
                &mut workspace.controller_frames,
                &mut workspace.sequence,
            )
            .map_err(SequenceOutputRenderError::from)?;
        Ok(&workspace.controller_frames)
    }

    fn render_at(
        &self,
        frame_index: u32,
        sample_time: SampleTime,
        workspace: &mut OutputEvaluationWorkspace,
    ) -> Result<RenderedSequenceFrame, SequenceOutputRenderError> {
        self.sample_into(sample_time, workspace)?;
        Ok(RenderedSequenceFrame {
            frame_index,
            frame_rate: self.frame_rate(),
            sample_time,
            fixtures: self
                .sequence
                .rendered_fixtures(&workspace.sequence)
                .map_err(SequenceOutputRenderError::from)?,
            controller_frames: workspace.controller_frames.clone(),
        })
    }
}
