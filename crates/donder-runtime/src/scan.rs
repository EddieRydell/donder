//! Scans: trails along each fixture's pixels, computed once per frame over a
//! whole input frame. A scan runs once per frame rather than once per strip,
//! so it stays in flash, outside the interpreter's instruction RAM.
use crate::dsl::bytecode::Direction;
use crate::dsl::{STRIP, ScanQuery, SourceWeights};
use crate::targets::TargetView;
use donder_runtime_types::Color;

/// Scan the plan-target frame `input` into `output`.
#[inline(never)]
pub(crate) fn scan_frame(
    pixels: TargetView<'_>,
    query: &ScanQuery,
    weights: &mut dyn SourceWeights,
    input: &[Color],
    output: &mut [Color],
) {
    let decay = query.decay;
    let forward = query.direction == Direction::Forward;
    let len = pixels.len();
    let chunks = len.div_ceil(STRIP);
    let mut running = [0.0f32; 3];
    let mut factors = [0.0f32; STRIP];
    // Each pixel's index in its fixture, and the fixture's pixel count.
    let mut places = [(0usize, 0usize); STRIP];
    for step in 0..chunks {
        let chunk = if forward { step } else { chunks - 1 - step };
        let start = chunk * STRIP;
        let size = (len - start).min(STRIP);
        let colors = &input[start..start + size];
        weights.weights(colors, &mut factors[..size]);
        for (place, pixel) in places[..size].iter_mut().zip(pixels.iter_from(start)) {
            *place = (pixel.pixel_index, pixel.pixel_count);
        }
        for step in 0..size {
            let at = if forward { step } else { size - 1 - step };
            let (index, count) = places[at];
            let first = if forward {
                index == 0
            } else {
                index + 1 == count
            };
            if first {
                running = [0.0; 3];
            }
            let weight = factors[at];
            let Color { red, green, blue } = colors[at];
            for (value, channel) in running.iter_mut().zip([red, green, blue]) {
                *value = *value * decay + channel * weight;
            }
            let [red, green, blue] = running;
            output[start + at] = Color { red, green, blue };
        }
    }
}
