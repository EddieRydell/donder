//! Runtime-private execution of language-admitted programs.
use super::{BoundParams, LaneContext, RunContext, SpatialContext, VmWorkspace};
use crate::values::Color;
use donder_language::dsl::SampleProgram;

pub(crate) trait SampleProgramExt: Sized {
    fn sample_block(
        &self,
        params: &BoundParams,
        contexts: &[LaneContext<'_>],
        workspace: &mut VmWorkspace,
        output: &mut [Color],
        reuse_uniform: bool,
    );
    #[allow(clippy::too_many_arguments)]
    fn sample(
        &self,
        params: &BoundParams,
        context: &RunContext,
        spatial: &SpatialContext,
        sections: crate::sections::SectionContext<'_>,
        workspace: &mut VmWorkspace,
        reuse_uniform: bool,
    ) -> Color;
}

impl SampleProgramExt for SampleProgram {
    fn sample_block(
        &self,
        params: &BoundParams,
        contexts: &[LaneContext<'_>],
        workspace: &mut VmWorkspace,
        output: &mut [Color],
        reuse_uniform: bool,
    ) {
        assert!(self.supports_numeric_blocks());
        super::vm::evaluate_numeric_block(
            self.bytecode(),
            params,
            &contexts[0].context,
            &contexts[0].spatial,
            Some(contexts),
            &mut super::vm::NoSignals,
            workspace,
            output,
            reuse_uniform,
            self.target_entry(),
        );
    }
    fn sample(
        &self,
        params: &BoundParams,
        context: &RunContext,
        spatial: &SpatialContext,
        sections: crate::sections::SectionContext<'_>,
        workspace: &mut VmWorkspace,
        reuse_uniform: bool,
    ) -> Color {
        let entry = workspace.sample_entry(
            reuse_uniform,
            self.target_entry(),
            self.bytecode().pixel_entry as usize,
            context,
            spatial,
        );
        super::vm::evaluate_sample(
            self.bytecode(),
            params,
            context,
            spatial,
            sections,
            workspace,
            entry,
        )
    }
}
