//! Runtime-private execution of language-admitted programs.
use super::{BoundParams, RunContext, SpatialContext, VmWorkspace};
use crate::values::Color;
use donder_language::dsl::SampleProgram;

pub(crate) trait SampleProgramExt: Sized {
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
    fn sample(
        &self,
        params: &BoundParams,
        context: &RunContext,
        spatial: &SpatialContext,
        sections: crate::sections::SectionContext<'_>,
        workspace: &mut VmWorkspace,
        reuse_uniform: bool,
    ) -> Color {
        super::vm::evaluate_sample(
            self.bytecode(),
            params,
            context,
            spatial,
            sections,
            workspace,
            if reuse_uniform {
                self.bytecode().pixel_entry as usize
            } else {
                0
            },
        )
    }
}
