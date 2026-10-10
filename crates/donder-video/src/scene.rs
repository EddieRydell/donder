use std::collections::HashMap;

use donder_model::{DonderProject, PreparedFixtureDefinitions};
use donder_runtime::SequenceFrame;

use crate::{VideoError, VideoOptions};

/// Antialiasing width of a bulb's edge, in pixels.
const EDGE_PIXELS: f32 = 1.0;

/// A pixel's front-view position and radius, in meters.
type WorldPixel = ([f32; 2], f32);

/// One drawn pixel: its screen centre and radius.
struct Bulb {
    x: f32,
    y: f32,
    radius: f32,
}

/// The layout seen from the front, fitted to the frame as the Preview fits it.
pub(crate) struct Scene {
    width: usize,
    height: usize,
    /// Bulbs of each fixture instance, in output pixel order, by fixture id.
    fixtures: HashMap<u32, Vec<Bulb>>,
    /// The Preview draws on an sRGB surface, which encodes every colour
    /// channel as linear; the video applies the same encoding.
    srgb: [u8; 256],
    /// The background with every bulb drawn unlit.
    unlit: Vec<u8>,
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
                        ([point.x, point.y], pixel.diameter_meters / 2.0)
                    })
                    .collect();
                (fixture.id.0, pixels)
            })
            .collect();
        let (mut min, mut max) = ([f32::MAX; 2], [f32::MIN; 2]);
        for (point, radius) in world.iter().flat_map(|(_, pixels)| pixels) {
            for axis in 0..2 {
                min[axis] = min[axis].min(point[axis] - radius);
                max[axis] = max[axis].max(point[axis] + radius);
            }
        }
        if min[0] > max[0] {
            return Err(VideoError::EmptyLayout);
        }
        let (width, height) = (options.width as f32, options.height as f32);
        let span = [
            (max[0] - min[0]).max(f32::EPSILON),
            (max[1] - min[1]).max(f32::EPSILON),
        ];
        let scale = options.canvas_fill_ratio * (width / span[0]).min(height / span[1]);
        let origin = [
            (width - span[0] * scale) / 2.0 - min[0] * scale,
            (height + span[1] * scale) / 2.0 + min[1] * scale,
        ];
        let fixtures = world
            .into_iter()
            .map(|(id, pixels)| {
                let bulbs = pixels
                    .into_iter()
                    .map(|(point, radius)| Bulb {
                        x: origin[0] + point[0] * scale,
                        y: origin[1] - point[1] * scale,
                        radius: (radius * scale).max(options.minimum_radius_pixels),
                    })
                    .collect();
                (id, bulbs)
            })
            .collect::<HashMap<_, _>>();
        let srgb = std::array::from_fn(|value| linear_to_srgb(value as u8));
        let background = options
            .background_rgb
            .map(|channel| srgb[usize::from(channel)]);
        let mut scene = Self {
            width: options.width as usize,
            height: options.height as usize,
            fixtures: HashMap::new(),
            srgb,
            unlit: background
                .iter()
                .copied()
                .cycle()
                .take(options.width as usize * options.height as usize * 3)
                .collect(),
        };
        let mut unlit = std::mem::take(&mut scene.unlit);
        let unlit_rgb = options.unlit_rgb.map(|channel| srgb[usize::from(channel)]);
        for bulb in fixtures.values().flatten() {
            scene.disc(&mut unlit, bulb, unlit_rgb);
        }
        scene.unlit = unlit;
        scene.fixtures = fixtures;
        Ok(scene)
    }

    /// Draw one evaluated frame into `rgb`: unlit bulbs, then each lit pixel in its colour.
    pub(crate) fn draw(&self, frame: &SequenceFrame<'_>, rgb: &mut [u8]) {
        rgb.copy_from_slice(&self.unlit);
        for fixture in frame.fixtures() {
            let Some(bulbs) = self.fixtures.get(&fixture.fixture_id) else {
                continue;
            };
            for (color, bulb) in fixture.pixels.iter().zip(bulbs) {
                if color.red != 0 || color.green != 0 || color.blue != 0 {
                    let encoded = [color.red, color.green, color.blue]
                        .map(|channel| self.srgb[usize::from(channel)]);
                    self.disc(rgb, bulb, encoded);
                }
            }
        }
    }

    /// An antialiased filled circle.
    fn disc(&self, rgb: &mut [u8], bulb: &Bulb, color: [u8; 3]) {
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
                for channel in 0..3 {
                    let under = f32::from(rgb[at + channel]);
                    let over = f32::from(color[channel]);
                    rgb[at + channel] = (under + (over - under) * coverage).round() as u8;
                }
            }
        }
    }
}

/// The sRGB transfer function, as a GPU applies it when writing a linear value
/// to an sRGB surface.
fn linear_to_srgb(value: u8) -> u8 {
    let linear = f32::from(value) / 255.0;
    let encoded = if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round() as u8
}
