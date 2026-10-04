//! One-pixel evaluation of an admitted effect, for tests.
use super::{BatchWorkspace, BoundParams, NoSignals, RunContext, SampleProgram, SpatialContext};
use crate::values::Color;

/// Sample one pixel as a one-lane batch. Section queries use the pixel's index
/// and count.
pub(crate) fn sample_once(
    program: &SampleProgram,
    params: &BoundParams,
    context: &RunContext,
    spatial: &SpatialContext,
    workspace: &mut BatchWorkspace,
) -> Color {
    workspace.reserve(program.bytecode(), program.batch());
    let mut batch = super::Batch::new(
        program.bytecode(),
        program.target_entry(),
        program.batch(),
        params,
        context,
        None,
        workspace,
    );
    let lanes = batch.lanes();
    lanes.pixel_index[0] = context.pixel_index;
    lanes.pixel_fraction[0] = context.pixel_fraction;
    lanes.x[0] = spatial.position[0];
    lanes.y[0] = spatial.position[1];
    let mut color = [Color::BLACK];
    batch.run(
        context.pixel_count as usize,
        spatial.min,
        spatial.max,
        &mut NoSignals,
        &mut color,
    );
    color[0]
}
