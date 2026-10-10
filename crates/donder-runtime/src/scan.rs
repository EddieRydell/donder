//! Scans: trails along each fixture's pixels, computed once per query over
//! the whole frame. A scan runs once per frame rather than once per strip, so
//! it stays in flash, outside the interpreter's instruction RAM.
use crate::dsl::bytecode::Direction;
use crate::dsl::{STRIP, ScanQuery, SourceWeights};
use crate::targets::TargetPixels;
use donder_runtime_types::Color;
use donder_runtime_types::sampling::byte_channel;

/// A scan's input colors, a strip at a time.
pub(crate) trait ScanColors {
    /// The input's colors from plan-target pixel `start`.
    fn colors(&mut self, query: &ScanQuery, start: usize, colors: &mut [Color]);
}

/// Scan `pixels` into `output`. A missing (NaN) decay or weight is zero, as
/// scaling a color by NaN is black.
#[inline(never)]
pub(crate) fn scan_frame(
    pixels: &TargetPixels,
    query: &ScanQuery,
    weights: &mut dyn SourceWeights,
    input: &mut dyn ScanColors,
    output: &mut [Color],
) {
    let decay = if query.decay.is_nan() {
        0.0
    } else {
        query.decay
    };
    let forward = query.direction == Direction::Forward;
    let chunks = output.len().div_ceil(STRIP);
    let mut running = [0.0f32; 3];
    let mut colors = [Color::BLACK; STRIP];
    let mut factors = [0.0f32; STRIP];
    // Each pixel's index in its fixture, and the fixture's pixel count.
    let mut places = [(0usize, 0usize); STRIP];
    for step in 0..chunks {
        let chunk = if forward { step } else { chunks - 1 - step };
        let start = chunk * STRIP;
        let size = (output.len() - start).min(STRIP);
        input.colors(query, start, &mut colors[..size]);
        weights.weights(&colors[..size], &mut factors[..size]);
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
            let weight = if factors[at].is_nan() {
                0.0
            } else {
                factors[at]
            };
            let Color { red, green, blue } = colors[at];
            for (value, channel) in running.iter_mut().zip([red, green, blue]) {
                *value = *value * decay + f32::from(channel) * weight;
            }
            output[start + at] = Color {
                red: byte_channel(running[0]),
                green: byte_channel(running[1]),
                blue: byte_channel(running[2]),
            };
        }
    }
}
