use crate::values::Color;
use alloc::boxed::Box;
use core::ops::Range;

#[cfg(test)]
mod tests;

pub(crate) use donder_language::execution::PixelEncoding;

#[derive(Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct PreparedPixelRoute {
    pub pixels: Range<usize>,
    pub frame: usize,
    pub start_slot: usize,
    pub encoding: PixelEncoding,
    pub lookup: Option<usize>,
}

#[derive(Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct PreparedPatch {
    pub routes: Box<[PreparedPixelRoute]>,
    pub lookups: Box<[[u8; 256]]>,
}

impl PreparedPatch {
    /// Only admitted sequences invoke packing, with their own fixed-size buffers.
    pub(crate) fn evaluate(&self, colors: &[Color], frames: &mut [impl AsMut<[u8]>]) {
        for frame in frames.iter_mut() {
            frame.as_mut().fill(0);
        }
        for route in &self.routes {
            let colors = &colors[route.pixels.clone()];
            let width = colors.len() * route.encoding.channel_order().len();
            let frame = frames[route.frame].as_mut();
            let start = route.start_slot;
            let output = &mut frame[start..start + width];
            let lookup = route.lookup.map(|index| &self.lookups[index]);
            match route.encoding {
                PixelEncoding::Rgb { order } => {
                    pack(colors, output, order, lookup, |c| [c.red, c.green, c.blue])
                }
                PixelEncoding::Rgbw { order } => pack(colors, output, order, lookup, |c| {
                    let white = c.red.min(c.green).min(c.blue);
                    [c.red - white, c.green - white, c.blue - white, white]
                }),
            }
        }
    }
}

fn pack<const N: usize>(
    colors: &[Color],
    output: &mut [u8],
    order: [u8; N],
    lookup: Option<&[u8; 256]>,
    channels: impl Fn(Color) -> [u8; N],
) {
    if let Some(lookup) = lookup {
        for (color, output) in colors.iter().zip(output.as_chunks_mut::<N>().0) {
            let channels = channels(*color);
            output.copy_from_slice(
                &order.map(|index| lookup[usize::from(channels[usize::from(index)])]),
            );
        }
    } else {
        for (color, output) in colors.iter().zip(output.as_chunks_mut::<N>().0) {
            let channels = channels(*color);
            output.copy_from_slice(&order.map(|index| channels[usize::from(index)]));
        }
    }
}
