//! Custom models: a grid of cells, each naming the node lit there.

use super::{Common, Local, Node, Span, account};
use crate::placement::{self, RenderSize};
use crate::read::{Name, Reader, Table, Use, strtol};
use crate::report::SkipReason;
use glam::DVec3;
use std::collections::{BTreeMap, HashSet};

const CUSTOM: Table = &[
    (Name::Exact("CustomModel"), Use::Modeled),
    (Name::Exact("CustomModelCompressed"), Use::Modeled),
    (Name::Exact("CustomStrings"), Use::Modeled),
    (Name::Numbered("NodeStart"), Use::Modeled),
    (Name::Numbered("String"), Use::Modeled),
    (
        Name::Exact("CustomWidth"),
        Use::Irrelevant("the size of xLights' effect buffer"),
    ),
    (
        Name::Exact("CustomHeight"),
        Use::Irrelevant("the size of xLights' effect buffer"),
    ),
    (
        Name::Exact("parm1"),
        Use::Irrelevant("the width of xLights' effect buffer"),
    ),
    (
        Name::Exact("parm2"),
        Use::Irrelevant("the height of xLights' effect buffer"),
    ),
    (
        Name::Exact("Depth"),
        Use::Irrelevant("the depth of xLights' effect buffer"),
    ),
    (
        Name::Exact("CustomBkgImage"),
        Use::Irrelevant("a picture behind the model in xLights' editor"),
    ),
    (
        Name::Exact("CustomBkgLightness"),
        Use::Irrelevant("a picture behind the model in xLights' editor"),
    ),
    (
        Name::Exact("CustomBkgScale"),
        Use::Irrelevant("a picture behind the model in xLights' editor"),
    ),
    (
        Name::Exact("CustomBkgBrightness"),
        Use::Irrelevant("a picture behind the model in xLights' editor"),
    ),
    (
        Name::Exact("CustomBkgTransparency"),
        Use::Irrelevant("a picture behind the model in xLights' editor"),
    ),
];

/// A cell of the grid: its layer, row and column.
type Cell = (usize, usize, usize);

/// The cells the grid sets, each to a node number; 0 or less is empty.
type Grid = BTreeMap<Cell, i32>;

/// xLights: `XmlSerialize::ParseCustomModel`, which fills a grid as wide
/// as its widest row, then cuts every layer to the last layer's height.
fn parse_grid(text: &str) -> Grid {
    let mut grid = Grid::new();
    let mut height = 1;
    for (layer, rows) in text.split('|').enumerate() {
        let rows: Vec<&str> = rows.split(';').collect();
        height = rows.len();
        for (row, values) in rows.iter().enumerate() {
            for (column, value) in values.split(',').enumerate() {
                let value = value.trim_start_matches(' ');
                if !value.is_empty() {
                    grid.insert((layer, row, column), strtol(value) as i32);
                }
            }
        }
    }
    grid.retain(|&(_, row, _), _| row < height);
    grid
}

/// xLights: `XmlSerialize::ParseCompressed`: `node,row,column[,layer]`
/// entries, the last for a cell winning.
fn parse_compressed(text: &str) -> Result<Grid, SkipReason> {
    let index = |value: i64| {
        usize::try_from(value as i32)
            .map_err(|_| SkipReason::Unplaceable("a cell outside the grid"))
    };
    let mut grid = Grid::new();
    for entry in text.split(';') {
        let parts: Vec<i64> = entry.split(',').map(strtol).collect();
        let (node, row, column, layer) = match parts[..] {
            [node, row, column] => (node, row, column, 0),
            [node, row, column, layer] => (node, row, column, layer),
            _ => continue,
        };
        grid.insert((index(layer)?, index(row)?, index(column)?), node as i32);
    }
    Ok(grid)
}

struct Settings {
    /// The highest node number. xLights: `GetCustomMaxChannel`.
    max_node: i32,
    /// The nodes the grid names, each with its cells.
    cells: BTreeMap<i32, Vec<Cell>>,
    /// The lowest node a string starts at.
    first_node: i32,
}

/// xLights: `DeserializeCustom`, `ParseCustomModelDataFromXml` and the
/// string start nodes `CustomModel::SetStringStartChannels` numbers from.
fn settings(reader: &Reader) -> Result<Settings, SkipReason> {
    let compressed = reader.as_string("CustomModelCompressed", "");
    let grid = if compressed.is_empty() {
        parse_grid(reader.as_string("CustomModel", ""))
    } else {
        parse_compressed(compressed)?
    };
    let mut cells: BTreeMap<i32, Vec<Cell>> = BTreeMap::new();
    let mut max_node = 0;
    for (&cell, &node) in &grid {
        max_node = max_node.max(node);
        if node > 0 {
            cells.entry(node).or_default().push(cell);
        }
    }
    let strings = reader.as_int("CustomStrings", 1);
    let mut first_node = 1;
    if strings > 1 {
        super::lights(strings as u64, 1)?;
        let count = cells.len() as f32;
        first_node = (0..strings)
            .map(|string| {
                let mut value = reader.as_string(&format!("NodeStart{}", string + 1), "");
                if value.is_empty() {
                    value = reader.as_string(&format!("String{}", string + 1), "0");
                }
                let mut node = strtol(value) as i32;
                if node == 0 {
                    // xLights: Model::ComputeStringStartNode.
                    node = if string == 0 {
                        1
                    } else {
                        (string as f32 * (count / strings as f32) + 1.0) as i32
                    };
                }
                node.max(1).min(max_node)
            })
            .min()
            .unwrap_or(1);
    }
    Ok(Settings {
        max_node,
        cells,
        first_node,
    })
}

