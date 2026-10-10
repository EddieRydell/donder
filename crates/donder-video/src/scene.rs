use std::collections::HashMap;

use donder_model::{DonderProject, PreparedFixtureDefinitions};
use donder_runtime::SequenceFrame;
use donder_runtime_types::Color;

use crate::{VideoError, VideoOptions};

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

/// The layout seen from the front, fitted and drawn as the Preview draws it:
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
    pub(crate) fn new(project: &DonderProject, options: &VideoOptions) -> Result<Self, VideoError> {
        let setup = project
            .setup(project.root().setup.id())
            .ok_or(VideoError::MissingSetup)?;
        let layout = project
            .layout(setup.layout.id())
            .ok_or(VideoError::MissingSetup)?;
        let definitions = PreparedFixtureDefinitions::prepare(&project.definitions().fixtures);
        let layout = definitions.prepare_layout(layout);
        let world: Vec<(u32, Vec<WorldPixel>)> = layout
            .instances
            .iter()
            .map(|fixture| {
                let pixels = fixture
                    .pixels
                    .iter()
                    .map(|pixel| {
                        let point = fixture.transform.transform_point3(pixel.position);
                        WorldPixel {
                            x: point.x,
                            y: point.y,
                            radius: pixel.diameter_meters / 2.0,
                        }
                    })
                    .collect();
                (fixture.id.0, pixels)
            })
            .collect();
        let camera = Camera::fit(&world, options)?;
        let fixtures = world
            .into_iter()
            .map(|(id, pixels)| {
                let bulbs = pixels.iter().map(|pixel| camera.bulb(pixel)).collect();
                (id, bulbs)
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
        Ok(Self {
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
        })
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

/// A pixel's front-view position and radius, in meters.
struct WorldPixel {
    x: f32,
    y: f32,
    radius: f32,
}

/// The Preview's camera (`PreviewCamera::fit`): pixel centres bound the
/// layout, the span is at least a meter, and the zoom at least one pixel
/// per meter, centred in the frame.
struct Camera {
    pan: [f32; 2],
    zoom: f32,
    screen: [f32; 2],
    minimum_radius: f32,
}

impl Camera {
    fn fit(world: &[(u32, Vec<WorldPixel>)], options: &VideoOptions) -> Result<Self, VideoError> {
        let mut pixels = world.iter().flat_map(|(_, pixels)| pixels);
        let first = pixels.next().ok_or(VideoError::EmptyLayout)?;
        let (mut min, mut max) = ([first.x, first.y], [first.x, first.y]);
        for pixel in pixels {
            min = [min[0].min(pixel.x), min[1].min(pixel.y)];
            max = [max[0].max(pixel.x), max[1].max(pixel.y)];
        }
        let span = [(max[0] - min[0]).max(1.0), (max[1] - min[1]).max(1.0)];
        let screen = [options.width as f32, options.height as f32];
        let zoom = (screen[0] * options.canvas_fill_ratio / span[0])
            .min(screen[1] * options.canvas_fill_ratio / span[1])
            .max(1.0);
        Ok(Self {
            pan: [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5],
            zoom,
            screen,
            minimum_radius: options.minimum_radius_pixels,
        })
    }

    fn bulb(&self, pixel: &WorldPixel) -> Bulb {
        Bulb {
            x: self.screen[0] * 0.5 + (pixel.x - self.pan[0]) * self.zoom,
            y: self.screen[1] * 0.5 - (pixel.y - self.pan[1]) * self.zoom,
            radius: (pixel.radius * self.zoom).max(self.minimum_radius),
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
