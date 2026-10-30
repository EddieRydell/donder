//! A fixture's pixels, in channel order, as the fewest Donder shapes that
//! put each pixel where it is: runs of evenly spaced pixels along a line or
//! an ellipse, and single pixels between them.

use crate::models::Ellipse;
use donder_language::{Distance, DistanceSpan, Point3, Rotation3, Scale3};
use donder_model::{FixtureElement, FixtureElementId, FixtureShape, FixtureTransform};
use glam::{DMat3, DQuat, DVec3, EulerRot};

/// Bulbs drawn in the Preview this wide, as for 12 mm pixels in a holder.
const DIAMETER: f32 = 0.043;

pub(crate) struct Pixel {
    /// Meters, in layout coordinates.
    pub(crate) position: DVec3,
    /// The ellipse the pixel is on and how far along it, in degrees.
    pub(crate) arc: Option<(usize, f64)>,
}

fn point(position: DVec3) -> Point3 {
    Point3 {
        x: Distance::from_meters(position.x as f32),
        y: Distance::from_meters(position.y as f32),
        z: Distance::from_meters(position.z as f32),
    }
}

fn rotation(rotation: DQuat) -> Rotation3 {
    let (x, y, z) = rotation.to_euler(EulerRot::XYZ);
    Rotation3 {
        x: x.to_degrees() as f32,
        y: y.to_degrees() as f32,
        z: z.to_degrees() as f32,
    }
}

/// The turn between two angles, from -180 up to 180 degrees.
fn turn(from: f64, to: f64) -> f64 {
    let turn = (to - from).rem_euclid(360.0);
    if turn > 180.0 { turn - 360.0 } else { turn }
}

fn same(a: f64, b: f64, scale: f64) -> bool {
    (a - b).abs() <= 1e-6 * scale.max(1.0)
}

/// Pixels from `start` on the same ellipse, each the same turn past the last.
fn arc_run(pixels: &[Pixel], start: usize) -> usize {
    let Some((arc, first)) = pixels[start].arc else {
        return 1;
    };
    let mut end = start + 1;
    let mut step = None;
    let mut last = first;
    while let Some((next_arc, degrees)) = pixels.get(end).and_then(|pixel| pixel.arc) {
        let this = turn(last, degrees);
        if next_arc != arc || this == 0.0 || step.is_some_and(|step| !same(step, this, 1.0)) {
            break;
        }
        step = Some(this);
        last = degrees;
        end += 1;
        // A closed ring goes round once.
        if (end - start) as f64 * this.abs() >= 360.0 - 1e-6 {
            break;
        }
    }
    end - start
}

/// Pixels from `start` along one line, each the same step past the last.
fn line_run(pixels: &[Pixel], start: usize) -> usize {
    let Some(next) = pixels.get(start + 1) else {
        return 1;
    };
    let step = next.position - pixels[start].position;
    let length = step.length();
    if length < 1e-6 {
        return 1;
    }
    let tolerance = length * 1e-4 + 1e-6;
    let mut end = start + 2;
    while let Some(pixel) = pixels.get(end) {
        if (pixel.position - pixels[end - 1].position - step).length() > tolerance {
            break;
        }
        end += 1;
    }
    end - start
}

/// The ellipse's axes as a turn and a stretch, when they are at right
/// angles: Donder arcs are circles scaled along their own axes.
fn ellipse_frame(ellipse: &Ellipse) -> Option<(DQuat, f64, f64)> {
    let (u, v) = (ellipse.u.length(), ellipse.v.length());
    if u < 1e-9 || v < 1e-9 || ellipse.u.dot(ellipse.v).abs() > 1e-6 * u * v {
        return None;
    }
    let (x, y) = (ellipse.u / u, ellipse.v / v);
    let frame = DMat3::from_cols(x, y, x.cross(y));
    Some((DQuat::from_mat3(&frame), u, v))
}

/// The fixture's shapes around `origin`, the fixture's own position.
pub(crate) fn shapes(pixels: &[Pixel], ellipses: &[Ellipse], origin: DVec3) -> Vec<FixtureElement> {
    let mut shapes = Vec::new();
    let mut start = 0;
    while start < pixels.len() {
        let mut taken = 1;
        let mut element = None;
        let arc = arc_run(pixels, start);
        if arc >= 3
            && let Some((index, degrees)) = pixels[start].arc
            && let Some((frame, radius, height)) = ellipse_frame(&ellipses[index])
        {
            let step = turn(degrees, pixels[start + 1].arc.map_or(0.0, |(_, next)| next));
            let closed = arc as f64 * step.abs() >= 360.0 - 1e-6;
            let sweep = if closed {
                360.0_f64.copysign(step)
            } else {
                step * (arc - 1) as f64
            };
            taken = arc;
            element = Some((
                FixtureShape::Arc {
                    radius: radius as f32,
                    start_degrees: degrees.rem_euclid(360.0) as f32,
                    sweep_degrees: sweep as f32,
                    count: arc as u32,
                    closed,
                },
                ellipses[index].center,
                frame,
                Scale3 {
                    x: 1.0,
                    y: (height / radius) as f32,
                    z: 1.0,
                },
            ));
        }
        if element.is_none() {
            let line = line_run(pixels, start);
            if line >= 2 {
                let from = pixels[start].position;
                let to = pixels[start + line - 1].position;
                taken = line;
                element = Some((
                    FixtureShape::Line {
                        length: from.distance(to) as f32,
                        count: line as u32,
                    },
                    from,
                    DQuat::from_rotation_arc(DVec3::X, (to - from).normalize()),
                    Scale3::default(),
                ));
            }
        }
        let (shape, position, turned, scale) = element.unwrap_or((
            FixtureShape::Pixel,
            pixels[start].position,
            DQuat::IDENTITY,
            Scale3::default(),
        ));
        let id = shapes.len() as u32 + 1;
        shapes.push(FixtureElement {
            id: FixtureElementId(id),
            name: donder_language::object_name(&format!("shape_{id}")),
            transform: FixtureTransform {
                position: point(position - origin),
                rotation: rotation(turned),
                scale,
            },
            diameter: DistanceSpan::from_meters(DIAMETER),
            reverse: false,
            shape,
        });
        start += taken;
    }
    shapes
}

pub(crate) fn fixture_position(origin: DVec3) -> Point3 {
    point(origin)
}
