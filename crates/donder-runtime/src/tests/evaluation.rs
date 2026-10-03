//! Private execution adapters shared by existing compiler/VM behavior tests.
use crate::dsl::{
    BoundParams, DslBindCache, OperatorProgramExt, RunContext, RuntimeError, SampleProgramExt,
    SignalSampler, SpatialContext, VmWorkspace,
};
use crate::sections::SectionContext;
use alloc::vec::Vec;
use donder_language::dsl::{
    BindingError, OperatorInvocation, SampleDefinition, SampleInvocation, SampleProgram, Value,
};
use donder_language::values::{Color, SampleDuration};

pub(super) trait BindForTest {
    fn bind_for_test(&self, values: Vec<Value>) -> Result<SampleInvocation, BindingError>;
}

impl BindForTest for SampleProgram {
    fn bind_for_test(&self, values: Vec<Value>) -> Result<SampleInvocation, BindingError> {
        SampleDefinition::new(self.clone()).bind(values)
    }
}

pub(super) trait SampleEvaluation {
    fn evaluate(
        &self,
        context: &RunContext,
        spatial: &SpatialContext,
        workspace: &mut VmWorkspace,
    ) -> Color;
}

impl SampleEvaluation for SampleInvocation {
    fn evaluate(
        &self,
        context: &RunContext,
        spatial: &SpatialContext,
        workspace: &mut VmWorkspace,
    ) -> Color {
        let params = BoundParams::from_validated(self.params(), &mut DslBindCache::default());
        self.program().sample(
            &params,
            context,
            spatial,
            SectionContext::Single {
                index: context.pixel_index,
                count: context.pixel_count,
            },
            workspace,
            false,
        )
    }
}

pub(super) trait OperatorEvaluation {
    fn evaluate(
        &self,
        context: &RunContext,
        spatial: &SpatialContext,
        sampler: &mut dyn SignalSampler,
        workspace: &mut VmWorkspace,
    ) -> Result<Color, RuntimeError>;
}

impl OperatorEvaluation for OperatorInvocation {
    fn evaluate(
        &self,
        context: &RunContext,
        spatial: &SpatialContext,
        sampler: &mut dyn SignalSampler,
        workspace: &mut VmWorkspace,
    ) -> Result<Color, RuntimeError> {
        let params = BoundParams::from_validated(self.params(), &mut DslBindCache::default());
        self.program().sample(
            &params,
            context,
            spatial,
            SectionContext::Single {
                index: context.pixel_index,
                count: context.pixel_count,
            },
            sampler,
            workspace,
            false,
        )
    }
}

pub(super) fn context(count: usize, pixel: usize, frame: usize) -> RunContext {
    let time = 3_000_000 + frame as u32 * 8_333;
    RunContext {
        progress: time as f32 / 8_000_000.0,
        time: SampleDuration::from_ticks(time),
        duration: SampleDuration::from_ticks(8_000_000),
        pixel_index: pixel as i32,
        pixel_count: count as i32,
        pixel_fraction: pixel as f32 / (count - 1).max(1) as f32,
    }
}
