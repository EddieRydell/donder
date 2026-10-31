//! One light at a point. Vixen: `PreviewSingle`.

use super::{Laid, ShapePixel, ShapeSkip, pixel_nodes, strings, unrotated};
use crate::vixen::contract;
use crate::xml::Element;
use glam::DVec2;

/// Refuses the custom string type, whose lights Vixen draws at their
/// `Location` instead. Vixen: `PreviewLightBaseShape.StringType`.
pub(super) fn not_custom(shape: &Element) -> Result<(), ShapeSkip> {
    match contract::text(shape, "StringType").map(str::trim) {
        Some("Custom") => Err(ShapeSkip::Setting("custom string type")),
        _ => Ok(()),
    }
}

/// The nodes of a shape's own `Pixels`, refusing what puts its lights
/// elsewhere than its layout does: child strings, whose pixels the `Pixels`
/// getter returns instead, the custom string type, and high-precision
/// lights, which Vixen moves to their `Location`. Vixen:
/// `PreviewLightBaseShape.Pixels`, `PreviewPixel.Point`.
pub(super) fn own_pixels(shape: &Element) -> Result<Vec<Option<String>>, ShapeSkip> {
    if !strings(shape).is_empty() {
        return Err(ShapeSkip::Setting("child strings"));
    }
    not_custom(shape)?;
    if let Some(pixels) = contract::field(shape, "Pixels") {
        for pixel in pixels.children("PreviewPixel") {
            if contract::boolean(pixel, "IsHighPrecision", false)? {
                return Err(ShapeSkip::Setting("high-precision lights"));
            }
        }
    }
    pixel_nodes(shape)
}

/// Vixen: `PreviewSingle.Layout`.
pub(super) fn layout(shape: &Element) -> Result<Laid, ShapeSkip> {
    unrotated(shape)?;
    let nodes = own_pixels(shape)?;
    let (x, y) = contract::point(shape, "p1")?;
    if nodes.len() > 1 {
        // Vixen places only the first.
        return Err(ShapeSkip::Unplaceable("a single light of several lights"));
    }
    Ok(Laid {
        pixels: nodes
            .into_iter()
            .map(|node| ShapePixel {
                node,
                position: DVec2::new(f64::from(x), f64::from(y)),
                arc: None,
            })
            .collect(),
        ellipses: Vec::new(),
    })
}
