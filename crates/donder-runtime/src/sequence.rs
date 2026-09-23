use crate::patch::{PatchError, PreparedPatch};
use crate::signal::{EvaluationError, EvaluationWorkspace, PreparedSignalGraph, RenderedFixture};
use crate::values::SampleTime;
use alloc::{boxed::Box, vec::Vec};

/// Frozen playback data; authoring, elaboration, networking, and pin timing are external.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedSequence {
    pub workspace_key: u32,
    pub signals: PreparedSignalGraph,
    pub patch: PreparedPatch,
    pub output_widths: Box<[u32]>,
}

#[derive(Debug)]
pub struct SequenceWorkspace {
    workspace_key: u32,
    signals: EvaluationWorkspace,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SequenceError {
    InvalidWorkspace,
    Signal(EvaluationError),
    Patch(PatchError),
}

impl PreparedSequence {
    pub fn workspace(&self) -> SequenceWorkspace {
        SequenceWorkspace {
            workspace_key: self.workspace_key,
            signals: self.signals.workspace(),
        }
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
        if buffers.len() != self.output_widths.len()
            || buffers
                .iter_mut()
                .zip(&self.output_widths)
                .any(|(buffer, width)| buffer.as_mut().len() != *width as usize)
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
