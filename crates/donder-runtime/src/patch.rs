//! Packing a frame's colors into its outputs' bytes: each channel becomes
//! the nearest byte, through the route's lookup table when it has one. Light
//! below 0 or NaN is off and light above 1 is full, as the cast saturates.
use crate::image::{self, NONE};
use donder_runtime_types::Color;
use donder_runtime_types::sampling::min_float;

use crate::show::Show;

/// Pack `colors` into every output's bytes; bytes no route writes are zero.
pub(crate) fn evaluate(show: Show<'_>, colors: &[Color], outputs: &mut [u8]) {
    outputs.fill(0);
    let s = &show.s;
    for route in s.routes {
        let colors = &colors[route.first as usize..route.end as usize];
        let rgbw = route.encoding == image::ENCODING_RGBW;
        let channels = if rgbw { 4 } else { 3 };
        let output = &s.outputs[route.output as usize];
        let start = (output.offset + route.start_slot) as usize;
        let bytes = &mut outputs[start..start + colors.len() * channels];
        let lookup = (route.lookup != NONE).then(|| &s.lookups[route.lookup as usize].table);
        let order = route.order;
        if rgbw {
            pack(
                colors,
                bytes,
                [order[0], order[1], order[2], order[3]],
                lookup,
                |c| {
                    let white = min_float(min_float(c.red, c.green), c.blue);
                    [c.red - white, c.green - white, c.blue - white, white]
                },
            );
        } else {
            pack(colors, bytes, [order[0], order[1], order[2]], lookup, |c| {
                [c.red, c.green, c.blue]
            });
        }
    }
}

/// A channel as the nearest byte.
#[inline(always)]
fn byte(channel: f32) -> u8 {
    (channel * 255.0 + 0.5) as u8
}

fn pack<const N: usize>(
    colors: &[Color],
    output: &mut [u8],
    order: [u8; N],
    lookup: Option<&[u8; 256]>,
    channels: impl Fn(Color) -> [f32; N],
) {
    if let Some(lookup) = lookup {
        for (color, output) in colors.iter().zip(output.as_chunks_mut::<N>().0) {
            let channels = channels(*color);
            output.copy_from_slice(
                &order.map(|index| lookup[usize::from(byte(channels[usize::from(index)]))]),
            );
        }
    } else {
        for (color, output) in colors.iter().zip(output.as_chunks_mut::<N>().0) {
            let channels = channels(*color);
            output.copy_from_slice(&order.map(|index| byte(channels[usize::from(index)])));
        }
    }
}
