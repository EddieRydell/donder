//! Strands of lights hanging straight down from points evenly spaced
//! between two points. Vixen: `PreviewIcicle`.

use super::arch::lights;
use super::multi_string::{add_line, child_line, counted_strings};
use super::{Laid, ShapeSkip, line, strings, unrotated};
use crate::vixen::contract;
use crate::xml::Element;
use glam::DVec2;

/// Vixen: `PreviewIcicle.OnDeserialized`, `PreviewIcicle.Layout`,
/// `PreviewIcicle.Strings`, `PreviewIcicle.VerticalSpacing`.
pub(super) fn layout(shape: &Element) -> Result<Laid, ShapeSkip> {
    unrotated(shape)?;
    let (first, last) = line::ends(shape)?;
    if strings(shape).is_empty() && !lights(shape)?.is_empty() {
        // Vixen's `Pixels` are then its own, which its layout does not
        // place.
        return Err(ShapeSkip::Setting("lights outside strings"));
    }
    let strings = counted_strings(shape)?;
    let vertical_spacing = contract::int(shape, "_verticalSpacing", 0)?.max(2);
    let step = (first - last) / (strings.len() as f64 - 1.0);
    let mut laid = Laid::default();
    for (index, child) in strings.into_iter().enumerate() {
        let nodes = child_line(child)?;
        // Vixen steps from the first point by the spacing; a single strand
        // takes only the first step, at the first point. Vixen rounds each
        // strand's top to whole units, which this does not.
        let top = match index {
            0 => first,
            _ => first - step * index as f64,
        };
        // The strand's length: its lights times the spacing, multiplied in
        // a wrapping `int` as Vixen does.
        let count = i32::try_from(nodes.len()).unwrap_or(i32::MAX);
        let length = vertical_spacing.wrapping_mul(count);
        let bottom = top + DVec2::new(0.0, f64::from(length));
        add_line(&mut laid, nodes, top, bottom, false, false)?;
    }
    Ok(laid)
}
