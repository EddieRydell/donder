//! Custom models: nodes drawn on a grid of cells.

use super::{Local, Node, Spec, mean};
use crate::placement;
use crate::report::SkipReason;
use glam::DVec3;
use std::collections::BTreeMap;

/// Each cell holding node `n` is a light of node `n`, its channels `n - 1`
/// nodes after the model's start. Rows count down from the top and layers
/// back from the front, centred on the cells in use.
pub(super) fn custom(spec: &Spec) -> Result<Local, SkipReason> {
    let invalid = |name: &str, value: &str| SkipReason::Attribute {
        name: name.to_string(),
        value: value.to_string(),
    };
    // Node number to its cells, as layer, row and column.
    let mut cells: BTreeMap<u32, Vec<(f64, f64, f64)>> = BTreeMap::new();
    let compressed = spec.text("CustomModelCompressed").unwrap_or_default();
    if !compressed.is_empty() {
        for entry in compressed
            .split(';')
            .filter(|entry| !entry.trim().is_empty())
        {
            let values = entry
                .split(',')
                .map(|value| value.trim().parse::<u32>())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| invalid("CustomModelCompressed", entry))?;
            let (node, row, column, layer) = match values[..] {
                [node, row, column] => (node, row, column, 0),
                [node, row, column, layer] => (node, row, column, layer),
                _ => return Err(invalid("CustomModelCompressed", entry)),
            };
            cells.entry(node).or_default().push((
                f64::from(layer),
                f64::from(row),
                f64::from(column),
            ));
        }
    } else {
        let model = spec.text("CustomModel").unwrap_or_default();
        for (layer, rows) in model.split('|').enumerate() {
            for (row, columns) in rows.split(';').enumerate() {
                for (column, cell) in columns.split(',').enumerate() {
                    let cell = cell.trim();
                    if cell.is_empty() {
                        continue;
                    }
                    let node: i64 = cell.parse().map_err(|_| invalid("CustomModel", cell))?;
                    if node > 0 {
                        let node = u32::try_from(node).map_err(|_| invalid("CustomModel", cell))?;
                        cells.entry(node).or_default().push((
                            layer as f64,
                            row as f64,
                            column as f64,
                        ));
                    }
                }
            }
        }
    }
    cells.retain(|node, _| *node > 0);
    if let Some(&last) = cells.keys().next_back() {
        super::nodes(&[last])?;
    }
    let all = || cells.values().flatten();
    let middle = |axis: fn(&(f64, f64, f64)) -> f64| {
        let low = all().map(axis).fold(f64::INFINITY, f64::min);
        let high = all().map(axis).fold(f64::NEG_INFINITY, f64::max);
        (low + high) / 2.0
    };
    let (layer_middle, row_middle, column_middle) = (
        middle(|cell| cell.0),
        middle(|cell| cell.1),
        middle(|cell| cell.2),
    );
    let nodes = cells
        .iter()
        .map(|(&node, places)| {
            let points: Vec<DVec3> = places
                .iter()
                .map(|&(layer, row, column)| {
                    DVec3::new(
                        column - column_middle,
                        row_middle - row,
                        layer_middle - layer,
                    )
                })
                .collect();
            Node {
                string: 0,
                offset: node - 1,
                position: mean(&points),
                arc: None,
            }
        })
        .collect::<Vec<_>>();
    let string_nodes = nodes.iter().map(|node| node.offset + 1).max().unwrap_or(0);
    Ok(Local {
        nodes,
        ellipses: Vec::new(),
        strings: 1,
        string_nodes,
        placement: placement::boxed(spec)?,
    })
}
