//! Runtime-private execution of language-admitted programs.
use super::{BoundParams, LaneContext, RunContext, SignalSampler, SpatialContext, VmWorkspace};
use crate::values::Color;
use donder_language::dsl::OperatorProgram;

pub(crate) trait OperatorProgramExt: Sized {
    #[allow(clippy::too_many_arguments)]
    fn sample_numeric_block(
        &self,
        params: &BoundParams,
        contexts: &[LaneContext<'_>],
        sampler: &mut dyn SignalSampler<core::convert::Infallible>,
        workspace: &mut VmWorkspace,
        output: &mut [Color],
        reuse_uniform: bool,
    );
    #[allow(clippy::too_many_arguments)]
    fn sample_block(
        &self,
        params: &BoundParams,
        context: &RunContext,
        spatial: &SpatialContext,
        sampler: &mut dyn SignalSampler<core::convert::Infallible>,
        workspace: &mut VmWorkspace,
        output: &mut [Color],
        reuse_uniform: bool,
    );
    #[allow(clippy::too_many_arguments)]
    fn sample<E>(
        &self,
        params: &BoundParams,
        context: &RunContext,
        spatial: &SpatialContext,
        sections: crate::sections::SectionContext<'_>,
        sampler: &mut dyn SignalSampler<E>,
        workspace: &mut VmWorkspace,
        reuse_uniform: bool,
    ) -> Result<Color, E>;
}

impl OperatorProgramExt for OperatorProgram {
    fn sample_numeric_block(
        &self,
        params: &BoundParams,
        contexts: &[LaneContext<'_>],
        sampler: &mut dyn SignalSampler<core::convert::Infallible>,
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
            sampler,
            workspace,
            output,
            reuse_uniform,
            self.target_entry(),
        );
    }
    fn sample_block(
        &self,
        params: &BoundParams,
        context: &RunContext,
        spatial: &SpatialContext,
        sampler: &mut dyn SignalSampler<core::convert::Infallible>,
        workspace: &mut VmWorkspace,
        output: &mut [Color],
        reuse_uniform: bool,
    ) {
        assert!(self.supports_color_blocks());
        super::vm::evaluate_operator_block(
            self.bytecode(),
            params,
            context,
            spatial,
            sampler,
            workspace,
            output,
            reuse_uniform,
        );
    }
    fn sample<E>(
        &self,
        params: &BoundParams,
        context: &RunContext,
        spatial: &SpatialContext,
        sections: crate::sections::SectionContext<'_>,
        sampler: &mut dyn SignalSampler<E>,
        workspace: &mut VmWorkspace,
        reuse_uniform: bool,
    ) -> Result<Color, E> {
        let entry = workspace.sample_entry(
            reuse_uniform,
            self.target_entry(),
            self.bytecode().pixel_entry as usize,
            context,
            spatial,
        );
        super::vm::evaluate_operator(
            self.bytecode(),
            params,
            context,
            spatial,
            sections,
            sampler,
            workspace,
            entry,
        )
    }
}
