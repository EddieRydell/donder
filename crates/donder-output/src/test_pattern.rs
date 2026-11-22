//! Test patterns that light patched pixels or raw channels on streamed
//! controllers, so wiring can be checked without a sequence.
use core::fmt;
use core::time::Duration;

use donder_model::{
    Controller, ControllerId, ControllerOutput, ControllerPortId, DonderProject, FixtureDefinition,
    FixtureInstanceId, FixtureTarget, Layout, LayoutId, Patch, PatchId, PixelRoute, PixelRouteId,
    PixelSpan, PreparedFixtureDefinitions, Setup,
};
use donder_runtime_types::PixelEncoding;
use indexmap::IndexMap;

use crate::OutputFrame;

/// Pixels a chase moves each second.
const CHASE_PIXELS_PER_SECOND: f32 = 20.0;
/// Pixels lit behind a chase's head, fading.
const CHASE_TAIL: u32 = 5;
/// Seconds a rise takes from the lowest pixel to the highest.
const RISE_SECONDS: f32 = 3.0;
/// A rise's band, as a share of the selection's height.
const RISE_BAND: f32 = 0.08;
/// The dim blue between a count's markers.
const COUNT_BACKGROUND: [u8; 3] = [0, 0, 128];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestPattern {
    /// Every pixel one color.
    Solid([u8; 3]),
    /// The first pixel red, every 10th green, every 50th white, the last
    /// magenta and the rest dim blue: shows count and direction.
    Count,
    /// Each of the first pixels lights one of its channels in wire order, so
    /// their colors show the color order.
    Order,
    /// One pixel with a fading tail runs from first to last.
    Chase,
    /// A band sweeps from the selection's lowest layout pixel to its
    /// highest, whatever the wiring order.
    Rise,
}

/// What a test lights.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestSelection {
    /// Channels of a streamed controller, from 0 across all its universes,
    /// lit as RGB pixels in wire order.
    Channels {
        controller: ControllerId,
        start_channel: u32,
        channel_count: u32,
    },
    /// Every setup route to a layout fixture, or to a group's fixtures.
    LayoutItem {
        layout: LayoutId,
        item: FixtureInstanceId,
    },
    Route {
        patch: PatchId,
        route: PixelRouteId,
    },
    /// Every setup route on one of a controller's ports.
    Port {
        controller: ControllerId,
        port: ControllerPortId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TestError {
    MissingSetup,
    MissingController(ControllerId),
    MissingPatch(PatchId),
    MissingRoute(PixelRouteId),
    NotStreamed(ControllerId),
    ChannelsOutOfRange { channels: u32 },
    PixelCount(String),
    NoStreamedRoutes,
}

impl fmt::Display for TestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSetup => write!(f, "The project's setup is missing."),
            Self::MissingController(_) => write!(f, "A tested controller is missing."),
            Self::MissingPatch(_) => write!(f, "The patch is missing."),
            Self::MissingRoute(_) => write!(f, "The output is missing from the patch."),
            Self::NotStreamed(_) => write!(f, "Only E1.31 and Art-Net controllers stream tests."),
            Self::ChannelsOutOfRange { channels } => {
                write!(
                    f,
                    "Choose channels within the controller's {channels} channels."
                )
            }
            Self::PixelCount(error) => write!(f, "Pixels can't be counted: {error}."),
            Self::NoStreamedRoutes => write!(
                f,
                "No output routes this to an E1.31 or Art-Net controller in the setup."
            ),
        }
    }
}

/// A stretch of a controller's channels and the pixels it carries.
#[derive(Clone, Debug)]
struct Region {
    controller: ControllerId,
    start_channel: u32,
    /// The channel after its last.
    end_channel: u32,
    /// Its first pixel's index in the fixture, and the fixture's pixels.
    first_pixel: u32,
    fixture_pixels: u32,
    pixels: u32,
    encoding: PixelEncoding,
    gamma: f32,
    brightness: f32,
    /// Each pixel's layout height; raw channels rise in channel order.
    heights: Vec<f32>,
    /// Patterns run from the fixture's last pixel to its first.
    reversed: bool,
}

