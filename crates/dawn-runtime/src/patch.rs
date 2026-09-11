use crate::values::Color;
use alloc::boxed::Box;
use core::ops::Range;

#[derive(Clone, Copy, Debug, Eq, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum PixelEncoding {
    Rgb { order: [u8; 3] },
    Rgbw { order: [u8; 4] },
}

impl PixelEncoding {
    pub fn channel_order(&self) -> &[u8] {
        match self {
            Self::Rgb { order } => order,
            Self::Rgbw { order } => order,
        }
    }
    pub fn is_valid(&self) -> bool {
        let order = self.channel_order();
        (0..order.len()).all(|channel| {
            order
                .iter()
                .filter(|&&value| usize::from(value) == channel)
                .count()
                == 1
        })
    }
}

#[derive(Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedPixelRoute {
    pub pixels: Range<u32>,
    pub frame: u32,
    pub start_slot: u32,
    pub encoding: PixelEncoding,
    pub lookup: Option<u16>,
}

#[derive(Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedPatch {
    pub routes: Box<[PreparedPixelRoute]>,
    pub lookups: Box<[[u8; 256]]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatchError {
    InvalidRoute,
    WidthMismatch,
}

impl PreparedPatch {
    pub fn evaluate(
        &self,
        colors: &[Color],
        frames: &mut [impl AsMut<[u8]>],
    ) -> Result<(), PatchError> {
        for frame in frames.iter_mut() {
            frame.as_mut().fill(0);
        }
        for route in &self.routes {
            let colors = colors
                .get(route.pixels.start as usize..route.pixels.end as usize)
                .ok_or(PatchError::InvalidRoute)?;
            let width = colors
                .len()
                .checked_mul(route.encoding.channel_order().len())
                .ok_or(PatchError::WidthMismatch)?;
            let frame = frames
                .get_mut(route.frame as usize)
                .ok_or(PatchError::InvalidRoute)?
                .as_mut();
            let start = route.start_slot as usize;
            let output = frame
                .get_mut(start..start.checked_add(width).ok_or(PatchError::WidthMismatch)?)
                .ok_or(PatchError::WidthMismatch)?;
            let lookup = route
                .lookup
                .map(|index| {
                    self.lookups
                        .get(usize::from(index))
                        .ok_or(PatchError::InvalidRoute)
                })
                .transpose()?;
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
        Ok(())
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
