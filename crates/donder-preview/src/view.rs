//! The Preview's view of a layout: every pixel seen from the front, the
//! bounds and camera that fit them to a canvas, and the style rules for that
//! fit. The desktop Preview window and video export both draw through it.
use donder_model::{DonderProject, PreparedFixtureDefinitions};
use glam::Vec2;

/// A pixel seen from the front, in meters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrontPixel {
    pub position: Vec2,
    pub radius: f32,
}

/// A fixture instance's pixels, in output pixel order.
#[derive(Clone, Debug, PartialEq)]
pub struct FrontFixture {
    pub id: u32,
    pub pixels: Vec<FrontPixel>,
}

/// The active layout seen from the front, fixtures in layout (drawing) order.
#[derive(Clone, Debug, PartialEq)]
pub struct FrontView {
    pub fixtures: Vec<FrontFixture>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrontViewError {
    MissingSetup,
    MissingLayout,
}

impl std::fmt::Display for FrontViewError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::MissingSetup => "The project's setup was not found.",
            Self::MissingLayout => "The setup's layout was not found.",
        })
    }
}

impl std::error::Error for FrontViewError {}

impl FrontView {
    pub fn from_project(project: &DonderProject) -> Result<Self, FrontViewError> {
        let setup = project
            .setup(project.root().setup.id())
            .ok_or(FrontViewError::MissingSetup)?;
        let layout = project
            .layout(setup.layout.id())
            .ok_or(FrontViewError::MissingLayout)?;
        let definitions = PreparedFixtureDefinitions::prepare(&project.definitions().fixtures);
        let layout = definitions.prepare_layout(layout);
        let fixtures = layout
            .instances
            .iter()
            .map(|fixture| FrontFixture {
                id: fixture.id.0,
                pixels: fixture
                    .pixels
                    .iter()
                    .map(|pixel| {
                        let point = fixture.transform.transform_point3(pixel.position);
                        FrontPixel {
                            position: Vec2::new(point.x, point.y),
                            radius: pixel.diameter_meters / 2.0,
                        }
                    })
                    .collect(),
            })
            .collect();
        Ok(Self { fixtures })
    }

    pub fn pixels(&self) -> impl Iterator<Item = &FrontPixel> {
        self.fixtures.iter().flat_map(|fixture| &fixture.pixels)
    }
}

/// The extent of pixel centres, in meters. An empty view spans the unit square.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewBounds {
    pub min: Vec2,
    pub max: Vec2,
}

impl ViewBounds {
    pub fn from_points(points: impl IntoIterator<Item = Vec2>) -> Self {
        let mut points = points.into_iter();
        let Some(first) = points.next() else {
            return Self::default();
        };
        let (min, max) = points.fold((first, first), |(min, max), point| {
            (min.min(point), max.max(point))
        });
        Self { min, max }
    }
}

impl Default for ViewBounds {
    fn default() -> Self {
        Self {
            min: Vec2::ZERO,
            max: Vec2::ONE,
        }
    }
}

/// Pan and zoom that fit `ViewBounds` into a canvas.
///
/// `preview.wgsl` applies `to_screen` and `radius` on the GPU; keep it in step
/// with them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewCamera {
    /// The meter position at the canvas centre.
    pub pan: Vec2,
    /// Canvas pixels per meter.
    pub zoom: f32,
}

impl ViewCamera {
    /// Fill `fill_ratio` of the canvas with the bounds, treating them as at
    /// least a meter across and never zooming below one pixel per meter.
    pub fn fit(bounds: ViewBounds, width: u32, height: u32, fill_ratio: f32) -> Self {
        let span = (bounds.max - bounds.min).max(Vec2::ONE);
        let available = Vec2::new(width as f32, height as f32) * fill_ratio;
        let zoom = (available.x / span.x).min(available.y / span.y).max(1.0);
        Self {
            pan: (bounds.min + bounds.max) * 0.5,
            zoom,
        }
    }

    /// The canvas position of a meter position, y down.
    pub fn to_screen(self, position: Vec2, width: u32, height: u32) -> Vec2 {
        Vec2::new(
            width as f32 * 0.5 + (position.x - self.pan.x) * self.zoom,
            height as f32 * 0.5 - (position.y - self.pan.y) * self.zoom,
        )
    }

    /// The drawn radius in canvas pixels of a pixel `radius` meters wide.
    pub fn radius(self, radius: f32, minimum_radius_pixels: f32) -> f32 {
        (radius * self.zoom).max(minimum_radius_pixels)
    }
}

/// How the view fits the canvas; the values come from the caller's theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewStyle {
    /// Share of the canvas the layout fills, in `(0, 1]`.
    pub canvas_fill_ratio: f32,
    /// Smallest drawn pixel radius, in canvas pixels.
    pub minimum_radius_pixels: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewStyleError {
    CanvasFillRatio,
    MinimumRadius,
}

impl std::fmt::Display for ViewStyleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::CanvasFillRatio => "The canvas fill ratio must be in (0, 1].",
            Self::MinimumRadius => "The minimum pixel radius must not be negative.",
        })
    }
}

impl std::error::Error for ViewStyleError {}

impl ViewStyle {
    pub fn validate(self) -> Result<Self, ViewStyleError> {
        if !self.canvas_fill_ratio.is_finite()
            || self.canvas_fill_ratio <= 0.0
            || self.canvas_fill_ratio > 1.0
        {
            return Err(ViewStyleError::CanvasFillRatio);
        }
        if !self.minimum_radius_pixels.is_finite() || self.minimum_radius_pixels < 0.0 {
            return Err(ViewStyleError::MinimumRadius);
        }
        Ok(self)
    }
}