pub(super) fn custom(
    reader: &Reader,
    common: &Common,
) -> (Result<Span, SkipReason>, Result<Local, SkipReason>) {
    let settings = match settings(reader) {
        Ok(settings) => settings,
        Err(reason) => return (Err(reason.clone()), Err(reason)),
    };
    let channels = u64::from(common.string_type.channels);
    let offset = (settings.first_node - 1).max(0) as u64 * channels;
    // xLights: InitCustomMatrix steps each node by the channels its node
    // holds (GetChanCountPerNode), made with no lights.
    let per_node = u64::from(common.string_type.empty_node_channels);
    let span = Ok(
        match (settings.cells.keys().next(), settings.cells.keys().last()) {
            (Some(&low), Some(&high)) => Span {
                first: offset + (low - 1) as u64 * per_node,
                end: offset + (high - 1) as u64 * per_node + per_node,
            },
            _ => Span { first: 0, end: 0 },
        },
    );
    (span, layout(reader, common, &settings, offset))
}

/// xLights: `CustomModel::InitCustomMatrix`: a node for each number the
/// grid names, in number order, its channels counted on from the first
/// string's start, each cell a light about the middle of the cells used.
fn layout(
    reader: &Reader,
    common: &Common,
    settings: &Settings,
    offset: u64,
) -> Result<Local, SkipReason> {
    account(
        reader,
        super::BOXED_PLACEMENT,
        &[CUSTOM],
        super::NO_CHILDREN,
    )?;
    if settings.cells.is_empty() {
        return Err(SkipReason::NoPixels);
    }
    super::lights(settings.max_node as u64, 1)?;
    super::lights(
        settings
            .cells
            .values()
            .map(|cells| cells.len() as u64)
            .sum(),
        1,
    )?;
    let all = settings.cells.values().flatten();
    let (mut low, mut high) = ((usize::MAX, usize::MAX, usize::MAX), (0, 0, 0));
    for &(layer, row, column) in all {
        low = (low.0.min(layer), low.1.min(row), low.2.min(column));
        high = (high.0.max(layer), high.1.max(row), high.2.max(column));
    }
    let center = |a: usize, b: usize| (a + b) as f32 / 2.0;
    let (center_layer, center_row, center_column) = (
        center(low.0, high.0),
        center(low.1, high.1),
        center(low.2, high.2),
    );
    let channels = u64::from(common.string_type.channels);
    let mut nodes = Vec::with_capacity(settings.cells.len());
    for (&number, cells) in &settings.cells {
        // A pixel goes at the mean of its node's cells, so they must touch.
        if !touching(cells) {
            return Err(SkipReason::Unplaceable("a node whose cells are apart"));
        }
        let channel = u32::try_from(offset + (number - 1) as u64 * channels)
            .map_err(|_| SkipReason::TooManyPixels)?;
        nodes.push(Node {
            channel,
            lights: cells
                .iter()
                .map(|&(layer, row, column)| {
                    DVec3::new(
                        f64::from(column as f32 - center_column),
                        f64::from(center_row - row as f32),
                        f64::from(center_layer - layer as f32),
                    )
                })
                .collect(),
            arc: None,
        });
    }
    Ok(Local {
        nodes,
        ellipses: Vec::new(),
        size: RenderSize { width: 1.0 },
        placement: placement::boxed(reader)?,
    })
}

/// Whether cells form one group, each next to another in every direction,
/// diagonals included.
fn touching(cells: &[Cell]) -> bool {
    let all: HashSet<Cell> = cells.iter().copied().collect();
    let mut reached = HashSet::from([cells[0]]);
    let mut queue = vec![cells[0]];
    while let Some((layer, row, column)) = queue.pop() {
        let near = |value: usize| value.saturating_sub(1)..=value.saturating_add(1);
        for layer in near(layer) {
            for row in near(row) {
                for column in near(column) {
                    let cell = (layer, row, column);
                    if all.contains(&cell) && reached.insert(cell) {
                        queue.push(cell);
                    }
                }
            }
        }
    }
    reached.len() == all.len()
}