/// A pattern on the channels a selection denotes, ready to render frames.
#[derive(Clone, Debug)]
pub struct OutputTest {
    pattern: TestPattern,
    controllers: IndexMap<ControllerId, Controller>,
    regions: Vec<Region>,
    /// The lowest and highest pixel heights across all regions.
    heights: (f32, f32),
}

fn setup_patch<'a>(project: &'a DonderProject, setup: &Setup) -> Result<&'a Patch, TestError> {
    project
        .get::<Patch>(*setup.patch.id())
        .ok_or(TestError::MissingPatch(*setup.patch.id()))
}

impl OutputTest {
    pub fn new(
        project: &DonderProject,
        selection: TestSelection,
        pattern: TestPattern,
    ) -> Result<Self, TestError> {
        let mut test = Self {
            pattern,
            controllers: IndexMap::new(),
            regions: Vec::new(),
            heights: (0.0, 0.0),
        };
        let setup = project
            .get::<Setup>(*project.root().setup.id())
            .ok_or(TestError::MissingSetup)?;
        // Each route to light, and the fixture pixels a slice limits it to.
        let (patch, routes): (&Patch, Vec<(&PixelRoute, Option<PixelSpan>)>) = match selection {
            TestSelection::Channels {
                controller,
                start_channel,
                channel_count,
            } => {
                let channels = test.add_controller(project, controller)?;
                let end = start_channel
                    .checked_add(channel_count)
                    .filter(|&end| channel_count > 0 && end <= channels)
                    .ok_or(TestError::ChannelsOutOfRange { channels })?;
                let pixels = channel_count.div_ceil(3);
                test.regions.push(Region {
                    controller,
                    start_channel,
                    end_channel: end,
                    first_pixel: 0,
                    fixture_pixels: pixels,
                    pixels,
                    encoding: PixelEncoding::Rgb { order: [0, 1, 2] },
                    gamma: 1.0,
                    brightness: 1.0,
                    heights: (0..pixels).map(|pixel| pixel as f32).collect(),
                    reversed: false,
                });
                test.measure();
                return Ok(test);
            }
            TestSelection::Route { patch, route } => {
                let patch = project
                    .get::<Patch>(patch)
                    .ok_or(TestError::MissingPatch(patch))?;
                let route = patch
                    .routes
                    .iter()
                    .find(|candidate| candidate.id == route)
                    .ok_or(TestError::MissingRoute(route))?;
                (patch, vec![(route, None)])
            }
            TestSelection::LayoutItem { layout, item } => {
                let patch = setup_patch(project, setup)?;
                let parts = project
                    .get::<Layout>(layout)
                    .map_or_else(Vec::new, |value| value.members(item));
                let routes = parts
                    .iter()
                    .flat_map(|part| {
                        patch
                            .routes
                            .iter()
                            .filter(move |route| {
                                route.target.layout == layout
                                    && route.target.fixture == part.fixture
                            })
                            .map(move |route| (route, part.pixels))
                    })
                    .collect();
                (patch, routes)
            }
            TestSelection::Port { controller, port } => {
                let patch = setup_patch(project, setup)?;
                let routes = patch
                    .routes
                    .iter()
                    .filter(|route| route.controller == controller && route.port == port)
                    .map(|route| (route, None))
                    .collect();
                (patch, routes)
            }
        };
        let counts = donder_model::fixture_pixel_counts(project.reusable::<FixtureDefinition>())
            .map_err(|error| TestError::PixelCount(error.to_string()))?;
        let geometry = PreparedFixtureDefinitions::prepare(project.reusable::<FixtureDefinition>());
        let mut layouts = IndexMap::new();
        let placements = donder_model::patch_placements(project, patch, &counts);
        let streamed = setup
            .controllers
            .iter()
            .map(|controller| *controller.id())
            .collect::<Vec<_>>();
        for (route, only) in routes {
            let placement = placements
                .get(&route.id)
                .filter(|placement| placement.output == ControllerOutput::Universes);
            let Some(placement) = placement.filter(|_| streamed.contains(&route.controller)) else {
                continue;
            };
            let layout = project
                .get::<Layout>(route.target.layout)
                .ok_or_else(|| TestError::PixelCount("the routed layout is missing".into()))?;
            let fixture_pixels = layout
                .target_pixel_count(
                    &FixtureTarget {
                        layout: route.target.layout,
                        fixture: route.target.fixture,
                    },
                    &counts,
                )
                .map_err(|error| TestError::PixelCount(format!("{error:?}")))?;
            let routed = route.pixels.unwrap_or(PixelSpan {
                start: 0,
                count: fixture_pixels,
            });
            // The routed pixels a slice selects; a route outside it lights none.
            let span = match only {
                None => routed,
                Some(only) => {
                    let start = routed.start.max(only.start);
                    let end = (routed.start + routed.count).min(only.start + only.count);
                    if end <= start {
                        continue;
                    }
                    PixelSpan {
                        start,
                        count: end - start,
                    }
                }
            };
            let width = route.encoding.channel_count() as u32;
            let start_channel = placement.channels.start + (span.start - routed.start) * width;
            let prepared = layouts
                .entry(route.target.layout)
                .or_insert_with(|| geometry.prepare_layout(layout));
            let heights = prepared
                .instances
                .iter()
                .find(|instance| instance.id == route.target.fixture)
                .map(|instance| {
                    instance
                        .pixels
                        .iter()
                        .skip(span.start as usize)
                        .take(span.count as usize)
                        .map(|pixel| instance.transform.transform_point3(pixel.position).y)
                        .collect()
                })
                .unwrap_or_default();
            test.add_controller(project, route.controller)?;
            test.regions.push(Region {
                controller: route.controller,
                start_channel,
                end_channel: start_channel + span.count * width,
                first_pixel: span.start,
                fixture_pixels,
                pixels: span.count,
                encoding: route.encoding,
                gamma: route.gamma,
                brightness: route.brightness,
                heights,
                reversed: false,
            });
        }
        if test.regions.is_empty() {
            return Err(TestError::NoStreamedRoutes);
        }
        test.measure();
        Ok(test)
    }

