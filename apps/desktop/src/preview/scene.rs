use bytemuck::{Pod, Zeroable};
use glam::Vec2;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct PreviewInstance {
    pub(crate) center_radius: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
pub(crate) struct PreviewColor {
    pub(crate) rgba: [u8; 4],
}

impl PreviewColor {
    pub const fn opaque(rgb: [u8; 3]) -> Self {
        Self {
            rgba: [rgb[0], rgb[1], rgb[2], u8::MAX],
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PreviewScene {
    pub(crate) revision: u64,
    pub(crate) instances: Vec<PreviewInstance>,
    bounds: PreviewBounds,
}

impl PreviewScene {
    pub(crate) fn new(revision: u64, instances: Vec<PreviewInstance>) -> Self {
        let bounds = PreviewBounds::from_instances(&instances);
        Self {
            revision,
            instances,
            bounds,
        }
    }

    pub(crate) fn bounds(&self) -> PreviewBounds {
        self.bounds
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PreviewBounds {
    min: Vec2,
    max: Vec2,
}

impl PreviewBounds {
    fn from_instances(instances: &[PreviewInstance]) -> Self {
        let Some(first) = instances.first() else {
            return Self::default();
        };
        let mut min = instance_position(first);
        let mut max = min;
        for instance in instances.iter().skip(1) {
            let position = instance_position(instance);
            min = min.min(position);
            max = max.max(position);
        }
        Self { min, max }
    }
}

impl Default for PreviewBounds {
    fn default() -> Self {
        Self {
            min: Vec2::ZERO,
            max: Vec2::ONE,
        }
    }
}

fn instance_position(instance: &PreviewInstance) -> Vec2 {
    Vec2::new(instance.center_radius[0], instance.center_radius[1])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PreviewSize {
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl PreviewSize {
    pub(crate) fn nonzero(width: u32, height: u32) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
        }
    }

    pub(crate) fn clamp_to(self, max_dimension: u32) -> Self {
        Self {
            width: self.width.min(max_dimension),
            height: self.height.min(max_dimension),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PreviewStyle {
    pub(crate) background_rgb: [u8; 3],
    pub(crate) unlit_rgb: [u8; 3],
    pub(crate) canvas_fill_ratio: f32,
    pub(crate) minimum_radius_pixels: f32,
}

impl PreviewStyle {
    pub(crate) fn validate(self) -> Result<Self, PreviewStyleError> {
        if !self.canvas_fill_ratio.is_finite()
            || self.canvas_fill_ratio <= 0.0
            || self.canvas_fill_ratio > 1.0
        {
            return Err(PreviewStyleError::CanvasFillRatio);
        }
        if !self.minimum_radius_pixels.is_finite() || self.minimum_radius_pixels < 0.0 {
            return Err(PreviewStyleError::MinimumRadius);
        }
        Ok(self)
    }

    pub(crate) fn unlit_color(self) -> PreviewColor {
        PreviewColor::opaque(self.unlit_rgb)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PreviewStyleError {
    CanvasFillRatio,
    MinimumRadius,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PreviewCamera {
    pub(crate) pan: Vec2,
    pub(crate) zoom: f32,
}

impl PreviewCamera {
    pub(crate) fn fit(bounds: PreviewBounds, size: PreviewSize, fill_ratio: f32) -> Self {
        let span = (bounds.max - bounds.min).max(Vec2::ONE);
        let available = Vec2::new(size.width as f32, size.height as f32) * fill_ratio;
        let zoom = (available.x / span.x).min(available.y / span.y).max(1.0);
        Self {
            pan: (bounds.min + bounds.max) * 0.5,
            zoom,
        }
    }
}
