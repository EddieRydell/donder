use crate::controller::{ControllerId, ControllerPortId};
use crate::element::{ElementNodeId, ElementNodeKind, ElementSelection};
use crate::model::DawnProject;
use crate::patch::{FilterDefinition, PatchNode, PatchSource, PatchValueType};
use crate::setup::SetupId;

/// Replace guided fixture routes in the caller's candidate transaction. Custom
/// processing and group selections require an explicit patch edit.
pub fn replace_fixture_outputs(
    project: &mut DawnProject,
    setup_id: &SetupId,
    node: ElementNodeId,
    controller: ControllerId,
    port: ControllerPortId,
    start_slot: u16,
) -> Result<(), String> {
    let setup = project.setups.get(setup_id).ok_or("Setup was not found.")?;
    let tree = project
        .element_trees
        .get(&setup.elements)
        .ok_or("Element tree was not found.")?;
    let Some(ElementNodeKind::Fixture { profile }) = tree.nodes.get(&node).map(|node| &node.kind)
    else {
        return Err("Choose a fixture element.".into());
    };
    let patch = project
        .patches
        .get(&setup.patch)
        .ok_or("Patch was not found.")?;
    let custom = || {
        "This fixture uses shared or custom patch processing. Edit its assignment in the patch editor.".to_string()
    };
    let mut sinks = Vec::new();
    for (id, candidate) in &patch.nodes {
        let PatchNode::Source(source) = candidate else {
            continue;
        };
        if source.selection.tree != setup.elements {
            continue;
        }
        let selected = tree
            .flatten_selection(&source.selection)
            .map_err(|error| format!("Invalid patch selection: {error:?}"))?;
        if source.selection.node != node && !selected.iter().any(|cell| cell.node == node) {
            continue;
        }
        if source.selection.node != node
            || source.selection.cells.is_some()
            || source.output
                != (PatchValueType::FixtureState {
                    width: 1,
                    profile: profile.clone(),
                })
        {
            return Err(custom());
        }
        let encoder = super::routing::next_node(patch, *id).map_err(|_| custom())?;
        if !matches!(patch.nodes.get(&encoder), Some(PatchNode::Filter(FilterDefinition::FixtureProfileEncoding { profile: encoded, fixture_count: 1, .. })) if encoded == profile)
        {
            return Err(custom());
        }
        let sink = super::routing::next_node(patch, encoder).map_err(|_| custom())?;
        if !matches!(patch.nodes.get(&sink), Some(PatchNode::Sink(_))) {
            return Err(custom());
        }
        sinks.push(sink);
    }
    let patch_id = setup.patch.clone();
    let patch = project
        .patches
        .get_mut(&patch_id)
        .ok_or("Patch was not found.")?;
    for sink in sinks {
        patch.remove_output(sink)?;
    }
    assign_fixture_output(project, setup_id, node, controller, port, start_slot)
}

/// Add an independently removable fixture route. The caller owns the edit
/// transaction; existing routes, including custom processing, remain intact.
pub fn assign_fixture_output(
    project: &mut DawnProject,
    setup_id: &SetupId,
    node: ElementNodeId,
    controller: ControllerId,
    port: ControllerPortId,
    start_slot: u16,
) -> Result<(), String> {
    let setup = project.setups.get(setup_id).ok_or("Setup was not found.")?;
    let tree = project
        .element_trees
        .get(&setup.elements)
        .ok_or("Element tree was not found.")?;
    let Some(ElementNodeKind::Fixture { profile }) = tree.nodes.get(&node).map(|node| &node.kind)
    else {
        return Err("Choose a fixture element.".into());
    };
    let definition = project
        .definitions
        .fixture_profiles
        .definitions
        .get(profile)
        .ok_or("Fixture profile was not found.")?;
    definition.validate().map_err(|error| error.to_string())?;
    let slot_count = u16::try_from(definition.slot_count())
        .map_err(|_| "The fixture profile is too large for one output.")?;
    super::direct_output::append_route(
        project,
        setup_id,
        super::direct_output::DirectOutputRoute {
            source: PatchSource {
                selection: ElementSelection {
                    tree: setup.elements.clone(),
                    node,
                    cells: None,
                },
                output: PatchValueType::FixtureState {
                    width: 1,
                    profile: profile.clone(),
                },
            },
            filters: vec![FilterDefinition::FixtureProfileEncoding {
                profile: profile.clone(),
                fixture_count: 1,
                slot_count: usize::from(slot_count),
            }],
            controller,
            port,
            start_slot,
            slot_count,
        },
    )
}
