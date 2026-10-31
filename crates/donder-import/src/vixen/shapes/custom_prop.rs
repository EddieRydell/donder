//! A prop drawn in Vixen's custom prop editor, its lights saved at their
//! own positions. Vixen: `PreviewCustomProp`.

use super::{Laid, ShapePixel, ShapeSkip};
use crate::vixen::contract;
use crate::xml::Element;
use glam::DVec2;

/// One of the prop's saved lights. Vixen: `PreviewPixel`.
struct PropPixel {
    node: Option<String>,
    location: DVec2,
    size: f64,
}

/// The prop's saved lights, in order. Vixen: `PreviewCustomProp.PropPixels`,
/// `PreviewPixel.Location`, `IsHighPrecision`, `PixelSize`.
fn prop_pixels(shape: &Element) -> Result<Vec<PropPixel>, ShapeSkip> {
    contract::required(shape, "PropPixels")?
        .children("PreviewPixel")
        .map(|pixel| {
            if !contract::boolean(pixel, "IsHighPrecision", false)? {
                // Vixen's rotation copies only a high-precision location.
                return Err(ShapeSkip::Setting("lights without a precise location"));
            }
            let (x, y) = match contract::field(pixel, "Location") {
                Some(location) => (
                    contract::double(location, "_x", 0.0)?,
                    contract::double(location, "_y", 0.0)?,
                ),
                None => (0.0, 0.0),
            };
            let size = contract::int(pixel, "PixelSize", 0)?;
            if size < 0 {
                // WPF's `Rect` throws on a negative size.
                return Err(ShapeSkip::Unplaceable("a light of negative size"));
            }
            Ok(PropPixel {
                node: contract::guid(pixel, "NodeId")?,
                location: DVec2::new(x, y),
                size: f64::from(size),
            })
        })
        .collect()
}

/// The centre of the lights' combined bounds, each light a square of its
/// size from its location. Vixen: `PreviewCustomProp.UpdateBounds`,
/// `Center`; `PreviewBaseShape.GetCombinedBounds`.
fn rotation_center(pixels: &[PropPixel]) -> DVec2 {
    let mut min = DVec2::splat(f64::INFINITY);
    let mut max = DVec2::splat(f64::NEG_INFINITY);
    for pixel in pixels {
        min = min.min(pixel.location);
        max = max.max(pixel.location + pixel.size);
    }
    let size = max - min;
    min + size / 2.0
}

/// `point` turned `angle` degrees clockwise on screen about `center`, in
/// the order WPF's `RotateTransform` computes it. Vixen:
/// `PreviewCustomProp.Rotate`; WPF `Matrix.RotateAt`, `Matrix.Transform`.
fn rotated(point: DVec2, angle: f64, center: DVec2) -> DVec2 {
    let radians = (angle % 360.0) * (std::f64::consts::PI / 180.0);
    let sin = radians.sin();
    let cos = radians.cos();
    let offset_x = center.x * (1.0 - cos) + center.y * sin;
    let offset_y = center.y * (1.0 - cos) - center.x * sin;
    DVec2::new(
        point.x * cos + (point.y * -sin + offset_x),
        point.y * cos + (point.x * sin + offset_y),
    )
}

/// Each saved light at its saved location, turned by the prop's rotation
/// about the centre of the lights' bounds, as the prop's `OnDeserialized`
/// lays them out at zoom 1. A rotation outside ±360 degrees is one Vixen's
/// setter ignores. Vixen: `PreviewCustomProp.OnDeserialized`, `Layout`,
/// `Pixels`; `PreviewBaseShape.RotationAngle`.
pub(super) fn layout(shape: &Element) -> Result<Laid, ShapeSkip> {
    let pixels = prop_pixels(shape)?;
    let angle = match contract::int(shape, "RotationAngle", 0)? {
        angle @ -360..=360 => angle,
        _ => 0,
    };
    let center = rotation_center(&pixels);
    Ok(Laid {
        pixels: pixels
            .into_iter()
            .map(|pixel| ShapePixel {
                node: pixel.node,
                position: if angle == 0 {
                    pixel.location
                } else {
                    rotated(pixel.location, f64::from(angle), center)
                },
                arc: None,
            })
            .collect(),
        ellipses: Vec::new(),
    })
}
