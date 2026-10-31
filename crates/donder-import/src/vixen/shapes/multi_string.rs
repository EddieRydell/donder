//! Strings of lights end to end through a list of points, one string
//! between each pair. Vixen: `PreviewMultiString`.

use super::arch::lights;
use super::{Laid, ShapePixel, ShapeSkip, line, strings, unrotated};
use crate::vixen::contract;
use crate::xml::Element;
use glam::DVec2;

/// A shape's child string, which must be a line Vixen lays out as it
/// loads, and the node of each of its lights. Vixen: `PreviewLine`,
/// `PreviewLine.OnDeserialized` (which indexes its two points), and the
/// parent's `Strings[i] as PreviewLine`.
pub(super) fn child_line(child: &Element) -> Result<Vec<Option<String>>, ShapeSkip> {
    if contract::type_name(child) != Some("PreviewLine") {
        return Err(ShapeSkip::Setting("strings that are not lines"));
    }
    // The line's own rotation turns its lights as it lays them out.
    unrotated(child)?;
    if !strings(child).is_empty() {
        return Err(ShapeSkip::Setting("strings within strings"));
    }
    line::ends(child)?;
    lights(child)
}

/// The lights of `nodes` laid out as a line from `start` to `end`, added to
/// `laid`. Vixen: `PreviewLine.Layout`, as the parent sets the line's
/// points and padding.
pub(super) fn add_line(
    laid: &mut Laid,
    nodes: Vec<Option<String>>,
    start: DVec2,
    end: DVec2,
    start_padding: bool,
    end_padding: bool,
) -> Result<(), ShapeSkip> {
    let positions = line::spaced(start, end, nodes.len(), start_padding, end_padding)?;
    laid.pixels.extend(
        nodes
            .into_iter()
            .zip(positions)
            .map(|(node, position)| ShapePixel {
                node,
                position,
                arc: None,
            }),
    );
    Ok(())
}

/// The shape's strings, which must number its `_stringCount`: Vixen lays
/// out only that many and leaves any others where they lay themselves out.
/// Vixen: `PreviewIcicle.Strings`, `PreviewPixelGrid.Layout`.
pub(super) fn counted_strings(shape: &Element) -> Result<Vec<&Element>, ShapeSkip> {
    let strings = strings(shape);
    let count = contract::int(shape, "_stringCount", 0)?;
    // Vixen lays out no strings for a negative count.
    let count = usize::try_from(count).unwrap_or(0);
    if count > strings.len() {
        return Err(ShapeSkip::Unplaceable(
            "fewer strings than its string count",
        ));
    }
    if count < strings.len() {
        return Err(ShapeSkip::Setting("strings beyond its string count"));
    }
    Ok(strings)
}

/// Vixen: `PreviewMultiString.OnDeserialized`, `PreviewMultiString.Layout`.
pub(super) fn layout(shape: &Element) -> Result<Laid, ShapeSkip> {
    unrotated(shape)?;
    let points = contract::points(shape, "_points")?;
    if points.is_empty() {
        // Vixen's load indexes the first point.
        return Err(ShapeSkip::Unplaceable("a multi-string without points"));
    }
    if points.len() < 2 {
        // Vixen then lays out nothing, leaving its strings where they lay
        // themselves out.
        return Err(ShapeSkip::Setting("a multi-string of one point"));
    }
    if contract::field(shape, "_strings").is_none() {
        // Vixen counts a null list of strings as it lays the shape out.
        return Err(ShapeSkip::Unplaceable("a multi-string without strings"));
    }
    let strings = strings(shape);
    if strings.is_empty() {
        // Vixen then lays out nothing, and its `Pixels` are its own, left
        // where they were saved.
        if lights(shape)?.is_empty() {
            return Ok(Laid::default());
        }
        return Err(ShapeSkip::Setting("lights outside strings"));
    }
    // Vixen resets a string count unlike its points' and then drops or
    // adds strings, which this does not reproduce.
    let count = points.len() - 1;
    let string_count = contract::int(shape, "_stringCount", 0)?;
    if usize::try_from(string_count).ok() != Some(count) || strings.len() != count {
        return Err(ShapeSkip::Setting("strings unlike its points"));
    }
    let point = |(x, y): (i32, i32)| DVec2::new(f64::from(x), f64::from(y));
    let mut laid = Laid::default();
    for (index, (child, ends)) in strings.into_iter().zip(points.windows(2)).enumerate() {
        let nodes = child_line(child)?;
        add_line(
            &mut laid,
            nodes,
            point(ends[0]),
            point(ends[1]),
            index > 0,
            false,
        )?;
    }
    Ok(laid)
}
