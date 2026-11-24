//! The simplest fixture shapes that explain where the pixels lit.
use core::f32::consts::TAU;
use core::ops::Range;

use donder_language::{Distance, DistanceSpan, Point3, Rotation3};
use donder_model::{
    FixtureElement, FixtureElementId, FixtureShape, FixtureTransform, GridAxis, GridCorner,
};
use glam::{Mat3, Vec2, Vec3};

/// Consecutive pixels this many spacings apart belong to separate shapes.
const GAP_SPACINGS: f32 = 3.0;
/// A shape fits when its pixels lie this many spacings from where they lit,
/// as a root mean square.
const TOLERANCE_SPACINGS: f32 = 0.3;
/// Polyline corners are kept where the path bends more than this many
/// spacings from a straight segment.
const SIMPLIFY_SPACINGS: f32 = 0.5;
/// An arc whose pixels go round within this share of a turn is closed.
const CLOSED_SHARE: f32 = 0.05;
/// A grid's columns and rows may lean from square by this cosine.
const SQUARE_COSINE: f32 = 0.1;
/// Grid fits within this factor of the closest are equally good.
const EQUAL_FIT_SHARE: f32 = 1.25;
/// Lit pixels sampled to measure the spacing between neighbors.
const NEIGHBOR_SAMPLES: usize = 1000;

/// A fitted shape in frame samples, y up.
#[derive(Debug)]
enum Piece {
    Pixel(Vec2),
    Line {
        start: Vec2,
        end: Vec2,
        count: u32,
    },
    Arc {
        center: Vec2,
        radius: f32,
        start: f32,
        sweep: f32,
        count: u32,
        closed: bool,
    },
    Polyline {
        points: Vec<Vec2>,
        count: u32,
    },
    Grid {
        origin: Vec2,
        across: Vec2,
        up: Vec2,
        columns: u32,
        rows: u32,
        axis: GridAxis,
        corner: GridCorner,
        serpentine: bool,
    },
}

/// Where the fitted shapes go: the definition's current pixels, scaled to
/// match, or centered at the origin with `spacing` meters between pixels.
pub(crate) struct Placement {
    pub(crate) center: Vec2,
    /// The diagonal of the pixels' bounds, in meters.
    pub(crate) diagonal: Option<f32>,
    pub(crate) spacing: f32,
    pub(crate) diameter: DistanceSpan,
}

/// The typical distance between consecutive lit pixels.
fn spacing(points: &[Option<Vec2>]) -> Option<f32> {
    let mut steps = observed(points)
        .collect::<Vec<_>>()
        .windows(2)
        .map(|pair| pair[0].1.distance(pair[1].1) / (pair[1].0 - pair[0].0) as f32)
        .collect::<Vec<_>>();
    steps.sort_by(f32::total_cmp);
    steps
        .get(steps.len() / 2)
        .copied()
        .filter(|&step| step > 0.0)
}

/// The typical distance from a lit pixel to the nearest other one, whatever
/// their order: the spacing a viewer sees, also between scattered lights.
fn neighbor_spacing(points: &[Option<Vec2>]) -> Option<f32> {
    let lit = observed(points).map(|(_, point)| point).collect::<Vec<_>>();
    let stride = lit.len().div_ceil(NEIGHBOR_SAMPLES).max(1);
    let mut nearest = lit
        .iter()
        .step_by(stride)
        .filter_map(|point| {
            lit.iter()
                .map(|other| point.distance(*other))
                .filter(|&distance| distance > 0.0)
                .min_by(f32::total_cmp)
        })
        .collect::<Vec<_>>();
    nearest.sort_by(f32::total_cmp);
    nearest.get(nearest.len() / 2).copied()
}

fn observed(points: &[Option<Vec2>]) -> impl Iterator<Item = (usize, Vec2)> + '_ {
    points
        .iter()
        .enumerate()
        .filter_map(|(index, point)| point.map(|point| (index, point)))
}

