//! Single Line and Poly Line models.

use super::{Common, Local, Node, Span, Strings, account};
use crate::report::SkipReason;
use crate::xlights::placement::{self, RenderSize};
use crate::xlights::read::{Name, Reader, Table, Use};
use glam::DVec3;

pub(super) struct SingleLineSettings {
    strings: Strings,
    nodes_per_string: i32,
    lights_per_node: i32,
}

const SINGLE_LINE: Table = &[
    (Name::Exact("NumStrings"), Use::Modeled),
    (Name::Exact("NodesPerString"), Use::Modeled),
    (Name::Exact("LightsPerNode"), Use::Modeled),
    (Name::Exact("parm1"), Use::Modeled),
    (Name::Exact("parm2"), Use::Modeled),
    (Name::Exact("parm3"), Use::Modeled),
];

/// xLights: `XmlDeserializingModelFactory::DeserializeSingleLine`.
fn single_line_settings(
    reader: &Reader,
    common: &Common,
) -> Result<SingleLineSettings, SkipReason> {
    let strings = reader.parm("NumStrings", "parm1", "1");
    let nodes_per_string = reader.parm("NodesPerString", "parm2", "50");
    let lights_per_node = reader.parm("LightsPerNode", "parm3", "1");
    Ok(SingleLineSettings {
        strings: Strings::uniform(strings, nodes_per_string, single_line_ts(common))?,
        nodes_per_string,
        lights_per_node,
    })
}

/// xLights: `SingleLineModel::NodesPerString`, which ignores `ts` for a
/// model whose strings are single nodes.
fn single_line_ts(common: &Common) -> i32 {
    if common.string_type.single_node {
        1
    } else {
        common.ts
    }
}

/// xLights: `SingleLineModel::InitLine` and `InitModel`. Each string's
/// channels run from its start, or back from its end when numbered right to
/// left; the lights sit one buffer column apart from 0, stepped in single
/// precision as xLights steps them.
fn single_line_layout(
    reader: &Reader,
    common: &Common,
    settings: &SingleLineSettings,
) -> Result<Local, SkipReason> {
    account(
        reader,
        super::TWO_POINT_PLACEMENT,
        &[SINGLE_LINE],
        super::NO_CHILDREN,
    )?;
    let strings = &settings.strings;
    if settings.lights_per_node < 1 {
        return Err(SkipReason::Unplaceable("each node has no lights"));
    }
    let lights = settings.lights_per_node as u32;
    super::lights(u64::from(strings.nodes), u64::from(lights))?;
    let width = strings.nodes;
    let string_type = &common.string_type;
    let mut nodes = Vec::with_capacity(strings.nodes as usize);
    let per_string = settings.nodes_per_string.max(0) as u32;
    for node in 0..strings.nodes {
        let string = node / per_string.max(1);
        let index = node % per_string.max(1);
        let index = if common.left_to_right {
            index
        } else {
            strings.nodes_per_string - 1 - index
        };
        nodes.push(Node {
            channel: strings.channel(string_type, string, index),
            lights: Vec::with_capacity(lights as usize),
            arc: None,
        });
    }
    if !nodes.is_empty() && (width > 1 || lights > 1) {
        let count = width as f32 * lights as f32;
        let step = width as f32 / (count - 1.0);
        let mut x = 0f32;
        for node in &mut nodes {
            for _ in 0..lights {
                node.lights.push(DVec3::new(f64::from(x), 0.0, 0.0));
                x += step;
            }
        }
    } else if let Some(node) = nodes.first_mut() {
        node.lights.push(DVec3::new(0.5, 0.0, 0.0));
    }
    Ok(Local {
        nodes,
        ellipses: Vec::new(),
        size: RenderSize {
            width: width as f32,
        },
        placement: placement::two_point(reader)?,
    })
}

pub(super) fn single_line(
    reader: &Reader,
    common: &Common,
) -> (Result<Span, SkipReason>, Result<Local, SkipReason>) {
    match single_line_settings(reader, common) {
        Ok(settings) => (
            Ok(settings.strings.span(&common.string_type)),
            single_line_layout(reader, common, &settings),
        ),
        Err(reason) => (Err(reason.clone()), Err(reason)),
    }
}