    /// Add a streamed controller; its channel count.
    fn add_controller(
        &mut self,
        project: &DonderProject,
        id: ControllerId,
    ) -> Result<u32, TestError> {
        let controller = project
            .get::<Controller>(id)
            .ok_or(TestError::MissingController(id))?;
        let channels = controller
            .universes()
            .ok_or(TestError::NotStreamed(id))?
            .channel_count();
        self.controllers.insert(id, controller.clone());
        Ok(channels)
    }

    /// Run the pattern from each fixture's last pixel to its first.
    pub fn reverse(&mut self) {
        for region in &mut self.regions {
            region.reversed = !region.reversed;
        }
    }

    /// Light `other`'s channels too, with this test's pattern.
    pub fn merge(&mut self, other: Self) {
        self.controllers.extend(other.controllers);
        self.regions.extend(other.regions);
        self.measure();
    }

    fn measure(&mut self) {
        let heights = self
            .regions
            .iter()
            .flat_map(|region| region.heights.iter().copied());
        self.heights = heights.fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), height| {
            (low.min(height), high.max(height))
        });
    }

    /// The controllers the test streams to.
    pub fn controllers(&self) -> &IndexMap<ControllerId, Controller> {
        &self.controllers
    }

    /// Each tested controller's channels `elapsed` into the test; channels no
    /// region covers are zero.
    pub fn frames(&self, elapsed: Duration) -> Vec<OutputFrame> {
        let seconds = elapsed.as_secs_f32();
        self.controllers
            .iter()
            .map(|(id, controller)| {
                let channels = controller
                    .universes()
                    .map_or(0, |universes| universes.channel_count());
                let mut slots = vec![0; channels as usize];
                for region in self
                    .regions
                    .iter()
                    .filter(|region| region.controller == *id)
                {
                    region.write(self.pattern, seconds, self.heights, &mut slots);
                }
                OutputFrame {
                    controller: *id,
                    slots,
                }
            })
            .collect()
    }
}

