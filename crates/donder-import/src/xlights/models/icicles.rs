//! Icicles: drops hanging from the placement's line.

use super::{Common, Local, Node, Span, Strings, account};
use crate::report::SkipReason;
use crate::xlights::placement::{self, RenderSize};
use crate::xlights::read::{Name, Reader, Table, Use, strtol};
use glam::DVec3;

const ICICLES: Table = &[
    (Name::Exact("NumStrings"), Use::Modeled),
    (Name::Exact("NodesPerString"), Use::Modeled),
    (Name::Exact("parm1"), Use::Modeled),
    (Name::Exact("parm2"), Use::Modeled),
    (Name::Exact("DropPattern"), Use::Modeled),
    (Name::Exact("AlternateNodes"), Use::Modeled),
    (Name::Exact("Shear"), Use::Modeled),
];

struct Settings {
    strings: i32,
    lights_per_string: i32,
    drops: Vec<i32>,
    alternate: bool,
}

/// xLights: `DeserializeIcicles` and `IciclesModel::ParseDropSizes`, which
/// keeps drops of 0, gap columns, and leaves out negative ones.
fn settings(reader: &Reader) -> Settings {
    let mut drops: Vec<i32> = reader
        .as_string("DropPattern", "3,4,5,4")
        .split(',')
        .map(|item| strtol(item) as i32)
        .filter(|drop| *drop >= 0)
        .collect();
    if drops.is_empty() {
        drops.push(5);
    }
    Settings {
        strings: reader.parm("NumStrings", "parm1", "1"),
        lights_per_string: reader.parm("NodesPerString", "parm2", "1"),
        drops,
        alternate: reader.equals("AlternateNodes", "false", "true"),
    }
}

pub(super) fn icicles(
    reader: &Reader,
    common: &Common,
) -> (Result<Span, SkipReason>, Result<Local, SkipReason>) {
    let settings = settings(reader);
    // xLights numbers every light on from the first string's start.
    let strings = match Strings::uniform(settings.strings, settings.lights_per_string, 1) {
        Ok(strings) => strings,
        Err(reason) => return (Err(reason.clone()), Err(reason)),
    };
    // A string type whose node holds a whole string has a node a string.
    let nodes = if common.string_type.single_node {
        u64::from(strings.count)
    } else {
        u64::from(strings.nodes)
    };
    let span = Ok(Span {
        first: 0,
        end: if nodes == 0 {
            0
        } else {
            (nodes - 1) * u64::from(common.string_type.channels)
                + u64::from(common.string_type.node_channels)
        },
    });
    (span, layout(reader, common, &settings, &strings))
}

/// xLights: `IciclesModel::InitModel`: each string's lights down drops of
/// the pattern's lengths, a column for each drop, the drops restarting with
/// each string.
fn layout(
    reader: &Reader,
    common: &Common,
    settings: &Settings,
    strings: &Strings,
) -> Result<Local, SkipReason> {
    account(
        reader,
        super::THREE_POINT_PLACEMENT,
        &[ICICLES],
        super::NO_CHILDREN,
    )?;
    if settings.drops.iter().all(|drop| *drop == 0) {
        return Err(SkipReason::Unplaceable("a drop pattern of only gaps"));
    }
    let drops = &settings.drops;
    let channels = common.string_type.channels;
    let mut width: i32 = -1;
    let mut lights: Vec<(i32, i32)> = Vec::with_capacity(strings.nodes as usize);
    for _ in 0..settings.strings {
        let mut y = 0;
        let mut drop = 0usize;
        let mut in_drop = drops[0];
        width += 1;
        for _ in 0..settings.lights_per_string {
            while y >= drops[drop] {
                width += 1;
                y = 0;
                drop = (drop + 1) % drops.len();
                in_drop = drops[drop];
            }
            let screen_y = if settings.alternate {
                if y < (in_drop + 1) / 2 {
                    2 * y
                } else {
                    (in_drop - (y + 1)) * 2 + 1
                }
            } else {
                y
            };
            lights.push((width, screen_y));
            y += 1;
        }
    }
    let single = width == 0;
    let nodes = lights
        .into_iter()
        .enumerate()
        .map(|(index, (x, y))| {
            let x = if common.left_to_right { x } else { width - x };
            Node {
                channel: index as u32 * channels,
                lights: vec![DVec3::new(
                    if single { 0.5 } else { f64::from(x) },
                    f64::from(y),
                    0.0,
                )],
                arc: None,
            }
        })
        .collect();
    Ok(Local {
        nodes,
        ellipses: Vec::new(),
        size: RenderSize {
            width: if single { 1.0 } else { width as f32 },
        },
        placement: placement::three_point(reader, false, true)?,
    })
}
