//! Private execution adapters shared by compiler/VM behavior tests. Programs
//! run as one-lane batches.
use crate::dsl::bytecode::SignalPixel;
use crate::dsl::{
    BATCH_LANES, Batch, BatchMask, BatchSignals, BatchWorkspace, BoundParams, DslBindCache,
    RunContext, RuntimeError, SpatialContext,
};
use alloc::vec::Vec;
use donder_language::dsl::{
    BindingError, OperatorInvocation, SampleDefinition, SampleInvocation, SampleProgram, Value,
};
use donder_language::values::{Color, SampleDuration, SampleTime};

/// A test signal source. An error is reported as the operator's result.
pub(super) trait SignalSampler {
    fn sample_signal(
        &mut self,
        input: usize,
        sample_time: SampleTime,
        pixel: SignalPixel<i32>,
        frame_cache: Option<usize>,
    ) -> Result<Color, RuntimeError>;
}

/// Lane zero of a one-lane run; the first error wins.
struct Adapter<'a> {
    sampler: &'a mut dyn SignalSampler,
    error: Option<RuntimeError>,
}

impl Adapter<'_> {
    fn sample(
        &mut self,
        input: usize,
        time: SampleTime,
        pixel: SignalPixel<i32>,
        frame_cache: Option<usize>,
    ) -> Color {
        match self.sampler.sample_signal(input, time, pixel, frame_cache) {
            Ok(color) => color,
            Err(error) => {
                self.error.get_or_insert(error);
                Color::BLACK
            }
        }
    }
}

impl BatchSignals for Adapter<'_> {
    fn sample_run(
        &mut self,
        input: usize,
        time: SampleTime,
        frame_cache: Option<usize>,
        _: BatchMask,
        output: &mut [Color; BATCH_LANES],
    ) {
        output[0] = self.sample(input, time, SignalPixel::Current, frame_cache);
    }

    fn sample_pixel(
        &mut self,
        input: usize,
        time: SampleTime,
        _: usize,
        pixel: SignalPixel<i32>,
        frame_cache: Option<usize>,
    ) -> Color {
        self.sample(input, time, pixel, frame_cache)
    }
}

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
        workspace: &mut BatchWorkspace,
    ) -> Color;
}

impl SampleEvaluation for SampleInvocation {
    fn evaluate(
        &self,
        context: &RunContext,
        spatial: &SpatialContext,
        workspace: &mut BatchWorkspace,
    ) -> Color {
        let params = BoundParams::from_validated(self.params(), &mut DslBindCache::default());
        crate::dsl::sample_once(self.program(), &params, context, spatial, workspace)
    }
}

pub(super) trait OperatorEvaluation {
    fn evaluate(
        &self,
        context: &RunContext,
        spatial: &SpatialContext,
        sampler: &mut dyn SignalSampler,
        workspace: &mut BatchWorkspace,
    ) -> Result<Color, RuntimeError>;
}

impl OperatorEvaluation for OperatorInvocation {
    fn evaluate(
        &self,
        context: &RunContext,
        spatial: &SpatialContext,
        sampler: &mut dyn SignalSampler,
        workspace: &mut BatchWorkspace,
    ) -> Result<Color, RuntimeError> {
        let params = BoundParams::from_validated(self.params(), &mut DslBindCache::default());
        let program = self.program();
        workspace.reserve(program.bytecode(), program.batch());
        let mut batch = Batch::new(
            program.bytecode(),
            program.target_entry(),
            program.batch(),
            &params,
            context,
            None,
            workspace,
        );
        let lanes = batch.lanes();
        lanes.pixel_index[0] = context.pixel_index;
        lanes.pixel_fraction[0] = context.pixel_fraction;
        lanes.x[0] = spatial.position[0];
        lanes.y[0] = spatial.position[1];
        let mut signals = Adapter {
            sampler,
            error: None,
        };
        let mut color = [Color::BLACK];
        batch.run(
            context.pixel_count as usize,
            spatial.min,
            spatial.max,
            &mut signals,
            &mut color,
        );
        match signals.error {
            Some(error) => Err(error),
            None => Ok(color[0]),
        }
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