/// Shapes for every pixel, or `None` when fewer than two consecutive pixels lit.
pub(crate) fn fit(points: &[Option<Vec2>], placement: &Placement) -> Option<Vec<FixtureElement>> {
    let spacing = spacing(points)?;
    let tolerance = TOLERANCE_SPACINGS * spacing;
    let pieces = match fit_grid(points, tolerance) {
        Some(grid) => vec![grid],
        None => runs(points, spacing)
            .into_iter()
            .flat_map(|run| fit_run(&points[run], spacing, tolerance))
            .collect(),
    };
    let (low, high) = observed(points).fold(
        (Vec2::INFINITY, Vec2::NEG_INFINITY),
        |(low, high), (_, point)| (low.min(point), high.max(point)),
    );
    let diagonal = low.distance(high);
    let scale = match placement.diagonal {
        Some(meters) if diagonal > 0.0 => meters / diagonal,
        _ => placement.spacing / neighbor_spacing(points).unwrap_or(spacing),
    };
    let middle = (low + high) / 2.0;
    let place = |point: Vec2| (point - middle) * scale + placement.center;
    Some(
        pieces
            .into_iter()
            .enumerate()
            .map(|(index, piece)| {
                element(index as u32 + 1, piece, &place, scale, placement.diameter)
            })
            .collect(),
    )
}

/// Index ranges split where consecutive lit pixels are far apart; the unseen
/// pixels between go half to each side.
fn runs(points: &[Option<Vec2>], spacing: f32) -> Vec<Range<usize>> {
    let lit = observed(points).collect::<Vec<_>>();
    let mut starts = vec![0];
    for pair in lit.windows(2) {
        let [(before, from), (after, to)] = [pair[0], pair[1]];
        if from.distance(to) / (after - before) as f32 > GAP_SPACINGS * spacing {
            starts.push(before + 1 + (after - before - 1) / 2);
        }
    }
    starts.push(points.len());
    starts.windows(2).map(|pair| pair[0]..pair[1]).collect()
}

/// Every pixel's position: lit ones as seen, unseen ones between or beyond
/// their lit neighbors. The run has at least one lit pixel.
fn filled(points: &[Option<Vec2>]) -> Vec<Vec2> {
    let lit = observed(points).collect::<Vec<_>>();
    (0..points.len())
        .map(|index| {
            if let Some(point) = points[index] {
                return point;
            }
            let after = lit.partition_point(|&(lit, _)| lit < index);
            let (from, to) = match (after.checked_sub(1), lit.get(after)) {
                (Some(before), Some(_)) => (lit[before], lit[after]),
                (Some(before), None) => (lit[before.saturating_sub(1)], lit[before]),
                (None, _) => (lit[0], lit[1.min(lit.len() - 1)]),
            };
            if from.0 == to.0 {
                return from.1;
            }
            let along = (index as f32 - from.0 as f32) / (to.0 - from.0) as f32;
            from.1.lerp(to.1, along)
        })
        .collect()
}

fn rms(points: &[Option<Vec2>], model: impl Fn(usize) -> Vec2) -> f32 {
    let (sum, count) = observed(points).fold((0.0, 0), |(sum, count), (index, point)| {
        (sum + model(index).distance_squared(point), count + 1)
    });
    (sum / count as f32).sqrt()
}

fn fit_run(points: &[Option<Vec2>], spacing: f32, tolerance: f32) -> Vec<Piece> {
    let count = points.len() as u32;
    if count == 1 {
        return filled(points).into_iter().map(Piece::Pixel).collect();
    }
    let fits = [fit_line, fit_arc]
        .into_iter()
        .find_map(|fit| {
            fit(points).filter(|piece| rms(points, |index| model(piece, index)) <= tolerance)
        })
        .or_else(|| {
            let piece = fit_polyline(points, spacing)?;
            (rms(points, |index| model(&piece, index)) <= tolerance).then_some(piece)
        });
    match fits {
        Some(piece) => vec![piece],
        None => filled(points).into_iter().map(Piece::Pixel).collect(),
    }
}

