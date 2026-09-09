use crate::controller::{ControllerId, ControllerPortId};
use crate::model::DawnProject;
use crate::patch::{
    FilterDefinition, PatchEdge, PatchId, PatchNode, PatchNodeId, PatchPortId, PatchSink,
    PatchSource,
};
use crate::setup::SetupId;

pub(super) struct DirectOutputRoute {
    pub source: PatchSource,
    pub filters: Vec<FilterDefinition>,
    pub controller: ControllerId,
    pub port: ControllerPortId,
    pub start_slot: u16,
    pub slot_count: u16,
}

pub(super) fn append_route(
    project: &mut DawnProject,
    setup_id: &SetupId,
    route: DirectOutputRoute,
) -> Result<(), String> {
    let DirectOutputRoute {
        source,
        filters,
        controller,
        port,
        start_slot,
        slot_count,
    } = route;
    let sink = PatchSink {
        controller,
        port,
        start_slot,
        slot_count,
    };
    let setup = project.setups.get(setup_id).ok_or("Setup was not found.")?;
    if !setup.controllers.contains(&sink.controller) {
        return Err("Attach this controller to the setup first.".into());
    }
    validate_destination(project, &setup.patch, &sink, None)?;
    let patch = project
        .patches
        .get_mut(&setup.patch)
        .ok_or("Patch was not found.")?;
    let first = patch
        .nodes
        .keys()
        .map(|id| id.0)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or("No patch identifiers remain.")?;
    let count = u32::try_from(filters.len()).map_err(|_| "Too many patch filters.")?;
    let last = first
        .checked_add(count)
        .and_then(|id| id.checked_add(1))
        .ok_or("No patch identifiers remain.")?;
    patch
        .nodes
        .insert(PatchNodeId(first), PatchNode::Source(source));
    for (offset, filter) in filters.into_iter().enumerate() {
        patch.nodes.insert(
            PatchNodeId(first + 1 + offset as u32),
            PatchNode::Filter(filter),
        );
    }
    patch.nodes.insert(PatchNodeId(last), PatchNode::Sink(sink));
    for id in first..last {
        patch.edges.push(PatchEdge {
            from: PatchNodeId(id),
            from_port: PatchPortId(0),
            to: PatchNodeId(id + 1),
            to_port: PatchPortId(0),
        });
    }
    Ok(())
}

pub(super) fn validate_destination(
    project: &DawnProject,
    patch_id: &PatchId,
    sink: &PatchSink,
    excluded: Option<PatchNodeId>,
) -> Result<(), String> {
    let PatchSink {
        controller,
        port,
        start_slot,
        slot_count,
    } = sink.clone();
    let output = project
        .controllers
        .get(&controller)
        .and_then(|controller| {
            controller
                .ports
                .iter()
                .find(|candidate| candidate.id == port)
        })
        .ok_or("Controller output was not found.")?;
    if u32::from(start_slot) + u32::from(slot_count) > u32::from(output.slot_count) {
        return Err(format!(
            "This assignment needs {slot_count} consecutive channels on one output. Choose an earlier start channel or an output with more channels."
        ));
    }
    let patch = project
        .patches
        .get(patch_id)
        .ok_or("Patch was not found.")?;
    if patch.nodes.iter().any(|(id, node)| {
        if Some(*id) == excluded {
            return false;
        }
        matches!(node, PatchNode::Sink(sink)
        if sink.controller == controller && sink.port == port
        && u32::from(sink.start_slot) < u32::from(start_slot) + u32::from(slot_count)
        && u32::from(start_slot) < u32::from(sink.start_slot) + u32::from(sink.slot_count))
    }) {
        return Err("These output channels are already assigned. Choose free channels or remove the existing output assignment.".into());
    }
    Ok(())
}
