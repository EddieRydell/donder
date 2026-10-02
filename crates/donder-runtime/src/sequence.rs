use crate::patch::{PatchError, PreparedPatch};
use crate::signal::{EvaluationError, EvaluationWorkspace, PreparedSignalGraph, RenderedFixture};
use crate::values::SampleTime;
use alloc::{boxed::Box, vec::Vec};

/// Frozen playback data; authoring, elaboration, networking, and pin timing are external.
/// Its graph cannot be changed through the playback object:
///
/// ```compile_fail
/// fn overwrite(sequence: &mut donder_runtime::sequence::PreparedSequence) {
///     sequence.signals.effects = Box::new([]);
/// }
/// ```
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedSequence {
    pub(crate) workspace_key: u32,
    pub(crate) signals: PreparedSignalGraph,
    pub(crate) patch: PreparedPatch,
    pub(crate) outputs: Box<[PreparedOutput]>,
}

/// One output buffer. Controller indices refer to the active setup's controller
/// order; authored document identities and network protocols stay on the host.
#[derive(Clone, Debug, Eq, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedOutput {
    pub controller_index: usize,
    pub port: u32,
    pub width: u32,
}

#[derive(Debug)]
pub struct SequenceWorkspace {
    workspace_key: u32,
    signals: EvaluationWorkspace,
}

/// Owns an admitted sequence together with the workspace sized for it.
/// Playback can inspect the sequence but cannot mutate it after admission.
pub struct SequencePlayback {
    sequence: PreparedSequence,
    workspace: SequenceWorkspace,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SequenceError {
    InvalidWorkspace,
    Signal(EvaluationError),
    Patch(PatchError),
}

impl PreparedSequence {
    /// Assemble the portable sequence after elaboration has finished lowering and
    /// selecting its outputs. Scratch storage is created from this same graph.
    pub fn new(
        signals: PreparedSignalGraph,
        patch: PreparedPatch,
        outputs: Box<[PreparedOutput]>,
    ) -> Self {
        Self {
            workspace_key: signals.workspace_key,
            signals,
            patch,
            outputs,
        }
    }

    pub fn signals(&self) -> &PreparedSignalGraph {
        &self.signals
    }

    pub fn patch(&self) -> &PreparedPatch {
        &self.patch
    }

    pub fn outputs(&self) -> &[PreparedOutput] {
        &self.outputs
    }

    /// Resolve an authored clip within this prepared sequence, including all of
    /// its generator children. No source project or second preparation is needed.
    pub fn clip(&self, id: u32) -> Option<crate::clip::SequenceClip<'_>> {
        self.signals
            .clips
            .iter()
            .find(|clip| clip.id == id)
            .map(|clip| crate::clip::SequenceClip {
                graph: &self.signals,
                clip,
            })
    }

    pub fn into_playback(self) -> Result<SequencePlayback, crate::wire::LoadError> {
        let workspace = self.workspace()?;
        Ok(SequencePlayback {
            sequence: self,
            workspace,
        })
    }

    pub fn workspace(&self) -> Result<SequenceWorkspace, crate::wire::LoadError> {
        crate::wire::validate_prepared_sequence(self)?;
        Ok(SequenceWorkspace {
            workspace_key: self.workspace_key,
            signals: self.signals.workspace_unchecked(),
        })
    }

    pub fn rendered_fixtures(
        &self,
        workspace: &SequenceWorkspace,
    ) -> Result<Vec<RenderedFixture>, SequenceError> {
        if workspace.workspace_key != self.workspace_key {
            return Err(SequenceError::InvalidWorkspace);
        }
        self.signals
            .snapshot(&workspace.signals)
            .map_err(SequenceError::Signal)
    }

