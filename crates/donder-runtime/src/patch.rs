//! Packing a frame's colors into its outputs' bytes. Each channel, clamped to
//! 0..1 with NaN off, becomes a level in 1/256 of a byte, through the route's
//! gamma and brightness table when it has one, and then a byte: the nearest,
//! or on a dithered route the one a threshold cycling over eight frames picks,
//! so the bytes average to the exact level.
use crate::format::{self, NONE};
use donder_runtime_types::Color;
use donder_runtime_types::sampling::min_float;

use crate::view::Show;

/// A byte's fraction that rounds up in each frame of the dither cycle: the
/// middles of the eighths in bit-reversed order, in 1/256 of a byte.
const DITHER: [u32; 8] = [16, 144, 80, 208, 48, 176, 112, 240];
/// The cycle's step from one pixel to the next, odd so neighbors differ.
const DITHER_PIXEL_STEP: u32 = 5;
const NEAREST: u32 = 128;

/// Pack `colors` into every output's bytes for `frame`; bytes no route writes
/// are zero.
pub(crate) fn evaluate(show: Show<'_>, colors: &[Color], outputs: &mut [u8], frame: u32) {
    outputs.fill(0);
    let s = &show.s;
    for route in s.routes {
        let first = route.first;
        let colors = &colors[first as usize..route.end as usize];
        let rgbw = route.encoding == format::ENCODING_RGBW;
        let channels = if rgbw { 4 } else { 3 };
        let output = &s.outputs[route.output as usize];
        let start = (output.offset + route.start_slot) as usize;
        let bytes = &mut outputs[start..start + colors.len() * channels];
        let lookup = (route.lookup != NONE).then(|| &s.lookups[route.lookup as usize].table);
        let threshold = |index: usize| {
            if route.dither == 0 {
                NEAREST
            } else {
                let pixel = first.wrapping_add(index as u32);
                DITHER[(frame.wrapping_add(pixel.wrapping_mul(DITHER_PIXEL_STEP)) % 8) as usize]
            }
        };
        let order = route.order;
        if rgbw {
            pack(
                colors,
                bytes,
                [order[0], order[1], order[2], order[3]],
                lookup,
                threshold,
                |c| {
                    let white = min_float(min_float(c.red, c.green), c.blue);
                    [c.red - white, c.green - white, c.blue - white, white]
                },
            );
        } else {
            pack(
                colors,
                bytes,
                [order[0], order[1], order[2]],
                lookup,
                threshold,
                |c| [c.red, c.green, c.blue],
            );
        }
    }
}

/// A channel's level in 1/256 of a byte, read between the table's entries.
#[inline(always)]
fn level(channel: f32, lookup: Option<&[u16; 256]>) -> u32 {
    let input = channel.clamp(0.0, 1.0) * 255.0;
    match lookup {
        None => (input * 256.0) as u32,
        Some(table) => {
            let index = (input as usize).min(254);
            let low = f32::from(table[index]);
            let high = f32::from(table[index + 1]);
            (low + (high - low) * (input - index as f32)) as u32
        }
    }
}

fn pack<const N: usize>(
    colors: &[Color],
    output: &mut [u8],
    order: [u8; N],
    lookup: Option<&[u16; 256]>,
    threshold: impl Fn(usize) -> u32,
    channels: impl Fn(Color) -> [f32; N],
) {
    for (index, (color, output)) in colors.iter().zip(output.as_chunks_mut::<N>().0).enumerate() {
        let channels = channels(*color);
        let threshold = threshold(index);
        output.copy_from_slice(&order.map(|channel| {
            ((level(channels[usize::from(channel)], lookup) + threshold) >> 8) as u8
        }));
    }
}
