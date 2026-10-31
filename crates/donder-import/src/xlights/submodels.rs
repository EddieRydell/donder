//! A model's submodels: named sets of its nodes, which groups and effects
//! target by `Model/Submodel`. xLights: `SubModel`, read by
//! `XmlSerializeFunctions` and filled by `SubModel::AddDefaultBuffer` or
//! `SubModel::AddRangeXY`.

use super::read::strtol;
use crate::report::SkipReason;
use crate::xml::Element;
use std::collections::HashSet;

/// xLights: `KEEP_XY`, the buffer style that keeps the model's layout.
const KEEP_XY: &str = "Keep XY";

pub(crate) struct SubModel {
    /// `Model/Submodel`, as groups name it.
    pub(crate) name: String,
    /// Its nodes, by index in the model's node order, or why it is not
    /// imported.
    pub(crate) nodes: Result<HashSet<usize>, SkipReason>,
}

/// A `line` entry's first and last node, from 1; 0 is a gap. xLights:
/// `getRange`.
fn range(entry: &str) -> (i64, i64) {
    match entry.split_once('-') {
        Some((first, last)) => (strtol(first), strtol(last)),
        None => {
            let node = strtol(entry);
            (node, node)
        }
    }
}

/// The nodes a submodel's lines name, of a model of `count` nodes.
fn nodes(element: &Element, count: usize) -> Result<HashSet<usize>, SkipReason> {
    if element.attribute("type").unwrap_or("ranges") != "ranges" {
        return Err(SkipReason::Setting(
            "submodel defined by a buffer rectangle",
        ));
    }
    let keep_xy = element.attribute("bufferstyle") == Some(KEEP_XY);
    let mut nodes = HashSet::new();
    for line in (0..).map_while(|index| element.attribute(&format!("line{index}"))) {
        for entry in line.split(',') {
            let (first, last) = range(entry);
            if first == 0 {
                continue;
            }
            let (low, high) = (first.min(last), first.max(last));
            // xLights: AddDefaultBuffer leaves out nodes the model does not
            // have; AddRangeXY keeps them, to no node.
            if keep_xy && (low < 1 || high > count as i64) {
                return Err(SkipReason::Setting(
                    "Keep XY submodel naming nodes the model does not have",
                ));
            }
            for node in low.max(1)..=high.min(count as i64) {
                nodes.insert(node as usize - 1);
            }
        }
    }
    Ok(nodes)
}

/// The model's submodels, of a model of `count` nodes, in order.
pub(crate) fn read(model: &str, element: &Element, count: usize) -> Vec<SubModel> {
    element
        .children("subModel")
        .map(|submodel| SubModel {
            name: format!(
                "{model}/{}",
                submodel.attribute("name").unwrap_or_default().trim()
            ),
            nodes: nodes(submodel, count),
        })
        .collect()
}
