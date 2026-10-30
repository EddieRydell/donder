//! Poly Line models: one string of lights along a path of straight and
//! curved segments, optionally hanging in drops.

use super::{Common, Local, Node, Span, Strings, account};
use crate::placement::{Placement, RenderSize};
use crate::read::{Name, Reader, Table, Use, strtod, strtol};
use crate::report::SkipReason;
use glam::{DVec3, Mat4, Vec3, Vec4};

const POLY_LINE: Table = &[
    (Name::Exact("LightsPerNode"), Use::Modeled),
    (Name::Exact("NodesPerString"), Use::Modeled),
    (Name::Exact("parm2"), Use::Modeled),
    (Name::Exact("parm3"), Use::Modeled),
    (Name::Exact("NumPoints"), Use::Modeled),
    (Name::Exact("PointData"), Use::Modeled),
    (Name::Exact("cPointData"), Use::Modeled),
    (Name::Exact("PolyStrings"), Use::RefusedUnlessDefault),
    (Name::Exact("DropPattern"), Use::Modeled),
    (Name::Exact("AlternateNodes"), Use::Modeled),
    (Name::Exact("ModelHeight"), Use::Modeled),
    (Name::Numbered("Seg"), Use::Modeled),
    (Name::Numbered("Corner"), Use::Modeled),
    (Name::Exact("ScaleX"), Use::Modeled),
    (Name::Exact("ScaleY"), Use::Modeled),
    (Name::Exact("ScaleZ"), Use::Modeled),
    (
        Name::Numbered("PolyNode"),
        Use::Irrelevant("a string's first node, read only with several strings, which are refused"),
    ),
    (
        Name::Exact("SegsExpanded"),
        Use::Irrelevant("whether xLights' property grid shows the segments"),
    ),
    (
        Name::Exact("RotateX"),
        Use::Irrelevant("a poly line is turned by its points"),
    ),
    (
        Name::Exact("RotateY"),
        Use::Irrelevant("a poly line is turned by its points"),
    ),
    (
        Name::Exact("RotateZ"),
        Use::Irrelevant("a poly line is turned by its points"),
    ),
];

/// xLights: `BezierCurve3D`'s default `steps`.
const CURVE_STEPS: f32 = 25.0;

/// A cubic Bezier segment, sampled as xLights samples it. xLights:
/// `BezierCurveCubic3D` with the positioning `PolyLineModel::InitModel`
/// gives it (a scale of 100 at the origin).
struct Curve {
    points: Vec<Vec3>,
    lengths: Vec<f32>,
    matrices: Vec<Mat4>,
}

impl Curve {
    /// xLights: `BezierCurveCubic3D::UpdatePoints` and
    /// `BezierCurve3D::UpdateMatrices`.
    fn new(p0: Vec3, p1: Vec3, cp0: Vec3, cp1: Vec3) -> Self {
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let mix = |a: Vec3, b: Vec3, t: f32| {
            Vec3::new(lerp(a.x, b.x, t), lerp(a.y, b.y, t), lerp(a.z, b.z, t))
        };
        let mut points = Vec::new();
        let step = 1.0 / CURVE_STEPS;
        let mut t = 0f32;
        while t < 1.0 {
            let (a, b, c) = (mix(p0, cp0, t), mix(cp0, cp1, t), mix(cp1, p1, t));
            let (d, e) = (mix(a, b, t), mix(b, c, t));
            points.push(mix(d, e, t));
            t += step;
        }
        points.push(p1);
        let scale = 100f32;
        let lengths = points
            .windows(2)
            .map(|pair| (pair[1] * scale - pair[0] * scale).length() / scale)
            .collect();
        Self {
            points,
            lengths,
            matrices: Vec::new(),
        }
    }

    fn length(&self) -> f32 {
        self.lengths.iter().sum()
    }

    fn segment_length(&self, segment: i32) -> f32 {
        usize::try_from(segment)
            .ok()
            .and_then(|segment| self.lengths.get(segment))
            .copied()
            .unwrap_or(0.0)
    }

    fn segments(&self) -> i32 {
        self.points.len() as i32 - 1
    }

    /// xLights: `BezierCurve3D::check_min_max`.
    fn bounds(&self, min: &mut Vec3, max: &mut Vec3) {
        for point in &self.points {
            *min = min.min(*point);
            *max = max.max(*point);
        }
    }

