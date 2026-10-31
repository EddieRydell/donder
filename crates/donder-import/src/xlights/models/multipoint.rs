//! MultiPoint models: a light at each of a list of points.

use super::{Common, Local, Node, Span, Strings, account};
use crate::report::SkipReason;
use crate::xlights::placement::{Placement, RenderSize};
use crate::xlights::read::{Name, Reader, Table, Use, strtod};
use glam::{DVec3, Mat4, Vec3};

const MULTI_POINT: Table = &[
    (Name::Exact("NumPoints"), Use::Modeled),
    (Name::Exact("PointData"), Use::Modeled),
    (Name::Exact("MultiStrings"), Use::RefusedUnlessDefault),
    (Name::Exact("ScaleX"), Use::Modeled),
    (Name::Exact("ScaleY"), Use::Modeled),
    (Name::Exact("ScaleZ"), Use::Modeled),
    (
        Name::Exact("ModelHeight"),
        Use::Irrelevant("not used to place a MultiPoint's lights (MultiPointModel::InitModel)"),
    ),
    (
        Name::Numbered("MultiNode"),
        Use::Irrelevant("a string's first node, read only with several strings, which are refused"),
    ),
    (
        Name::Exact("cPointData"),
        Use::Irrelevant("a MultiPoint has no curves"),
    ),
    (
        Name::Exact("RotateX"),
        Use::Irrelevant("a MultiPoint is placed by its points"),
    ),
    (
        Name::Exact("RotateY"),
        Use::Irrelevant("a MultiPoint is placed by its points"),
    ),
    (
        Name::Exact("RotateZ"),
        Use::Irrelevant("a MultiPoint is placed by its points"),
    ),
];

/// xLights: `DeserializeMultiPoint`, `DeserializePolyPointScreenLocationAttributes`
/// with at least one point, and `SetDataFromString`.
fn points(reader: &Reader) -> Result<Vec<Vec3>, SkipReason> {
    let count = reader.as_int("NumPoints", 2).max(1) as usize;
    super::lights(count as u64, 1)?;
    let mut values: Vec<&str> = reader
        .as_string("PointData", "0.0, 0.0, 0.0, 0.0, 0.0, 0.0")
        .split(',')
        .collect();
    while values.len() < count * 3 {
        values.push("0.0");
    }
    let number = |text: &str| {
        strtod(text)
            .map(|value| value as f32)
            .filter(|value| value.is_finite())
            .ok_or(SkipReason::Unplaceable("a point that is not a number"))
    };
    (0..count)
        .map(|index| {
            Ok(Vec3::new(
                number(values[index * 3])?,
                number(values[index * 3 + 1])?,
                number(values[index * 3 + 2])?,
            ))
        })
        .collect()
}

pub(super) fn multi_point(
    reader: &Reader,
    common: &Common,
) -> (Result<Span, SkipReason>, Result<Local, SkipReason>) {
    let points = match points(reader) {
        Ok(points) => points,
        Err(reason) => return (Err(reason.clone()), Err(reason)),
    };
    let strings = match Strings::uniform(1, points.len() as i32, 1) {
        Ok(strings) => strings,
        Err(reason) => return (Err(reason.clone()), Err(reason)),
    };
    let span = Ok(strings.span(&common.string_type));
    (span, layout(reader, common, &points))
}

/// xLights: `MultiPointModel::InitModel` and `InitLine`, and the
/// `PolyPointScreenLocation` main matrix: each point normalized into the
/// points' bounds, then stretched back over them.
fn layout(reader: &Reader, common: &Common, points: &[Vec3]) -> Result<Local, SkipReason> {
    account(reader, &[], &[MULTI_POINT], super::NO_CHILDREN)?;
    if reader.as_int("MultiStrings", 1) != 1 {
        return Err(SkipReason::Setting("number of strings"));
    }
    // xLights: a right to left MultiPoint counts its channels down from
    // one node past its start (its NodesPerString is 1).
    if !common.left_to_right && points.len() > 1 {
        return Err(SkipReason::Setting("right to left numbering"));
    }
    let mut min = Vec3::splat(100_000.0);
    let mut max = Vec3::ZERO;
    for point in points {
        min = min.min(*point);
        max = max.max(*point);
    }
    let delta = max - min;
    let norm = |value: f32, min: f32, delta: f32| {
        if delta == 0.0 {
            0.0
        } else {
            (value - min) / delta
        }
    };
    let channels = common.string_type.channels;
    let nodes: Vec<Node> = if points.len() > 1 {
        points
            .iter()
            .enumerate()
            .map(|(index, point)| Node {
                channel: index as u32 * channels,
                lights: vec![DVec3::new(
                    f64::from(norm(point.x, min.x, delta.x)),
                    f64::from(norm(point.y, min.y, delta.y)),
                    f64::from(norm(point.z, min.z, delta.z)),
                )],
                arc: None,
            })
            .collect()
    } else {
        vec![Node {
            channel: 0,
            lights: vec![DVec3::new(0.5, 0.0, 0.0)],
            arc: None,
        }]
    };
    let scale = |name: &str| -> Result<f32, SkipReason> {
        let value = reader.as_float(name, 1.0)?;
        Ok(if value <= 0.0 { 1.0 } else { value })
    };
    let scale = Vec3::new(scale("ScaleX")?, scale("ScaleY")?, scale("ScaleZ")?);
    let world = Vec3::new(
        reader.as_float("WorldPosX", 0.0)?,
        reader.as_float("WorldPosY", 0.0)?,
        reader.as_float("WorldPosZ", 0.0)?,
    );
    // xLights: PolyPointScreenLocation::PrepareToDraw, with the render
    // height of 1 MultiPointModel::InitModel sets. Its bounds come from the
    // segments between points, so a single point leaves them at their
    // starting values.
    let (low, high) = if points.len() > 1 {
        (min, max)
    } else {
        (Vec3::splat(100_000.0), Vec3::ZERO)
    };
    let transform =
        Mat4::from_translation(low * scale + world) * Mat4::from_scale((high - low) * scale);
    Ok(Local {
        nodes,
        ellipses: Vec::new(),
        size: RenderSize { width: 1.0 },
        placement: Placement::Matrix(transform),
    })
}
