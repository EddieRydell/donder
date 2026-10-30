//! Custom models: a grid of cells, each naming the node lit there.

use super::{Common, Local, Node, Span, account};
use crate::placement::{self, RenderSize};
use crate::read::{Name, Reader, Table, Use, strtol};
use crate::report::SkipReason;
use glam::DVec3;
use std::collections::BTreeMap;

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

/// The most cells the importer reads a custom model's grid into.
const MOST_CELLS: usize = 10_000_000;

/// Node numbers by layer, row and column; 0 or less is an empty cell.
type Grid = Vec<Vec<Vec<i32>>>;

/// xLights: `XmlSerialize::ParseCustomModel`.
fn parse_grid(text: &str) -> Result<Grid, SkipReason> {
    let mut grid: Grid = Vec::new();
    let mut width = 1usize;
    let mut height = 1usize;
    let mut allocated = 0usize;
    for layer in text.split('|') {
        let rows: Vec<&str> = layer.split(';').collect();
        height = rows.len();
        let mut current = vec![Vec::new(); height];
        for (row, values) in rows.iter().enumerate() {
            let columns: Vec<&str> = values.split(',').collect();
            width = width.max(columns.len());
            allocated += width;
            if allocated > MOST_CELLS {
                return Err(SkipReason::TooManyPixels);
            }
            current[row].resize(width, -1);
            for (column, value) in columns.iter().enumerate() {
                let value = value.trim_start_matches(' ');
                if !value.is_empty() {
                    current[row][column] = strtol(value) as i32;
                }
            }
        }
        grid.push(current);
    }
    if grid.len().saturating_mul(height).saturating_mul(width) > MOST_CELLS {
        return Err(SkipReason::TooManyPixels);
    }
    for layer in &mut grid {
        layer.resize(height, Vec::new());
        for row in layer {
            row.resize(width, -1);
        }
    }
    Ok(grid)
}

/// xLights: `XmlSerialize::ParseCompressed`: `node,row,column[,layer]`
/// entries, the last for a cell winning.
fn parse_compressed(text: &str) -> Result<Grid, SkipReason> {
    let mut entries = Vec::new();
    for entry in text.split(';') {
        let parts: Vec<i64> = entry.split(',').map(strtol).collect();
        match parts[..] {
            [node, row, column] => entries.push((node, row, column, 0)),
            [node, row, column, layer] => entries.push((node, row, column, layer)),
            _ => {}
        }
    }
    let index = |value: i64| {
        usize::try_from(value)
            .ok()
            .filter(|value| *value < 100_000)
            .ok_or(SkipReason::Unplaceable("a cell outside the grid"))
    };
    let (mut layers, mut rows, mut columns) = (0, 0, 0);
    for &(_, row, column, layer) in &entries {
        layers = layers.max(index(layer)?);
        rows = rows.max(index(row)?);
        columns = columns.max(index(column)?);
    }
    let cells = (layers as u64 + 1) * (rows as u64 + 1) * (columns as u64 + 1);
    if cells > MOST_CELLS as u64 {
        return Err(SkipReason::TooManyPixels);
    }
    let mut grid = vec![vec![vec![-1; columns + 1]; rows + 1]; layers + 1];
    for (node, row, column, layer) in entries {
        grid[layer as usize][row as usize][column as usize] = node as i32;
    }
    Ok(grid)
}

struct Settings {
    /// The highest node number. xLights: `GetCustomMaxChannel`.
    max_node: i32,
    /// The nodes the grid names, each with its cells.
    cells: BTreeMap<i32, Vec<(usize, usize, usize)>>,
    /// The lowest node a string starts at.
    first_node: i32,
}

/// xLights: `DeserializeCustom`, `ParseCustomModelDataFromXml` and the
/// string start nodes `CustomModel::SetStringStartChannels` numbers from.
fn settings(reader: &Reader) -> Result<Settings, SkipReason> {
    let compressed = reader.as_string("CustomModelCompressed", "");
    let grid = if compressed.is_empty() {
        parse_grid(reader.as_string("CustomModel", ""))?
    } else {
        parse_compressed(compressed)?
    };
    let mut cells: BTreeMap<i32, Vec<(usize, usize, usize)>> = BTreeMap::new();
    let mut max_node = 0;
    for (layer, rows) in grid.iter().enumerate() {
        for (row, columns) in rows.iter().enumerate() {
            for (column, &node) in columns.iter().enumerate() {
                max_node = max_node.max(node);
                if node > 0 {
                    cells.entry(node).or_default().push((layer, row, column));
                }
            }
        }
    }
    let strings = reader.as_int("CustomStrings", 1);
    let mut first_node = 1;
    if strings > 1 {
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
    if settings.max_node > 1_000_000 {
        return Err(SkipReason::TooManyPixels);
    }
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
fn touching(cells: &[(usize, usize, usize)]) -> bool {
    let mut reached = vec![false; cells.len()];
    let mut queue = vec![0];
    reached[0] = true;
    while let Some(from) = queue.pop() {
        for (to, cell) in cells.iter().enumerate() {
            let near = |a: usize, b: usize| a.abs_diff(b) <= 1;
            if !reached[to]
                && near(cell.0, cells[from].0)
                && near(cell.1, cells[from].1)
                && near(cell.2, cells[from].2)
            {
                reached[to] = true;
                queue.push(to);
            }
        }
    }
    reached.into_iter().all(|reached| reached)
}
