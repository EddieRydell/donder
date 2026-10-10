use std::collections::HashMap;

use donder_runtime::SequenceFrame;
use donder_runtime_types::Color;

use crate::VideoOptions;
use crate::view::{FrontView, ViewBounds, ViewCamera};

/// Antialiasing width of a bulb's edge, in pixels.
const EDGE_PIXELS: f32 = 1.0;
/// Steps of the linear-to-sRGB table used for blended edge pixels.
const ENCODE_STEPS: usize = 4096;

/// One drawn pixel: its screen centre and radius.
struct Bulb {
    x: f32,
    y: f32,
    radius: f32,
}

/// The front view rasterized as the Preview draws it, fitted by `ViewCamera`:
/// every pixel in layout order, unlit pixels in the unlit colour, colours
/// treated as linear and written through the sRGB transfer function.
pub(crate) struct Scene {
    width: usize,
    height: usize,
    /// Each fixture instance's bulbs in output pixel order, in layout order.
    fixtures: Vec<(u32, Vec<Bulb>)>,
    unlit: Color,
    /// The background, already sRGB-encoded, one RGB triple per pixel.
    background: Vec<u8>,
    /// sRGB encoding of each 8-bit linear channel value.
    encode8: [u8; 256],
    /// sRGB decoding of each 8-bit encoded value back to linear.
    decode8: [f32; 256],
    /// sRGB encoding of a linear value in `0..=1`, in `ENCODE_STEPS` steps.
    encode: Vec<u8>,
}

impl Scene {
    pub(crate) fn new(view: &FrontView, options: &VideoOptions) -> Self {
        let (width, height) = (options.width, options.height);
        let camera = ViewCamera::fit(
            ViewBounds::from_points(view.pixels().map(|pixel| pixel.position)),
            width,
            height,
            options.style.canvas_fill_ratio,
        );
        let fixtures = view
            .fixtures
            .iter()
            .map(|fixture| {
                let bulbs = fixture
                    .pixels
                    .iter()
                    .map(|pixel| {
                        let center = camera.to_screen(pixel.position, width, height);
                        Bulb {
                            x: center.x,
                            y: center.y,
                            radius: camera
                                .radius(pixel.radius, options.style.minimum_radius_pixels),
                        }
                    })
                    .collect();
                (fixture.id, bulbs)
            })
            .collect();
        let encode8 = std::array::from_fn(|value| encode_srgb(value as f32 / 255.0));
        let decode8 = std::array::from_fn(|value| decode_srgb(value as f32 / 255.0));
        let encode = (0..ENCODE_STEPS)
            .map(|step| encode_srgb(step as f32 / (ENCODE_STEPS - 1) as f32))
            .collect();
        let background = options
            .background_rgb
            .map(|channel| encode8[usize::from(channel)]);
        let [red, green, blue] = options.unlit_rgb;
        Self {
            width: options.width as usize,
            height: options.height as usize,
            fixtures,
            unlit: Color { red, green, blue },
            background: background
                .iter()
                .copied()
                .cycle()
                .take(options.width as usize * options.height as usize * 3)
                .collect(),
            encode8,
            decode8,
            encode,
        }
    }

    /// Draw one evaluated frame into `rgb`.
    pub(crate) fn draw(&self, frame: &SequenceFrame<'_>, rgb: &mut [u8]) {
        rgb.copy_from_slice(&self.background);
        let colors: HashMap<u32, &[Color]> = frame
            .fixtures()
            .map(|fixture| (fixture.fixture_id, fixture.pixels))
            .collect();
        for (id, bulbs) in &self.fixtures {
            let pixels = colors.get(id).copied().unwrap_or_default();
            for (index, bulb) in bulbs.iter().enumerate() {
                let color = pixels
                    .get(index)
                    .copied()
                    .filter(|color| color.red != 0 || color.green != 0 || color.blue != 0)
                    .unwrap_or(self.unlit);
                self.disc(rgb, bulb, color);
            }
        }
    }

    /// An antialiased filled circle, blended in linear light.
    fn disc(&self, rgb: &mut [u8], bulb: &Bulb, color: Color) {
        let linear = [color.red, color.green, color.blue].map(|channel| f32::from(channel) / 255.0);
        let solid =
            [color.red, color.green, color.blue].map(|channel| self.encode8[usize::from(channel)]);
        let reach = bulb.radius + EDGE_PIXELS;
        let left = (bulb.x - reach).floor().max(0.0) as usize;
        let top = (bulb.y - reach).floor().max(0.0) as usize;
        let right = ((bulb.x + reach).ceil().max(0.0) as usize).min(self.width);
        let bottom = ((bulb.y + reach).ceil().max(0.0) as usize).min(self.height);
        for y in top..bottom {
            for x in left..right {
                let dx = x as f32 + 0.5 - bulb.x;
                let dy = y as f32 + 0.5 - bulb.y;
                let distance = (dx * dx + dy * dy).sqrt();
                let coverage = (bulb.radius + EDGE_PIXELS / 2.0 - distance).clamp(0.0, 1.0);
                if coverage <= 0.0 {
                    continue;
                }
                let at = (y * self.width + x) * 3;
                if coverage >= 1.0 {
                    rgb[at..at + 3].copy_from_slice(&solid);
                    continue;
                }
                for channel in 0..3 {
                    let under = self.decode8[usize::from(rgb[at + channel])];
                    let blended = under + (linear[channel] - under) * coverage;
                    let step = (blended * (ENCODE_STEPS - 1) as f32).round() as usize;
                    rgb[at + channel] = self.encode[step.min(ENCODE_STEPS - 1)];
                }
            }
        }
    }
}

/// The sRGB transfer function, as a GPU applies it when writing a linear value
/// to an sRGB surface.
fn encode_srgb(linear: f32) -> u8 {
    let encoded = if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round() as u8
}

fn decode_srgb(encoded: f32) -> f32 {
    if encoded <= 0.040_45 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}
