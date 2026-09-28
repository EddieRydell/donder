use super::{PreparedPixel, fixture_transform};
use donder_language::fixture::{
    FixtureElement, FixtureElementId, FixtureShape, GridAxis, GridCorner,
};
use glam::Vec3;

#[derive(Debug)]
pub struct InvalidFixtureElement(pub FixtureElementId);

fn point(point: &donder_language::values::Point3) -> Vec3 {
    Vec3::new(
        point.x.as_meters_f32(),
        point.y.as_meters_f32(),
        point.z.as_meters_f32(),
    )
}

/// Authored control handles, in fixture coordinates. Does not depend on pixel count.
pub fn element_handles(element: &FixtureElement) -> Vec<Vec3> {
    let handles = match &element.shape {
        FixtureShape::Pixel => Vec::new(),
        FixtureShape::Line { length, .. } => vec![Vec3::ZERO, Vec3::X * *length],
        FixtureShape::Polyline { points, .. } => points.iter().map(point).collect(),
        FixtureShape::Arc {
            radius,
            start_degrees,
            sweep_degrees,
            closed,
            ..
        } => {
            let mut handles = vec![Vec3::ZERO, arc_point(*radius, *start_degrees)];
            if !closed {
                handles.push(arc_point(
                    *radius,
                    start_degrees.rem_euclid(360.0) + sweep_degrees,
                ));
            }
            handles
        }
        FixtureShape::Grid { width, height, .. } => {
            vec![Vec3::ZERO, Vec3::new(*width, *height, 0.0)]
        }
    };
    let transform = fixture_transform(&element.transform);
    handles
        .into_iter()
        .map(|point| transform.transform_point3(point))
        .collect()
}

fn arc_point(radius: f32, degrees: f32) -> Vec3 {
    let (sin, cos) = degrees.rem_euclid(360.0).to_radians().sin_cos();
    Vec3::new(radius * cos, radius * sin, 0.0)
}

/// Expand a validated authored shape once, before playback.
pub fn element_pixels(
    element: &FixtureElement,
) -> Result<Vec<PreparedPixel>, InvalidFixtureElement> {
    if !element.is_valid() {
        return Err(InvalidFixtureElement(element.id));
    }
    if element_handles(element)
        .iter()
        .any(|point| !point.is_finite() || point.abs().max_element() > 2_000.0)
    {
        return Err(InvalidFixtureElement(element.id));
    }
    let count = element
        .shape
        .pixel_count()
        .ok_or(InvalidFixtureElement(element.id))?;
    let transform = fixture_transform(&element.transform);
    let polyline = match &element.shape {
        FixtureShape::Polyline { points, .. } => points.iter().map(point).collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let mut total_length = 0.0;
    let segments = polyline
        .windows(2)
        .filter_map(|pair| {
            let length = pair[0].distance(pair[1]);
            if length == 0.0 {
                return None;
            }
            let start_distance = total_length;
            total_length += length;
            Some((pair[0], pair[1], start_distance, total_length))
        })
        .collect::<Vec<_>>();
    (0..count)
        .map(|index| {
            let ordinal = if element.reverse {
                count - 1 - index
            } else {
                index
            };
            let fraction = if count <= 1 {
                0.0
            } else {
                ordinal as f32 / (count - 1) as f32
            };
            let position = match &element.shape {
                FixtureShape::Pixel => Vec3::ZERO,
                FixtureShape::Line { length, .. } => Vec3::X * *length * fraction,
                FixtureShape::Polyline { .. } => {
                    let distance = fraction * total_length;
                    let segment = segments.partition_point(|segment| segment.3 < distance);
                    let (start, end, from, to) = segments
                        .get(segment)
                        .ok_or(InvalidFixtureElement(element.id))?;
                    start.lerp(*end, (distance - from) / (to - from))
                }

                FixtureShape::Arc {
                    radius,
                    start_degrees,
                    sweep_degrees,
                    closed,
                    ..
                } => {
                    let fraction = if *closed {
                        ordinal as f32 / count as f32
                    } else {
                        fraction
                    };
                    arc_point(
                        *radius,
                        start_degrees.rem_euclid(360.0) + sweep_degrees * fraction,
                    )
                }
                FixtureShape::Grid {
                    columns,
                    rows,
                    width,
                    height,
                    axis,
                    corner,
                    serpentine,
                } => {
                    let minor = match axis {
                        GridAxis::Rows => *columns,
                        GridAxis::Columns => *rows,
                    };
                    let lane = ordinal / minor;
                    let along = ordinal % minor;
                    let along = if *serpentine && lane % 2 == 1 {
                        minor - 1 - along
                    } else {
                        along
                    };
                    let (mut x, mut y) = match axis {
                        GridAxis::Rows => (along, lane),
                        GridAxis::Columns => (lane, along),
                    };
                    if matches!(corner, GridCorner::BottomRight | GridCorner::TopRight) {
                        x = columns - 1 - x;
                    }
                    if matches!(corner, GridCorner::TopLeft | GridCorner::TopRight) {
                        y = rows - 1 - y;
                    }
                    Vec3::new(
                        if *columns > 1 {
                            *width * x as f32 / (columns - 1) as f32
                        } else {
                            0.0
                        },
                        if *rows > 1 {
                            *height * y as f32 / (rows - 1) as f32
                        } else {
                            0.0
                        },
                        0.0,
                    )
                }
            };
            let position = transform.transform_point3(position);
            if !position.is_finite() || position.abs().max_element() > 2_000.0 {
                return Err(InvalidFixtureElement(element.id));
            }
            Ok(PreparedPixel {
                element: element.id,
                ordinal,
                position,
                diameter_meters: element.diameter.as_meters_f32(),
            })
        })
        .collect()
}