/// Where a piece puts its `index`th pixel.
fn model(piece: &Piece, index: usize) -> Vec2 {
    let fraction = |count: u32| {
        if count <= 1 {
            0.0
        } else {
            index as f32 / (count - 1) as f32
        }
    };
    match piece {
        Piece::Pixel(point) => *point,
        Piece::Line { start, end, count } => start.lerp(*end, fraction(*count)),
        Piece::Arc {
            center,
            radius,
            start,
            sweep,
            count,
            closed,
        } => {
            let along = if *closed {
                index as f32 / *count as f32
            } else {
                fraction(*count)
            };
            *center + Vec2::from_angle(start + sweep * along) * *radius
        }
        Piece::Polyline { points, count } => along_polyline(points, fraction(*count)),
        Piece::Grid {
            origin,
            across,
            up,
            columns,
            rows,
            axis,
            corner,
            serpentine,
        } => {
            let (x, y) = grid_cell(index as u32, *columns, *rows, *axis, *serpentine);
            let y = if *corner == GridCorner::TopLeft {
                rows - 1 - y
            } else {
                y
            };
            *origin + *across * x as f32 + *up * y as f32
        }
    }
}

/// The column and row of a grid's `index`th pixel from its bottom left, as
/// fixture geometry lays it out.
fn grid_cell(index: u32, columns: u32, rows: u32, axis: GridAxis, serpentine: bool) -> (u32, u32) {
    let minor = match axis {
        GridAxis::Rows => columns,
        GridAxis::Columns => rows,
    };
    let lane = index / minor;
    let along = index % minor;
    let along = if serpentine && lane % 2 == 1 {
        minor - 1 - along
    } else {
        along
    };
    match axis {
        GridAxis::Rows => (along, lane),
        GridAxis::Columns => (lane, along),
    }
}

fn along_polyline(points: &[Vec2], fraction: f32) -> Vec2 {
    let total: f32 = points
        .windows(2)
        .map(|pair| pair[0].distance(pair[1]))
        .sum();
    let mut left = fraction * total;
    for pair in points.windows(2) {
        let length = pair[0].distance(pair[1]);
        if left <= length && length > 0.0 {
            return pair[0].lerp(pair[1], left / length);
        }
        left -= length;
    }
    points.last().copied().unwrap_or(Vec2::ZERO)
}

/// Pixels evenly spaced from a start to an end.
fn fit_line(points: &[Option<Vec2>]) -> Option<Piece> {
    let count = points.len() as u32;
    let along = |index: usize| index as f32 / (count - 1) as f32;
    let lit = observed(points).collect::<Vec<_>>();
    let mean_along = lit.iter().map(|&(index, _)| along(index)).sum::<f32>() / lit.len() as f32;
    let mean = lit.iter().map(|&(_, point)| point).sum::<Vec2>() / lit.len() as f32;
    let (spread, step) = lit
        .iter()
        .fold((0.0, Vec2::ZERO), |(spread, step), &(index, point)| {
            let offset = along(index) - mean_along;
            (spread + offset * offset, step + (point - mean) * offset)
        });
    if spread <= 0.0 {
        return None;
    }
    let direction = step / spread;
    let start = mean - direction * mean_along;
    (direction.length() > 0.0).then_some(Piece::Line {
        start,
        end: start + direction,
        count,
    })
}

