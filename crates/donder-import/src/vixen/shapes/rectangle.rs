//! Four strings of lights around a quadrilateral's sides, starting at its
//! bottom left corner. Vixen: `PreviewRectangle`.

use super::arch::point;
use super::multi_string::{add_line, child_line};
use super::{Laid, ShapeSkip, strings, unrotated};
use crate::vixen::contract::{self, FieldError};
use crate::xml::Element;
use glam::DVec2;

/// Which way the strings run around the corners. Vixen:
/// `PreviewRectangle.Directions`.
#[derive(Clone, Copy)]
enum Direction {
    Clockwise,
    CounterClockwise,
}

/// Vixen: `PreviewRectangle.Direction`, whose unwritten value is the
/// enum's first.
fn direction(shape: &Element) -> Result<Direction, ShapeSkip> {
    match contract::text(shape, "Direction").map(str::trim) {
        None | Some("Clockwise") => Ok(Direction::Clockwise),
        Some("CounterClockwise") => Ok(Direction::CounterClockwise),
        Some(other) => Err(ShapeSkip::Field(FieldError {
            field: "Direction".to_string(),
            value: other.to_string(),
        })),
    }
}

/// Vixen: `PreviewRectangle.OnDeserialized`, `PreviewRectangle.Layout`,
/// `PreviewRectangle.Pixels`.
pub(super) fn layout(shape: &Element) -> Result<Laid, ShapeSkip> {
    unrotated(shape)?;
    let top_left = point(shape, "_topLeft")?;
    let top_right = point(shape, "_topRight")?;
    let bottom_left = point(shape, "_bottomLeft")?;
    let bottom_right = point(shape, "_bottomRight")?;
    let direction = direction(shape)?;
    let strings = strings(shape);
    if strings.len() < 4 {
        // Vixen lays out its first four strings by index.
        return Err(ShapeSkip::Unplaceable(
            "a rectangle of fewer than four strings",
        ));
    }
    if strings.len() > 4 {
        // Vixen leaves the others where they lay themselves out.
        return Err(ShapeSkip::Setting("strings beyond four"));
    }
    // Each side's start and end, and whether it is padded at both ends;
    // the padded sides are pulled in a unit at each end, in whole units
    // as Vixen does.
    let x = DVec2::X;
    let y = DVec2::Y;
    let sides = match direction {
        Direction::CounterClockwise => [
            (bottom_left, bottom_right, false),
            (bottom_right - y, top_right + y, true),
            (top_right, top_left, false),
            (top_left + y, bottom_left - y, true),
        ],
        Direction::Clockwise => [
            (bottom_left, top_left, false),
            (top_left + x, top_right - x, true),
            (top_right, bottom_right, false),
            (bottom_right - x, bottom_left + x, true),
        ],
    };
    let mut laid = Laid::default();
    for (child, (start, end, padded)) in strings.into_iter().zip(sides) {
        let nodes = child_line(child)?;
        add_line(&mut laid, nodes, start, end, padded, padded)?;
    }
    Ok(laid)
}