    /// Evaluate and pack directly from signal storage, without intermediate pixel copies.
    pub fn evaluate(
        &self,
        sample_time: SampleTime,
        buffers: &mut [impl AsMut<[u8]>],
        workspace: &mut SequenceWorkspace,
    ) -> Result<(), SequenceError> {
        if workspace.workspace_key != self.workspace_key {
            return Err(SequenceError::InvalidWorkspace);
        }
        if buffers.len() != self.outputs.len()
            || buffers
                .iter_mut()
                .zip(&self.outputs)
                .any(|(buffer, output)| buffer.as_mut().len() != output.width as usize)
        {
            return Err(SequenceError::Patch(PatchError::WidthMismatch));
        }
        let colors = self
            .signals
            .evaluate(sample_time, &mut workspace.signals)
            .map_err(SequenceError::Signal)?;
        self.patch
            .evaluate(colors, buffers)
            .map_err(SequenceError::Patch)
    }
}

impl SequencePlayback {
    pub fn sequence(&self) -> &PreparedSequence {
        &self.sequence
    }

    pub fn evaluate(
        &mut self,
        sample_time: SampleTime,
        buffers: &mut [impl AsMut<[u8]>],
    ) -> Result<(), SequenceError> {
        self.sequence
            .evaluate(sample_time, buffers, &mut self.workspace)
    }

    pub fn evaluate_signals(
        &mut self,
        sample_time: SampleTime,
    ) -> Result<&[crate::values::Color], EvaluationError> {
        self.sequence
            .signals
            .evaluate(sample_time, &mut self.workspace.signals)
    }

    pub fn rendered_fixtures(&self) -> Result<Vec<RenderedFixture>, SequenceError> {
        self.sequence.rendered_fixtures(&self.workspace)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::{PreparedSignalKind, PreparedSignalNode, PreparedTarget, SignalPlan};
    use crate::values::SampleDuration;
    use alloc::vec;

    fn empty_sequence() -> PreparedSequence {
        PreparedSequence::new(
            PreparedSignalGraph {
                clips: Box::new([]),
                parameter_environments: Box::new([]),
                workspace_key: 1,
                frame_rate: 60,
                frame_count: 60,
                duration: SampleDuration::from_ticks(1_000_000),
                fixtures: Box::new([]),
                fixture_pixel_offsets: Box::new([]),
                pixel_count: 0,
                effects: Box::new([]),
                programs: Box::new([]),
                targets: vec![PreparedTarget {
                    pixels: 0..0,
                    sample_count: 0,
                }]
                .into(),
                target_pixels: Box::new([]),
                spatial_contexts: Box::new([]),
                effects_by_layer: Box::new([]),
                layers: Box::new([]),
                plan: SignalPlan {
                    output_index: 0,
                    target: 0,
                    nodes: vec![PreparedSignalNode {
                        kind: PreparedSignalKind::Output {
                            inputs: Box::new([]),
                        },
                    }]
                    .into(),
                    vm_workspace_count: 0,
                    frame_nodes: vec![0].into(),
                    frame_slots: vec![0].into(),
                    frame_buffer_count: 1,
                },
            },
            PreparedPatch {
                routes: Box::new([]),
                lookups: Box::new([]),
            },
            vec![PreparedOutput {
                controller_index: 3,
                port: 7,
                width: 3,
            }]
            .into(),
        )
    }

    #[test]
    fn constructor_keeps_output_identity_and_empty_sequences_clear_outputs() {
        let sequence = empty_sequence();
        assert_eq!(
            sequence.outputs(),
            &[PreparedOutput {
                controller_index: 3,
                port: 7,
                width: 3
            }]
        );
        let mut workspace = sequence.workspace().unwrap();
        let mut output = [vec![255; 3]];
        sequence
            .evaluate(SampleTime::from_ticks(0), &mut output, &mut workspace)
            .unwrap();
        assert_eq!(output[0], [0, 0, 0]);
    }

    #[test]
    fn archive_roundtrip_preserves_output_metadata_and_empty_playback() {
        let sequence = empty_sequence();
        let bytes = crate::wire::encode_sequence(&sequence).unwrap();
        let decoded =
            crate::wire::decode_sequence(&bytes, crate::wire::LoadLimits::default()).unwrap();
        assert_eq!(decoded.outputs(), sequence.outputs());
        let mut playback = decoded.into_playback().unwrap();
        let mut output = [vec![255; 3]];
        playback
            .evaluate(SampleTime::from_ticks(500_000), &mut output)
            .unwrap();
        assert_eq!(output[0], [0, 0, 0]);
    }
}