impl Region {
    fn write(&self, pattern: TestPattern, seconds: f32, range: (f32, f32), slots: &mut [u8]) {
        let width = self.encoding.channel_count();
        for pixel in 0..self.pixels {
            let start = self.start_channel as usize + pixel as usize * width;
            let end = (start + width).min(self.end_channel as usize);
            let Some(bytes) = slots.get_mut(start..end) else {
                return;
            };
            if pattern == TestPattern::Order {
                for (channel, byte) in bytes.iter_mut().enumerate() {
                    *byte = if channel == pixel as usize { 255 } else { 0 };
                }
                continue;
            }
            let height = self
                .heights
                .get(pixel as usize)
                .map_or(0.0, |height| (height - range.0) / (range.1 - range.0));
            let index = self.first_pixel + pixel;
            let index = if self.reversed {
                self.fixture_pixels - 1 - index
            } else {
                index
            };
            let [red, green, blue] = color(pattern, index, self.fixture_pixels, height, seconds)
                .map(|channel| f32::from(channel) / 255.0);
            let levels: [f32; 4] = match self.encoding {
                PixelEncoding::Rgb { order } => {
                    let parts = [red, green, blue];
                    [0, 1, 2, 0].map(|channel| parts[usize::from(order[channel])])
                }
                PixelEncoding::Rgbw { order } => {
                    let white = red.min(green).min(blue);
                    let parts = [red - white, green - white, blue - white, white];
                    order.map(|component| parts[usize::from(component)])
                }
                PixelEncoding::Channels { colors } => {
                    let mut levels = [0.0; 4];
                    for (level, color) in levels.iter_mut().zip(colors.as_slice()) {
                        *level = donder_runtime_types::channel_level(
                            [red, green, blue],
                            color.map(|part| f32::from(part) / 255.0),
                        );
                    }
                    levels
                }
            };
            for (byte, level) in bytes.iter_mut().zip(levels) {
                *byte = (donder_model::patched_level(
                    level.clamp(0.0, 1.0),
                    self.gamma,
                    self.brightness,
                ) * 255.0)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// A pattern's color for a fixture's pixel.
fn color(pattern: TestPattern, pixel: u32, pixels: u32, height: f32, seconds: f32) -> [u8; 3] {
    match pattern {
        TestPattern::Solid(color) => color,
        TestPattern::Order => [0, 0, 0],
        TestPattern::Count => {
            if pixel == 0 {
                [255, 0, 0]
            } else if pixel + 1 == pixels {
                [255, 0, 255]
            } else if pixel % 50 == 49 {
                [255, 255, 255]
            } else if pixel % 10 == 9 {
                [0, 255, 0]
            } else {
                COUNT_BACKGROUND
            }
        }
        TestPattern::Rise => {
            let level = (seconds / RISE_SECONDS).fract() * (1.0 + RISE_BAND);
            if (height - level).abs() < RISE_BAND {
                [255, 255, 255]
            } else {
                [0, 0, 0]
            }
        }
        TestPattern::Chase => {
            let head = (seconds * CHASE_PIXELS_PER_SECOND) as u32 % pixels;
            match (head + pixels - pixel) % pixels {
                0 => [255, 255, 255],
                behind if behind <= CHASE_TAIL => [0, 0, (255 / (behind + 1)) as u8],
                _ => [0, 0, 0],
            }
        }
    }
}
