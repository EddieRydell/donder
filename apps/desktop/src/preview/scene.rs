use bytemuck::{Pod, Zeroable};
use donder_preview::{ViewBounds, ViewStyle, ViewStyleError};
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
    bounds: ViewBounds,
}

impl PreviewScene {
    pub(crate) fn new(revision: u64, instances: Vec<PreviewInstance>) -> Self {
        let bounds = ViewBounds::from_points(
            instances
                .iter()
                .map(|instance| Vec2::new(instance.center_radius[0], instance.center_radius[1])),
        );
        Self {
            revision,
            instances,
            bounds,
        }
    }

    pub(crate) fn bounds(&self) -> ViewBounds {
        self.bounds
    }
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
    pub(crate) fn validate(self) -> Result<Self, ViewStyleError> {
        self.view().validate()?;
        Ok(self)
    }

    pub(crate) fn view(self) -> ViewStyle {
        ViewStyle {
            canvas_fill_ratio: self.canvas_fill_ratio,
            minimum_radius_pixels: self.minimum_radius_pixels,
        }
    }

    pub(crate) fn unlit_color(self) -> PreviewColor {
        PreviewColor::opaque(self.unlit_rgb)
    }
}