/// Pixels evenly spaced round a circle.
fn fit_arc(points: &[Option<Vec2>]) -> Option<Piece> {
    let count = points.len() as u32;
    let lit = observed(points).collect::<Vec<_>>();
    if lit.len() < 3 {
        return None;
    }
    let mean = lit.iter().map(|&(_, point)| point).sum::<Vec2>() / lit.len() as f32;
    // x² + y² + a·x + b·y + c = 0, by least squares about the mean.
    let (mut normal, mut right) = (Mat3::ZERO, Vec3::ZERO);
    for &(_, point) in &lit {
        let point = point - mean;
        let row = Vec3::new(point.x, point.y, 1.0);
        normal += Mat3::from_cols(row * row.x, row * row.y, row * row.z);
        right -= row * point.length_squared();
    }
    if normal.determinant().abs() <= f32::EPSILON {
        return None;
    }
    let [a, b, c] = (normal.inverse() * right).to_array();
    let offset = Vec2::new(-a / 2.0, -b / 2.0);
    let radius = (offset.length_squared() - c).sqrt();
    let center = mean + offset;
    // Angles unwrapped in pixel order, then fitted as start + step·index.
    let mut angles = Vec::with_capacity(lit.len());
    for &(index, point) in &lit {
        let angle = (point - center).to_angle();
        let angle = match angles.last() {
            Some(&(_, before)) => before + wrapped(angle - before),
            None => angle,
        };
        angles.push((index as f32, angle));
    }
    let mean_index = angles.iter().map(|&(index, _)| index).sum::<f32>() / angles.len() as f32;
    let mean_angle = angles.iter().map(|&(_, angle)| angle).sum::<f32>() / angles.len() as f32;
    let (spread, turn) = angles
        .iter()
        .fold((0.0, 0.0), |(spread, turn), &(index, angle)| {
            let offset = index - mean_index;
            (
                spread + offset * offset,
                turn + offset * (angle - mean_angle),
            )
        });
    let step = turn / spread;
    let sweep = step * count as f32;
    if !(radius.is_finite() && radius > 0.0) {
        return None;
    }
    if (sweep.abs() - TAU).abs() <= CLOSED_SHARE * TAU {
        let step = TAU.copysign(sweep) / count as f32;
        let start = angles
            .iter()
            .map(|&(index, angle)| Vec2::from_angle(angle - step * index))
            .sum::<Vec2>()
            .to_angle();
        return Some(Piece::Arc {
            center,
            radius,
            start,
            sweep: TAU.copysign(sweep),
            count,
            closed: true,
        });
    }
    let sweep = step * (count - 1) as f32;
    (sweep.abs() <= TAU).then_some(Piece::Arc {
        center,
        radius,
        start: mean_angle - step * mean_index,
        sweep,
        count,
        closed: false,
    })
}

fn wrapped(angle: f32) -> f32 {
    (angle + TAU / 2.0).rem_euclid(TAU) - TAU / 2.0
}

/// Pixels evenly spaced along the path's corners.
fn fit_polyline(points: &[Option<Vec2>], spacing: f32) -> Option<Piece> {
    let path = filled(points);
    let mut keep = vec![false; path.len()];
    keep[0] = true;
    keep[path.len() - 1] = true;
    simplify(
        &path,
        0,
        path.len() - 1,
        SIMPLIFY_SPACINGS * spacing,
        &mut keep,
    );
    let corners = path
        .iter()
        .zip(&keep)
        .filter_map(|(point, &keep)| keep.then_some(*point))
        .collect::<Vec<_>>();
    (corners.windows(2).any(|pair| pair[0] != pair[1])).then_some(Piece::Polyline {
        points: corners,
        count: points.len() as u32,
    })
}

