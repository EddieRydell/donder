use dawn_language::preview::PropGeometry as DomainGeometry;
use dawn_language::values::DistanceSpan;

use crate::dto::{
    Geometry, GeometryRenderBounds, GeometryRenderGuide, GeometryRenderPlan, GeometryRenderPoint,
    PreviewPropPlacement,
};
use crate::preview::{arc_point, point3_meters, render_point};

pub(crate) fn geometry(geometry: &DomainGeometry) -> Geometry {
    match geometry {
        DomainGeometry::Points { points } => Geometry::Points {
            points: points.iter().map(|point| point3_meters(*point)).collect(),
        },
        DomainGeometry::Lines {
            points,
            point_count,
        } => Geometry::Lines {
            points: points.iter().map(|point| point3_meters(*point)).collect(),
            pixels: *point_count,
        },
        DomainGeometry::Arc {
            center,
            radius,
            start_degrees,
            end_degrees,
            point_count,
        } => Geometry::Arc {
            center: point3_meters(*center),
            radius_meters: radius.as_meters_f32(),
            start_degrees: *start_degrees,
            end_degrees: *end_degrees,
            pixels: *point_count,
        },
    }
}

pub(crate) fn render_plan(
    geometry: &DomainGeometry,
    bulb_radius: DistanceSpan,
) -> GeometryRenderPlan {
    let emitters = crate::preview::geometry_emitters(geometry);
    let guides = guides(geometry);
    let bounds = bounds_for_points(emitters.iter().cloned());
    GeometryRenderPlan {
        emitters,
        guides,
        bounds,
        bulb_radius_meters: bulb_radius.as_meters_f32(),
    }
}

fn guides(geometry: &DomainGeometry) -> Vec<GeometryRenderGuide> {
    match geometry {
        DomainGeometry::Lines { points, .. } => points
            .windows(2)
            .filter_map(|window| {
                let [from, to] = window else { return None };
                Some(GeometryRenderGuide::Line {
                    from: render_point(*from),
                    to: render_point(*to),
                })
            })
            .collect(),
        DomainGeometry::Arc {
            center,
            radius,
            start_degrees,
            end_degrees,
            ..
        } => {
            let radius_meters = radius.as_meters_f32();
            vec![GeometryRenderGuide::Arc {
                start: arc_point(*center, radius_meters, *start_degrees),
                end: arc_point(*center, radius_meters, *end_degrees),
                radius_x_meters: radius_meters,
                radius_y_meters: radius_meters,
                rotation: 0.0,
                large_arc: (*end_degrees - *start_degrees).abs() > 180.0,
                sweep_positive: end_degrees >= start_degrees,
            }]
        }
        DomainGeometry::Points { .. } => Vec::new(),
    }
}

fn bounds_for_points(points: impl Iterator<Item = GeometryRenderPoint>) -> GeometryRenderBounds {
    let mut bounds: Option<(f32, f32, f32, f32)> = None;
    for point in points {
        bounds = Some(match bounds {
            None => (
                point.x_meters,
                point.y_meters,
                point.x_meters,
                point.y_meters,
            ),
            Some((min_x, min_y, max_x, max_y)) => (
                min_x.min(point.x_meters),
                min_y.min(point.y_meters),
                max_x.max(point.x_meters),
                max_y.max(point.y_meters),
            ),
        });
    }
    let (min_x, min_y, mut max_x, mut max_y) = bounds.unwrap_or((0.0, 0.0, 1.0, 1.0));
    if (max_x - min_x).abs() < 1.0 {
        max_x = min_x + 1.0;
    }
    if (max_y - min_y).abs() < 1.0 {
        max_y = min_y + 1.0;
    }
    GeometryRenderBounds {
        min_x_meters: min_x,
        min_y_meters: min_y,
        max_x_meters: max_x,
        max_y_meters: max_y,
    }
}

pub(crate) fn layout_bounds(fixtures: &[PreviewPropPlacement]) -> GeometryRenderBounds {
    bounds_for_points(fixtures.iter().map(|fixture| GeometryRenderPoint {
        x_meters: fixture.transform.position.x_meters,
        y_meters: fixture.transform.position.y_meters,
        z_meters: fixture.transform.position.z_meters,
    }))
}

pub(crate) fn geometry_summary(geometry: &DomainGeometry) -> String {
    match geometry {
        DomainGeometry::Points { points } => format!("{} points", points.len()),
        DomainGeometry::Lines { point_count, .. } => format!("{point_count} line points"),
        DomainGeometry::Arc { point_count, .. } => format!("{point_count} arc points"),
    }
}
