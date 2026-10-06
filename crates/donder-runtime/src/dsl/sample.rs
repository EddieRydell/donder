//! One-pixel evaluation of an admitted effect, for tests.
use super::{BoundParams, NoSignals, RunContext, SampleProgram, SpatialContext, StripWorkspace};
use crate::values::Color;

/// Sample one pixel, its index and fraction, as a one-pixel strip. Section
/// queries use the pixel's index and count.
pub(crate) fn sample_once(
    program: &SampleProgram,
    params: &BoundParams,
    context: &RunContext,
    (index, fraction): (i32, f32),
    spatial: &SpatialContext,
    workspace: &mut StripWorkspace,
) -> Color {
    workspace.reserve(program.bytecode());
    let mut strip = super::Strip::new(program.bytecode(), params, context, None, workspace);
    let pixels = strip.pixels();
    pixels.index[0].set(index);
    pixels.fraction[0].set(fraction);
    pixels.x[0].set(spatial.position[0]);
    pixels.y[0].set(spatial.position[1]);
    let mut color = [Color::BLACK];
    strip.run(
        context.pixel_count as usize,
        spatial.min,
        spatial.max,
        &mut NoSignals,
        &mut color,
    );
    color[0]
}
