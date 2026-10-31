//! A custom template: a group of shapes saved together, each laid out on
//! its own. Vixen: `PreviewCustom`.

use super::{Laid, ShapeSkip, pixel_nodes, strings, unrotated};
use crate::xml::Element;

/// Each of the template's shapes laid out where it is saved, its lights in
/// turn, as the template's `OnDeserialized` lays them out again after each
/// shape's own: the template neither moves nor scales them on load, and its
/// zoom-1 `SetPixelZoom` changes nothing. A template without shapes would
/// show its own saved lights, which hold no position. Vixen:
/// `PreviewCustom.OnDeserialized`, `Layout`; `PreviewLightBaseShape.Pixels`,
/// `SetPixelZoom`.
pub(super) fn layout(shape: &Element) -> Result<Laid, ShapeSkip> {
    unrotated(shape)?;
    let shapes = strings(shape);
    if shapes.is_empty() {
        return if pixel_nodes(shape)?.is_empty() {
            Ok(Laid::default())
        } else {
            Err(ShapeSkip::Setting("lights outside its shapes"))
        };
    }
    let mut laid = Laid::default();
    for child in shapes {
        let Laid { pixels, ellipses } = super::layout(child)?;
        let first_ellipse = laid.ellipses.len();
        laid.ellipses.extend(ellipses);
        laid.pixels.extend(pixels.into_iter().map(|mut pixel| {
            pixel.arc = pixel
                .arc
                .map(|(ellipse, degrees)| (first_ellipse + ellipse, degrees));
            pixel
        }));
    }
    Ok(laid)
}
