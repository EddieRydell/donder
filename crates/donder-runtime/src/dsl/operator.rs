//! Runtime-private execution of language-admitted programs.
use super::{BoundParams, RunContext, SignalSampler, SpatialContext, VmWorkspace};
use crate::values::Color;
use donder_language::dsl::OperatorProgram;

pub(crate) trait OperatorProgramExt: Sized {
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
        super::vm::evaluate_operator(
            self.bytecode(),
            params,
            context,
            spatial,
            sections,
            sampler,
            workspace,
            if reuse_uniform {
                self.bytecode().pixel_entry as usize
            } else {
                0
            },
        )
    }
}