/// Douglas–Peucker: keep the point farthest from each chord while it is
/// farther than `tolerance`.
fn simplify(path: &[Vec2], first: usize, last: usize, tolerance: f32, keep: &mut [bool]) {
    let (from, to) = (path[first], path[last]);
    let chord = to - from;
    let farthest = (first + 1..last)
        .map(|index| {
            let offset = path[index] - from;
            let distance = if chord.length_squared() > 0.0 {
                chord.perp_dot(offset).abs() / chord.length()
            } else {
                offset.length()
            };
            (index, distance)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((index, distance)) = farthest
        && distance > tolerance
    {
        keep[index] = true;
        simplify(path, first, index, tolerance, keep);
        simplify(path, index, last, tolerance, keep);
    }
}

/// The whole fixture as a grid of evenly spaced rows and columns, in any of
/// the orders fixture geometry lays grids out.
fn fit_grid(points: &[Option<Vec2>], tolerance: f32) -> Option<Piece> {
    let count = points.len() as u32;
    let lit = observed(points).collect::<Vec<_>>();
    let mut fits: Vec<(f32, f32, Piece)> = Vec::new();
    for columns in (2..=count / 2).filter(|&columns| count.is_multiple_of(columns)) {
        let rows = count / columns;
        if rows < 2 {
            continue;
        }
        for axis in [GridAxis::Rows, GridAxis::Columns] {
            for serpentine in [false, true] {
                // point = origin + across·column + up·row, by least squares.
                let (mut normal, mut right_x, mut right_y) = (Mat3::ZERO, Vec3::ZERO, Vec3::ZERO);
                for &(index, point) in &lit {
                    let (x, y) = grid_cell(index as u32, columns, rows, axis, serpentine);
                    let row = Vec3::new(x as f32, y as f32, 1.0);
                    normal += Mat3::from_cols(row * row.x, row * row.y, row * row.z);
                    right_x += row * point.x;
                    right_y += row * point.y;
                }
                if normal.determinant().abs() <= f32::EPSILON {
                    continue;
                }
                let inverse = normal.inverse();
                let (solved_x, solved_y) = (inverse * right_x, inverse * right_y);
                let across = Vec2::new(solved_x.x, solved_y.x);
                let up = Vec2::new(solved_x.y, solved_y.y);
                if across.length() == 0.0
                    || up.length() == 0.0
                    || across.dot(up).abs() > SQUARE_COSINE * across.length() * up.length()
                {
                    continue;
                }
                let origin = Vec2::new(solved_x.z, solved_y.z);
                // A mirrored grid starts from its top left instead.
                let (origin, up, corner) = if across.perp_dot(up) < 0.0 {
                    (origin + up * (rows - 1) as f32, -up, GridCorner::TopLeft)
                } else {
                    (origin, up, GridCorner::BottomLeft)
                };
                let piece = Piece::Grid {
                    origin,
                    across,
                    up,
                    columns,
                    rows,
                    axis,
                    corner,
                    serpentine,
                };
                let error = rms(points, |index| model(&piece, index));
                if error <= tolerance {
                    fits.push((error, wrapped(across.to_angle()).abs(), piece));
                }
            }
        }
    }
    // The same grid fits in several orders; take the most upright of the
    // closest fits.
    let closest = fits.iter().map(|fit| fit.0).fold(f32::INFINITY, f32::min);
    fits.into_iter()
        .filter(|fit| fit.0 <= closest * EQUAL_FIT_SHARE + f32::EPSILON)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(_, _, piece)| piece)
}

fn point3(point: Vec2) -> Point3 {
    Point3 {
        x: Distance::from_meters(point.x),
        y: Distance::from_meters(point.y),
        z: Distance::from_meters(0.0),
    }
}

/// The fixture element for a piece, placed and scaled into meters.
fn element(
    id: u32,
    piece: Piece,
    place: &impl Fn(Vec2) -> Vec2,
    scale: f32,
    diameter: DistanceSpan,
) -> FixtureElement {
    let at = |position: Vec2, degrees: f32| FixtureTransform {
        position: point3(place(position)),
        rotation: Rotation3 {
            x: 0.0,
            y: 0.0,
            z: degrees,
        },
        ..Default::default()
    };
    let (kind, transform, shape) = match piece {
        Piece::Pixel(point) => ("pixel", at(point, 0.0), FixtureShape::Pixel),
        Piece::Line { start, end, count } => (
            "line",
            at(start, (end - start).to_angle().to_degrees()),
            FixtureShape::Line {
                length: start.distance(end) * scale,
                count,
            },
        ),
        Piece::Arc {
            center,
            radius,
            start,
            sweep,
            count,
            closed,
        } => (
            "arc",
            at(center, 0.0),
            FixtureShape::Arc {
                radius: radius * scale,
                start_degrees: start.to_degrees(),
                sweep_degrees: if closed {
                    360f32.copysign(sweep)
                } else {
                    sweep.to_degrees()
                },
                count,
                closed,
            },
        ),
        Piece::Polyline { points, count } => {
            let first = points[0];
            (
                "polyline",
                at(first, 0.0),
                FixtureShape::Polyline {
                    points: points
                        .iter()
                        .map(|&point| point3((point - first) * scale))
                        .collect(),
                    count,
                },
            )
        }
        Piece::Grid {
            origin,
            across,
            up,
            columns,
            rows,
            axis,
            corner,
            serpentine,
        } => (
            "grid",
            at(origin, across.to_angle().to_degrees()),
            FixtureShape::Grid {
                columns,
                rows,
                width: across.length() * (columns - 1) as f32 * scale,
                height: up.length() * (rows - 1) as f32 * scale,
                axis,
                corner,
                serpentine,
            },
        ),
    };
    FixtureElement {
        id: FixtureElementId(id),
        name: donder_language::object_name(&format!("{kind}_{id}")),
        transform,
        diameter,
        reverse: false,
        shape,
    }
}