    /// xLights: `BezierCurve3D::CreateNormalizedMatrix`.
    fn normalize(&mut self, min: Vec3, max: Vec3) {
        let delta = max - min;
        let norm = |value: f32, min: f32, delta: f32| {
            if delta == 0.0 {
                0.0
            } else {
                (value - min) / delta
            }
        };
        self.matrices = self
            .points
            .windows(2)
            .map(|pair| {
                let p = |point: Vec3| {
                    Vec3::new(
                        norm(point.x, min.x, delta.x),
                        norm(point.y, min.y, delta.y),
                        norm(point.z, min.z, delta.z),
                    )
                };
                let (from, to) = (p(pair[0]), p(pair[1]));
                segment_matrix(from, to, Vec3::new((to - from).length(), 1.0, 1.0))
            })
            .collect();
    }
}

/// `translate(from) * rotationMatrixFromXAxisToVector(to - from) *
/// scale(scale)`, the matrix xLights gives each piece of a path.
fn segment_matrix(from: Vec3, to: Vec3, scale: Vec3) -> Mat4 {
    Mat4::from_translation(from)
        * crate::placement::x_axis_to_vector(to - from)
        * Mat4::from_scale(scale)
}

/// One point of the path, as xLights keeps it. xLights: `xlPolyPoint`.
struct PathPoint {
    position: Vec3,
    curve: Option<Curve>,
    length: f32,
    matrix: Mat4,
}

struct Settings {
    lights_per_node: i32,
    total_lights: i32,
    points: Vec<PathPoint>,
    drops: Vec<i32>,
    alternate: bool,
    height: f32,
    /// Lights per segment, when they are set per segment.
    segments: Option<Vec<i32>>,
    lead: Vec<f32>,
    trail: Vec<f32>,
    scale: Vec3,
    world: Vec3,
}

/// xLights: `strtof`, as `SetDataFromString` reads each coordinate.
fn strtof(text: &str) -> Result<f32, SkipReason> {
    strtod(text)
        .map(|value| value as f32)
        .filter(|value| value.is_finite())
        .ok_or(SkipReason::Unplaceable("a point that is not a number"))
}

/// xLights: `DeserializePolyLine`, `DeserializePolyPointScreenLocationAttributes`,
/// `SetDataFromString`, `SetCurveDataFromString` and `ParseDropSizes`.
fn settings(reader: &Reader) -> Result<Settings, SkipReason> {
    if reader.as_int("PolyStrings", 1) != 1 {
        return Err(SkipReason::Setting("number of strings"));
    }
    let count = reader.as_int("NumPoints", 2).max(2) as usize;
    super::lights(count as u64, 1)?;
    let mut values: Vec<&str> = reader
        .as_string("PointData", "0.0, 0.0, 0.0, 0.0, 0.0, 0.0")
        .split(',')
        .collect();
    while values.len() < count * 3 {
        values.push("0.0");
    }
    let mut points = (0..count)
        .map(|index| {
            Ok(PathPoint {
                position: Vec3::new(
                    strtof(values[index * 3])?,
                    strtof(values[index * 3 + 1])?,
                    strtof(values[index * 3 + 2])?,
                ),
                curve: None,
                length: 0.0,
                matrix: Mat4::IDENTITY,
            })
        })
        .collect::<Result<Vec<_>, SkipReason>>()?;
    let curves: Vec<&str> = reader.as_string("cPointData", "").split(',').collect();
    for curve in curves.as_chunks::<7>().0 {
        let segment = strtol(curve[0]);
        let segment = usize::try_from(segment)
            .ok()
            .filter(|segment| segment + 1 < count)
            .ok_or(SkipReason::Unplaceable(
                "a curve on a segment the path does not have",
            ))?;
        let cp0 = Vec3::new(strtof(curve[1])?, strtof(curve[2])?, strtof(curve[3])?);
        let cp1 = Vec3::new(strtof(curve[4])?, strtof(curve[5])?, strtof(curve[6])?);
        points[segment].curve = Some(Curve::new(
            points[segment].position,
            points[segment + 1].position,
            cp0,
            cp1,
        ));
    }
    let drops: Vec<i32> = reader
        .as_string("DropPattern", "1")
        .split(',')
        .map(|item| match strtol(item) as i32 {
            0 => Ok(1),
            size => super::lights(u64::from(size.unsigned_abs()), 1).map(|()| size),
        })
        .collect::<Result<_, _>>()?;
    let segments = count - 1;
    let auto = !reader.has("Seg1");
    let corner = |index: usize| reader.as_string(&format!("Corner{}", index + 1), "Neither");
    let lead_of = |corner: &str| match corner {
        "Leading Segment" => 1.0,
        "Trailing Segment" => 0.0,
        _ => 0.5,
    };
    let trail_of = |corner: &str| match corner {
        "Leading Segment" => 0.0,
        "Trailing Segment" => 1.0,
        _ => 0.5,
    };
    let (mut lead, mut trail) = (vec![0.0; segments], vec![0.0; segments]);
    for index in 0..=segments {
        let corner = corner(index);
        if index < segments {
            lead[index] = lead_of(corner);
        }
        if index > 0 {
            trail[index - 1] = trail_of(corner);
        }
    }
    let scale = |name: &str| -> Result<f32, SkipReason> {
        let value = reader.as_float(name, 1.0)?;
        Ok(if value <= 0.0 { 1.0 } else { value })
    };
    Ok(Settings {
        lights_per_node: reader.parm("LightsPerNode", "parm3", "1"),
        total_lights: reader.parm("NodesPerString", "parm2", "0"),
        points,
        drops,
        alternate: reader.equals("AlternateNodes", "false", "true"),
        height: reader.as_float("ModelHeight", 1.0)?,
        segments: (!auto).then(|| {
            (0..segments)
                .map(|index| reader.as_int(&format!("Seg{}", index + 1), 0))
                .collect()
        }),
        lead,
        trail,
        scale: Vec3::new(scale("ScaleX")?, scale("ScaleY")?, scale("ScaleZ")?),
        world: Vec3::new(
            reader.as_float("WorldPosX", 0.0)?,
            reader.as_float("WorldPosY", 0.0)?,
            reader.as_float("WorldPosZ", 0.0)?,
        ),
    })
}

