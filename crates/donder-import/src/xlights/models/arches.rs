//! Arches: each a half ellipse of lights, side by side.

use super::{ArcPoint, Common, Ellipse, Local, Node, Span, Strings, account};
use crate::report::SkipReason;
use crate::xlights::placement::{self, RenderSize};
use crate::xlights::read::{self, Name, Reader, Table, Use};
use glam::DVec3;
use std::f64::consts::{FRAC_PI_2, PI};

const ARCHES: Table = &[
    (Name::Exact("NumArches"), Use::Modeled),
    (Name::Exact("NodesPerArch"), Use::Modeled),
    (Name::Exact("LightsPerNode"), Use::Modeled),
    (Name::Exact("parm1"), Use::Modeled),
    (Name::Exact("parm2"), Use::Modeled),
    (Name::Exact("parm3"), Use::Modeled),
    (Name::Exact("Arc"), Use::Modeled),
    (Name::Exact("arc"), Use::Modeled),
    (Name::Exact("Gap"), Use::Modeled),
    (Name::Exact("ArchesSkew"), Use::Modeled),
    (Name::Exact("LayerSizes"), Use::RefusedUnlessDefault),
    (
        Name::Exact("Hollow"),
        Use::Irrelevant("only shapes arches with layer sizes, which are refused"),
    ),
    (
        Name::Exact("ZigZag"),
        Use::Irrelevant("only orders arches with layer sizes, which are refused"),
    ),
    (
        Name::Exact("Shear"),
        Use::Irrelevant("xLights shears only icicles"),
    ),
];

struct Settings {
    arches: i32,
    nodes_per_arch: i32,
    lights_per_node: i32,
    /// Degrees of arc, `Arc`.
    arc: i32,
    gap: i32,
    /// Degrees the arches lean, `Angle` or `ArchesSkew`.
    skew: i32,
    height: f32,
}

/// xLights: `XmlDeserializingModelFactory::DeserializeArches` and
/// `DeserializeThreePointScreenLocationAttributes`.
fn settings(reader: &Reader) -> Result<Settings, SkipReason> {
    if !read::layer_sizes(reader.as_string("LayerSizes", "")).is_empty() {
        return Err(SkipReason::Setting("layer sizes"));
    }
    let arc = match reader.raw("Arc").or_else(|| reader.raw("arc")) {
        Some(text) => read::strtol(text) as i32,
        None => 180,
    };
    let mut skew = reader.as_int("Angle", 0);
    if reader.has("ArchesSkew") {
        skew = reader.as_int("ArchesSkew", 0);
    }
    Ok(Settings {
        arches: reader.parm("NumArches", "parm1", "1"),
        nodes_per_arch: reader.parm("NodesPerArch", "parm2", "1"),
        lights_per_node: reader.parm("LightsPerNode", "parm3", "1"),
        arc,
        gap: if reader.has("Gap") {
            reader.as_int("Gap", 0)
        } else {
            0
        },
        skew,
        height: reader.as_float("Height", 1.0)?,
    })
}

/// xLights: `toRadians` in `UtilFunctions.h`.
fn to_radians(degrees: f32) -> f64 {
    2.0 * PI * f64::from(degrees) / 360.0
}

/// xLights: `rotate_point` in `ArchesModel.cpp`, about `(cx, 0)`.
fn rotate_point(cx: f32, angle: f32, x: &mut f32, y: &mut f32) {
    let (s, c) = angle.sin_cos();
    let (px, py) = (*x - cx, *y);
    *x = px * c - py * s + cx;
    *y = px * s + py * c;
}

pub(super) fn arches(
    reader: &Reader,
    common: &Common,
) -> (Result<Span, SkipReason>, Result<Local, SkipReason>) {
    let settings = match settings(reader) {
        Ok(settings) => settings,
        Err(reason) => return (Err(reason.clone()), Err(reason)),
    };
    // xLights: ArchesModel::CalcChannelsPerString, which takes no ts.
    let strings = match Strings::uniform(settings.arches, settings.nodes_per_arch, 1) {
        Ok(strings) => strings,
        Err(reason) => return (Err(reason.clone()), Err(reason)),
    };
    // xLights: ArchesModel spaces arches by its nodes per arch and keeps a
    // node a segment even when a node holds a whole string.
    let string_type = &common.string_type;
    let span = Ok(if strings.nodes == 0 {
        Span { first: 0, end: 0 }
    } else {
        let per_arch = (strings.nodes / strings.count) as u64;
        let step = u64::from(string_type.channels);
        Span {
            first: 0,
            end: (u64::from(strings.count) - 1) * step * per_arch
                + (per_arch - 1) * step
                + u64::from(string_type.node_channels),
        }
    });
    (span, layout(reader, common, &settings, &strings))
}

