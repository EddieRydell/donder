//! Parallel strings of lights evenly spaced across a box, running up it or
//! across it. Vixen: `PreviewPixelGrid`.

use super::arch::point;
use super::multi_string::{add_line, child_line, counted_strings};
use super::{Laid, ShapeSkip, unrotated};
use crate::vixen::contract::{self, FieldError};
use crate::xml::Element;
use glam::DVec2;

/// Which way the strings run. Vixen: `PreviewPixelGrid.StringOrientations`.
#[derive(Clone, Copy)]
enum Orientation {
    Vertical,
    Horizontal,
}

/// Vixen: `PreviewPixelGrid._stringOrientation`, whose unwritten value is
/// the enum's first.
fn orientation(shape: &Element) -> Result<Orientation, ShapeSkip> {
    const FIELD: &str = "_stringOrientation";
    match contract::text(shape, FIELD).map(str::trim) {
        None | Some("Vertical") => Ok(Orientation::Vertical),
        Some("Horizontal") => Ok(Orientation::Horizontal),
        Some(other) => Err(ShapeSkip::Field(FieldError {
            field: FIELD.to_string(),
            value: other.to_string(),
        })),
    }
}

/// Vixen: `PreviewPixelGrid.OnDeserialized`, `PreviewPixelGrid.Layout`,
/// `PreviewPixelGrid.Pixels`.
pub(super) fn layout(shape: &Element) -> Result<Laid, ShapeSkip> {
    unrotated(shape)?;
    let top_left = point(shape, "_topLeft")?;
    let bottom_right = point(shape, "_bottomRight")?;
    let bottom_left = point(shape, "_bottomLeft")?;
    // Vixen's load sets the top right's point type, so it must be there,
    // though the layout does not use it.
    point(shape, "_topRight")?;
    let orientation = orientation(shape)?;
    // The grid's own pixels are cleared as it loads, and with no strings
    // its `Pixels` are those.
    let strings = counted_strings(shape)?;
    let gaps = strings.len() as f64 - 1.0;
    let mut laid = Laid::default();
    for (index, child) in strings.into_iter().enumerate() {
        let nodes = child_line(child)?;
        // Vixen steps from the first string by the spacing; a single string
        // takes only the first step. Vixen rounds each string's offset to
        // whole units, which this does not.
        let along = |from: f64, to: f64| match index {
            0 => from,
            _ => from + (to - from) / gaps * index as f64,
        };
        let (start, end) = match orientation {
            // Up from the bottom edge's height to the top's, left to right.
            Orientation::Vertical => {
                let x = along(top_left.x, bottom_right.x);
                (DVec2::new(x, bottom_right.y), DVec2::new(x, top_left.y))
            }
            // Across from the bottom left's x to the bottom right's,
            // bottom to top.
            Orientation::Horizontal => {
                let y = along(bottom_left.y, top_left.y);
                (DVec2::new(bottom_left.x, y), DVec2::new(bottom_right.x, y))
            }
        };
        add_line(&mut laid, nodes, start, end, false, false)?;
    }
    Ok(laid)
}