/// The lights of the model, and how many drop positions they hang from.
/// xLights: the start of `PolyLineModel::InitModel`.
fn light_count(settings: &Settings) -> Result<(u32, u32), SkipReason> {
    let drops = &settings.drops;
    let (mut lights, mut positions) = (0u64, 0u64);
    // Every drop has a light, so stopping past the limit bounds the loops.
    let mut sizes = drops.iter().map(|size| size.unsigned_abs()).cycle();
    let mut next = || {
        let size = sizes.next().unwrap_or(1);
        lights += u64::from(size);
        positions += 1;
        super::lights(lights, 1).map(|()| size)
    };
    match &settings.segments {
        Some(segments) => {
            for &size in segments {
                for _ in 0..size.max(0) {
                    next()?;
                }
            }
        }
        None => {
            let mut left = i64::from(settings.total_lights);
            while left > 0 {
                left -= i64::from(next()?);
            }
        }
    }
    Ok((lights as u32, positions as u32))
}

pub(super) fn poly_line(
    reader: &Reader,
    common: &Common,
) -> (Result<Span, SkipReason>, Result<Local, SkipReason>) {
    let settings = match settings(reader) {
        Ok(settings) => settings,
        Err(reason) => return (Err(reason.clone()), Err(reason)),
    };
    let lights = match light_count(&settings) {
        Ok((lights, _)) => lights,
        Err(reason) => return (Err(reason.clone()), Err(reason)),
    };
    // xLights: PolyLineModel::NodesPerString, the model's one string.
    let strings = match Strings::uniform(1, lights as i32, 1) {
        Ok(strings) => strings,
        Err(reason) => return (Err(reason.clone()), Err(reason)),
    };
    let span = Ok(strings.span(&common.string_type));
    (span, layout(reader, common, settings, &strings))
}

