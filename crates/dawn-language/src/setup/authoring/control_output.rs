use crate::controller::{ControllerId, ControllerPortId};
use crate::element::{ElementNodeId, ElementNodeKind, ElementSelection, ElementTreeId};
use crate::model::DawnProject;
use crate::patch::{
    FilterDefinition, PatchId, PatchNode, PatchNodeId, PatchSource, PatchValueType,
};
use crate::setup::SetupId;
use indexmap::IndexMap;

pub enum ControlOutputMapping {
    Scalar,
    Indexed { entries: IndexMap<u32, u8> },
}

pub struct ControlOutputAssignment {
    pub node: ElementNodeId,
    pub controller: ControllerId,
    pub port: ControllerPortId,
    pub start_slot: u16,
    pub mapping: ControlOutputMapping,
}

fn route(
    project: &DawnProject,
    setup_id: &SetupId,
    assignment: ControlOutputAssignment,
) -> Result<super::direct_output::DirectOutputRoute, String> {
    let setup = project.setups.get(setup_id).ok_or("Setup was not found.")?;
    let kind = &project
        .element_trees
        .get(&setup.elements)
        .and_then(|tree| tree.nodes.get(&assignment.node))
        .ok_or("Control element was not found.")?
        .kind;
    let (cells, output, mapping) = match (kind, assignment.mapping) {
        (ElementNodeKind::Scalar { cells }, ControlOutputMapping::Scalar) => (
            *cells,
            PatchValueType::Scalar {
                width: *cells as usize,
            },
            FilterDefinition::ScalarToComponents {
                width: *cells as usize,
            },
        ),
        (
            ElementNodeKind::Indexed { cells, options },
            ControlOutputMapping::Indexed { entries },
        ) => {
            let needs_inactive = !options.iter().any(|option| option.id.0 == 0);
            if entries.len() != options.len() + usize::from(needs_inactive)
                || !entries.contains_key(&0)
                || options
                    .iter()
                    .any(|option| !entries.contains_key(&option.id.0))
            {
                return Err("Assign an 8-bit channel value to every indexed option and inactive ID 0, without extra identifiers.".into());
            }
            (
                *cells,
                PatchValueType::Indexed {
                    width: *cells as usize,
                },
                FilterDefinition::IndexedValueMapping {
                    entries: entries
                        .into_iter()
                        .map(|(id, value)| (id, f32::from(value) / 255.0))
                        .collect(),
                    width: *cells as usize,
                },
            )
        }
        _ => {
            return Err(
                "Choose a dimmer or indexed control and its matching output mapping.".into(),
            );
        }
    };
    let slot_count = u16::try_from(cells)
        .ok()
        .filter(|count| *count > 0)
        .ok_or("The control needs between 1 and 65535 channels on one output.")?;
    Ok(super::direct_output::DirectOutputRoute {
        source: PatchSource {
            selection: ElementSelection {
                tree: setup.elements.clone(),
                node: assignment.node,
                cells: None,
            },
            output,
        },
        filters: vec![
            mapping,
            FilterDefinition::Quantize8 {
                width: cells as usize,
            },
        ],
        controller: assignment.controller,
        port: assignment.port,
        start_slot: assignment.start_slot,
        slot_count,
    })
}

/// Add one independently removable route; the caller owns the candidate transaction.
pub fn assign_control_output(
    project: &mut DawnProject,
    setup_id: &SetupId,
    assignment: ControlOutputAssignment,
) -> Result<(), String> {
    let route = route(project, setup_id, assignment)?;
    super::direct_output::append_route(project, setup_id, route)
}

/// Replace only complete direct routes. Group, partial, shared, or custom routes
/// require an explicit patch edit, and are never silently removed.
pub fn replace_control_outputs(
    project: &mut DawnProject,
    setup_id: &SetupId,
    assignment: ControlOutputAssignment,
) -> Result<(), String> {
    let route = route(project, setup_id, assignment)?;
    let setup = project.setups.get(setup_id).ok_or("Setup was not found.")?;
    let sinks = direct_routes(project, &setup.patch, &route.source)?;
    let patch_id = setup.patch.clone();
    let patch = project
        .patches
        .get_mut(&patch_id)
        .ok_or("Patch was not found.")?;
    for sink in sinks {
        patch.remove_output(sink.sink)?;
    }
    super::direct_output::append_route(project, setup_id, route)
}

struct ControlRoute {
    source: PatchNodeId,
    mapping: PatchNodeId,
    quantize: PatchNodeId,
    sink: PatchNodeId,
}