/// xLights: `ArchesModel::InitModel` and `SetArchCoord`.
fn layout(
    reader: &Reader,
    common: &Common,
    settings: &Settings,
    strings: &Strings,
) -> Result<Local, SkipReason> {
    account(
        reader,
        super::THREE_POINT_PLACEMENT,
        &[ARCHES],
        super::NO_CHILDREN,
    )?;
    if settings.lights_per_node < 1 {
        return Err(SkipReason::Unplaceable("each node has no lights"));
    }
    super::lights(u64::from(strings.nodes), settings.lights_per_node as u64)?;
    let (arches, per_arch) = (strings.count, settings.nodes_per_arch as u32);
    let lights = settings.lights_per_node as u32;
    if per_arch * lights < 2 {
        return Err(SkipReason::Unplaceable("each arch has one light"));
    }
    let string_type = &common.string_type;
    let length = f64::from(per_arch * lights);
    let midpt = (length - 1.0) / 2.0;
    let total = to_radians(settings.arc as f32);
    let start = (PI - total) / 2.0;
    let skew = to_radians(settings.skew as f32) as f32;
    let first = -FRAC_PI_2 + start;
    let width = length * 2.0 - (midpt * first.sin() * 2.0 + length);
    let mut nodes = Vec::with_capacity((arches * per_arch) as usize);
    let mut ellipses = Vec::new();
    let mut min_y = 999_999f64;
    let mut gaps = 0u32;
    for arch in 0..arches {
        let xoffset = f64::from(arch) * width;
        ellipses.push(Ellipse {
            center: DVec3::new(
                xoffset + length + f64::from(gaps) * f64::from(settings.gap),
                0.0,
                0.0,
            ),
            u: DVec3::new(midpt * 2.0, 0.0, 0.0),
            v: DVec3::new(0.0, length * f64::from(settings.height), 0.0),
        });
        for x in 0..per_arch {
            let channel = if common.left_to_right {
                strings.channel(string_type, arch, x)
            } else {
                strings.channel(string_type, arches - arch - 1, per_arch - x - 1)
            };
            let mut node_lights = Vec::with_capacity(lights as usize);
            for c in 0..lights {
                let angle = -FRAC_PI_2 + start + total * f64::from(x * lights + c) / midpt / 2.0;
                let px = xoffset
                    + midpt * angle.sin() * 2.0
                    + length
                    + f64::from(gaps) * f64::from(settings.gap);
                let py = length * angle.cos();
                let mut sx = px as f32;
                let mut sy = (py * f64::from(settings.height)) as f32;
                rotate_point(px as f32, skew, &mut sx, &mut sy);
                min_y = min_y.min(py);
                node_lights.push(DVec3::new(f64::from(sx), f64::from(sy), 0.0));
            }
            nodes.push(Node {
                channel,
                lights: node_lights,
                arc: (lights == 1 && settings.skew == 0).then(|| ArcPoint {
                    arc: ellipses.len() - 1,
                    degrees: 90.0
                        - (-FRAC_PI_2 + start + total * f64::from(x) / midpt / 2.0).to_degrees(),
                }),
            });
            if (nodes.len() as u32).is_multiple_of(per_arch) {
                gaps += 1;
            }
        }
    }
    if min_y > 1.0 {
        for node in &mut nodes {
            for light in &mut node.lights {
                // xLights: a float less a double, stored as a float.
                light.y = f64::from((light.y - min_y) as f32);
            }
        }
        for ellipse in &mut ellipses {
            ellipse.center.y -= min_y;
        }
    }
    let arch_count = f64::from(arches);
    Ok(Local {
        nodes,
        ellipses,
        size: RenderSize {
            width: (width * arch_count + (arch_count - 1.0) * f64::from(settings.gap)) as f32,
        },
        placement: placement::three_point(reader, true, false)?,
    })
}