/// xLights: `PolyLineModel::InitModel`, `DistributeLightsEvenly`,
/// `DistributeLightsAcrossIndivSegments` and `DistributeLightsAcrossSegment`,
/// for a model of individual pixels on one string.
fn layout(
    reader: &Reader,
    common: &Common,
    mut settings: Settings,
    strings: &Strings,
) -> Result<Local, SkipReason> {
    account(reader, &[], &[POLY_LINE], super::NO_CHILDREN)?;
    if settings.lights_per_node < 1 {
        return Err(SkipReason::Unplaceable("each node has no lights"));
    }
    let (num_lights, drop_points) = light_count(&settings)?;
    if num_lights == 0 {
        return Err(SkipReason::NoPixels);
    }
    super::lights(u64::from(num_lights), settings.lights_per_node as u64)?;
    let per_node = settings.lights_per_node as usize;
    let channels = common.string_type.channels;
    let drops = settings.drops.clone();
    let max_h = drops
        .iter()
        .map(|drop| drop.unsigned_abs())
        .max()
        .unwrap_or(1)
        .max(1) as i32;
    let count = settings.points.len();

    // Bounds of the path, its curves included.
    let mut min = Vec3::splat(100_000.0);
    let mut max = Vec3::ZERO;
    for point in &settings.points {
        min = min.min(point.position);
        max = max.max(point.position);
        if let Some(curve) = &point.curve {
            curve.bounds(&mut min, &mut max);
        }
    }
    let delta = max - min;
    let mut total_length = 0f32;
    for index in 0..count - 1 {
        let (from, to) = (
            settings.points[index].position,
            settings.points[index + 1].position,
        );
        if settings.segments.is_none() {
            if let Some(curve) = &settings.points[index].curve {
                total_length += curve.length();
            } else {
                let length = (to - from).length();
                settings.points[index].length = length;
                total_length += length;
            }
        }
        let norm = |value: f32, min: f32, delta: f32| {
            if delta == 0.0 {
                0.0
            } else {
                (value - min) / delta
            }
        };
        let normalize = |point: Vec3| {
            Vec3::new(
                norm(point.x, min.x, delta.x),
                if delta.y.abs() < 0.1 {
                    0.0
                } else {
                    (point.y - min.y) / delta.y
                },
                norm(point.z, min.z, delta.z),
            )
        };
        let (a, b) = (normalize(from), normalize(to));
        let flat = count == 2 && settings.points[index].curve.is_none();
        let squash = if flat { 0.0 } else { 1.0 };
        settings.points[index].matrix =
            segment_matrix(a, b, Vec3::new((b - a).length(), squash, squash));
        if let Some(curve) = &mut settings.points[index].curve {
            curve.normalize(min, max);
        }
    }

    // Each node's channel and buffer position.
    struct Light {
        buf_x: i32,
        buf_y: i32,
        screen: Vec3,
    }
    let mut nodes: Vec<(u32, Vec<Light>)> = (0..num_lights).map(|_| (0, Vec::new())).collect();
    let left_to_right = common.left_to_right;
    let per_string = strings.nodes_per_string as i64;
    let mut chan: i64 = 0;
    let mut first = true;
    let (mut y, mut drop, mut width, mut node, mut coord) = (0i32, 0usize, 0i32, 0usize, 0usize);
    let mut up = drops[0] < 0;
    let mut in_drop = drops[0].abs();
    let mut in_drop_last = in_drop;
    let mut remaining = u64::from(num_lights) * per_node as u64;
    while remaining > 0 {
        if coord >= per_node {
            node += 1;
            coord = 0;
            chan += i64::from(channels);
        }
        while y >= drops[drop].abs() {
            width += 1;
            y = 0;
            drop = (drop + 1) % drops.len();
            in_drop = drops[drop].abs();
            if !left_to_right && coord == 0 {
                chan -= i64::from(in_drop_last + in_drop) * i64::from(channels);
            }
            in_drop_last = in_drop;
            up = drops[drop] < 0;
        }
        if first {
            first = false;
            chan = 0;
            if !left_to_right && coord == 0 {
                chan += (per_string - i64::from(in_drop)) * i64::from(channels);
            }
        }
        nodes[node].0 = u32::try_from(chan)
            .map_err(|_| SkipReason::Unplaceable("a node before the model's first channel"))?;
        let buf_y = if settings.alternate {
            let half = y < (in_drop + 1) / 2;
            let row = if half {
                2 * y
            } else {
                (in_drop - (y + 1)) * 2 + 1
            };
            if up { row } else { max_h - 1 - row }
        } else if up {
            y
        } else {
            max_h - y - 1
        };
        let screen_y = if settings.alternate && up {
            -(max_h - 1)
                + if y < (in_drop + 1) / 2 {
                    2 * y
                } else {
                    (in_drop - (y + 1)) * 2 + 1
                }
        } else {
            buf_y
        };
        nodes[node].1.push(Light {
            buf_x: width,
            buf_y,
            screen: Vec3::new(width as f32, screen_y as f32, 0.0),
        });
        remaining -= 1;
        coord += 1;
        if coord == per_node {
            y += 1;
        }
    }

    let render_height = max_h as f32;
    let model_height = f64::from(delta.y).max(f64::from(render_height));
    let height = (f64::from(settings.height * 10.0) / model_height) as f32;
    let find = |nodes: &[(u32, Vec<Light>)], x: i32, y: i32| {
        nodes.iter().position(|(_, lights)| {
            lights
                .first()
                .is_some_and(|light| light.buf_x == x && light.buf_y == y)
        })
    };
    let at = |point: &PathPoint, sub: i32, t: f32| -> Vec3 {
        let matrix = match &point.curve {
            Some(curve) => curve.matrices.get(sub as usize).copied(),
            None => Some(point.matrix),
        };
        matrix.map_or(point.position, |matrix| {
            (matrix * Vec4::new(t, 0.0, 0.0, 1.0)).truncate()
        })
    };
    let using_icicles = max_h > 1;
    let coord_offset = if using_icicles {
        1.0 / per_node as f32
    } else {
        0.0
    };
    let place =
        |nodes: &mut [(u32, Vec<Light>)], node: usize, c: usize, v: Vec3, z: u32, up: bool| {
            let drop_y = (z as f32 + c as f32 * coord_offset) * height;
            nodes[node].1[c].screen =
                Vec3::new(v.x, if up { v.y + drop_y } else { v.y - drop_y }, v.z);
        };
    match &settings.segments {
        None => {
            // xLights: DistributeLightsEvenly.
            let divisor = drop_points as f32 * if using_icicles { 1.0 } else { per_node as f32 };
            let offset = if divisor > 0.0 {
                total_length / divisor
            } else {
                0.0
            };
            let mut current = offset / 2.0;
            let (mut segment, mut sub) = (0usize, 0i32);
            let mut seg_start = current;
            let length_of = |point: &PathPoint, sub: i32| {
                point
                    .curve
                    .as_ref()
                    .map_or(point.length, |curve| curve.segment_length(sub))
            };
            let mut segment_length = length_of(&settings.points[0], 0);
            let mut seg_end = seg_start + segment_length;
            let (mut c, mut xpos, mut drop) = (0usize, 0i32, 0usize);
            let to_distribute = u64::from(num_lights) * per_node as u64;
            let mut placed = 0u64;
            let mut rounds = 0u64;
            while placed < to_distribute {
                // xLights loops forever when a drop's node is not found.
                rounds += 1;
                if rounds > to_distribute * 4 + 1000 {
                    return Err(SkipReason::Unplaceable(
                        "drops whose nodes xLights cannot find",
                    ));
                }
                while current > seg_end {
                    sub += 1;
                    let point = &settings.points[segment];
                    if let Some(curve) = point.curve.as_ref().filter(|curve| sub < curve.segments())
                    {
                        seg_start = seg_end;
                        segment_length = curve.segment_length(sub);
                        seg_end = seg_start + segment_length;
                    } else if segment == count - 2 {
                        seg_end += 0.0001;
                    } else {
                        sub = 0;
                        segment += 1;
                        seg_start = seg_end;
                        segment_length = length_of(&settings.points[segment], 0);
                        seg_end = seg_start + segment_length;
                    }
                }
                let t = if segment_length > 0.0 {
                    (current - seg_start) / segment_length
                } else {
                    0.0
                };
                let v = at(&settings.points[segment], sub, t);
                let up = drops[drop] < 0;
                let in_this = drops[drop].unsigned_abs();
                drop += 1;
                for z in 0..in_this {
                    let row = if up { z as i32 } else { max_h - z as i32 - 1 };
                    if let Some(found) = find(&nodes, xpos, row) {
                        if using_icicles {
                            for coord in 0..per_node {
                                place(&mut nodes, found, coord, v, z, up);
                                placed += 1;
                            }
                            c = per_node;
                        } else {
                            place(&mut nodes, found, c, v, z, up);
                            placed += 1;
                            c += 1;
                        }
                    }
                    if c == per_node {
                        c = 0;
                    }
                }
                drop %= drops.len();
                current += offset;
                if c == 0 {
                    xpos += 1;
                }
            }
        }
        Some(sizes) => {
            // xLights: DistributeLightsAcrossIndivSegments.
            let (mut drop, mut xpos) = (0usize, 0i32);
            for (segment, &size) in sizes.iter().enumerate() {
                let mut lights = 0u64;
                let mut index = drop;
                for _ in 0..size.max(0) {
                    lights += u64::from(drops[index].unsigned_abs());
                    index = (index + 1) % drops.len();
                }
                let to_distribute = lights * per_node as u64;
                let point = &settings.points[segment];
                let total = point.curve.as_ref().map_or(size as f32, Curve::length);
                let gaps = if using_icicles {
                    settings.lead[segment] + settings.trail[segment] + size as f32 - 1.0
                } else {
                    settings.lead[segment] + settings.trail[segment] + to_distribute as f32 - 1.0
                };
                let offset = if gaps > 0.0 { total / gaps } else { 0.0 };
                let mut current = settings.lead[segment] * offset;
                let mut c = 0usize;
                let mut sub = 0i32;
                let mut seg_start = 0f32;
                let mut segment_length = point
                    .curve
                    .as_ref()
                    .map_or(size as f32, |curve| curve.segment_length(0));
                let mut seg_end = seg_start + segment_length;
                let mut placed = 0u64;
                let mut rounds = 0u64;
                while placed < to_distribute {
                    // xLights loops forever when a drop's node is not found.
                    rounds += 1;
                    if rounds > to_distribute * 4 + 1000 {
                        return Err(SkipReason::Unplaceable(
                            "drops whose nodes xLights cannot find",
                        ));
                    }
                    let up = drops[drop] < 0;
                    let in_this = drops[drop].unsigned_abs();
                    while current > seg_end {
                        let next = point
                            .curve
                            .as_ref()
                            .map_or(1.0, |curve| curve.segment_length(sub + 1));
                        if next <= 0.0 {
                            break;
                        }
                        sub += 1;
                        seg_start = seg_end;
                        segment_length = next;
                        seg_end = seg_start + segment_length;
                    }
                    let v = match &point.curve {
                        Some(_) => {
                            let t = if segment_length > 0.0 {
                                (current - seg_start) / segment_length
                            } else {
                                0.0
                            };
                            at(point, sub, t)
                        }
                        None => {
                            let t = if size as f32 > 0.0 {
                                current / size as f32
                            } else {
                                0.0
                            };
                            at(point, 0, t)
                        }
                    };
                    for z in 0..in_this {
                        if let Some(found) = find(&nodes, xpos, max_h - z as i32 - 1) {
                            if using_icicles {
                                for coord in 0..per_node {
                                    place(&mut nodes, found, coord, v, z, up);
                                    placed += 1;
                                }
                                c = per_node;
                            } else {
                                place(&mut nodes, found, c, v, z, up);
                                placed += 1;
                                c += 1;
                            }
                        }
                        if c == per_node {
                            c = 0;
                        }
                    }
                    drop = (drop + 1) % drops.len();
                    current += offset;
                    if c == 0 {
                        xpos += 1;
                    }
                }
            }
        }
    }

    // xLights: PolyPointScreenLocation::PrepareToDraw's main matrix.
    let scale = settings.scale;
    let mut y_scale = delta.y * scale.y;
    if render_height > 1.0 && delta.y < render_height {
        y_scale = render_height;
    }
    let world = settings.world;
    let transform = Mat4::from_translation(min * scale + world)
        * Mat4::from_scale(Vec3::new(delta.x * scale.x, y_scale, delta.z * scale.z));
    let nodes: Vec<Node> = nodes
        .into_iter()
        .map(|(channel, lights)| Node {
            channel,
            lights: lights
                .iter()
                .map(|light| {
                    DVec3::new(
                        f64::from(light.screen.x),
                        f64::from(light.screen.y),
                        f64::from(light.screen.z),
                    )
                })
                .collect(),
            arc: None,
        })
        .collect();
    if nodes
        .iter()
        .flat_map(|node| &node.lights)
        .any(|light| !light.is_finite())
    {
        return Err(SkipReason::Unplaceable("a segment with no length"));
    }
    Ok(Local {
        nodes,
        ellipses: Vec::new(),
        size: RenderSize { width: 1.0 },
        placement: Placement::Matrix(transform),
    })
}