fn direct_routes(
    project: &DawnProject,
    patch_id: &PatchId,
    source_expected: &PatchSource,
) -> Result<Vec<ControlRoute>, String> {
    let tree = project
        .element_trees
        .get(&source_expected.selection.tree)
        .ok_or("Element tree was not found.")?;
    let patch = project
        .patches
        .get(patch_id)
        .ok_or("Patch was not found.")?;
    let custom = || {
        "This control uses shared, partial, or custom patch processing. Edit its assignment in the patch editor.".to_string()
    };
    let mut sinks = Vec::new();
    for (id, candidate) in &patch.nodes {
        let PatchNode::Source(source) = candidate else {
            continue;
        };
        if source.selection.tree != source_expected.selection.tree {
            continue;
        }
        let selected = tree
            .flatten_selection(&source.selection)
            .map_err(|error| format!("Invalid patch selection: {error:?}"))?;
        if source.selection.node != source_expected.selection.node
            && !selected
                .iter()
                .any(|cell| cell.node == source_expected.selection.node)
        {
            continue;
        }
        if source.selection != source_expected.selection || source.output != source_expected.output
        {
            return Err(custom());
        }
        let mapping = super::routing::next_node(patch, *id).map_err(|_| custom())?;
        let matches_mapping = match (patch.nodes.get(&mapping), &source_expected.output) {
            (
                Some(PatchNode::Filter(FilterDefinition::ScalarToComponents { width })),
                PatchValueType::Scalar { width: expected },
            ) => width == expected,
            (
                Some(PatchNode::Filter(FilterDefinition::IndexedValueMapping { width, .. })),
                PatchValueType::Indexed { width: expected },
            ) => width == expected,
            _ => false,
        };
        if !matches_mapping {
            return Err(custom());
        }
        let quantize = super::routing::next_node(patch, mapping).map_err(|_| custom())?;
        if !matches!(patch.nodes.get(&quantize), Some(PatchNode::Filter(FilterDefinition::Quantize8 { width })) if *width == source_expected.output.width())
        {
            return Err(custom());
        }
        let sink = super::routing::next_node(patch, quantize).map_err(|_| custom())?;
        if !matches!(patch.nodes.get(&sink), Some(PatchNode::Sink(_))) {
            return Err(custom());
        }
        sinks.push(ControlRoute {
            source: *id,
            mapping,
            quantize,
            sink,
        });
    }
    Ok(sinks)
}

/// Resize complete guided routes in the caller's candidate. Returns the changed
/// patches so the IO boundary can enforce document ownership.
pub fn resize_control_outputs(
    project: &mut DawnProject,
    tree_id: &ElementTreeId,
    node: ElementNodeId,
    cells: u32,
) -> Result<Vec<PatchId>, String> {
    let tree = project
        .element_trees
        .get(tree_id)
        .ok_or("Element tree was not found.")?;
    let output = match &tree.nodes.get(&node).ok_or("Control was not found.")?.kind {
        ElementNodeKind::Scalar { cells } => PatchValueType::Scalar {
            width: *cells as usize,
        },
        ElementNodeKind::Indexed { cells, .. } => PatchValueType::Indexed {
            width: *cells as usize,
        },
        _ => return Err("Choose a dimmer or indexed control.".into()),
    };
    if cells == 0 {
        return Err("A control needs at least one cell.".into());
    }
    if cells as usize == output.width() {
        return Ok(Vec::new());
    }
    let source = PatchSource {
        selection: ElementSelection {
            tree: tree_id.clone(),
            node,
            cells: None,
        },
        output,
    };
    let assignments = project
        .patches
        .keys()
        .map(|patch| direct_routes(project, patch, &source).map(|routes| (patch.clone(), routes)))
        .collect::<Result<Vec<_>, _>>()?;
    let mut changed = Vec::new();
    for (patch_id, routes) in assignments {
        if routes.is_empty() {
            continue;
        }
        let count = u16::try_from(cells).map_err(|_| "This control is too large for one output. Use separate controls or edit the patch explicitly.")?;
        let patch = project
            .patches
            .get_mut(&patch_id)
            .ok_or("Patch was not found.")?;
        for route in &routes {
            match patch.nodes.get_mut(&route.source) {
                Some(PatchNode::Source(PatchSource {
                    output: PatchValueType::Scalar { width } | PatchValueType::Indexed { width },
                    ..
                })) => *width = cells as usize,
                _ => return Err("The control source changed during resizing.".into()),
            }
            for id in [route.mapping, route.quantize] {
                match patch.nodes.get_mut(&id) {
                    Some(PatchNode::Filter(
                        FilterDefinition::ScalarToComponents { width }
                        | FilterDefinition::IndexedValueMapping { width, .. }
                        | FilterDefinition::Quantize8 { width },
                    )) => *width = cells as usize,
                    _ => return Err("The control filter changed during resizing.".into()),
                }
            }
            match patch.nodes.get_mut(&route.sink) {
                Some(PatchNode::Sink(sink)) => sink.slot_count = count,
                _ => return Err("The control output changed during resizing.".into()),
            }
        }
        for route in &routes {
            let Some(PatchNode::Sink(sink)) = project.patches[&patch_id].nodes.get(&route.sink)
            else {
                return Err("Control output was not found.".into());
            };
            super::direct_output::validate_destination(project, &patch_id, sink, Some(route.sink))?;
        }
        changed.push(patch_id);
    }
    Ok(changed)
}
