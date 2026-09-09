use crate::controller::{Controller, ControllerId, ControllerPortId};
use crate::element::{
    ColorCapability, ElementCellRange, ElementNodeId, ElementNodeKind, ElementSelection,
    ElementTreeId,
};
use crate::model::DawnProject;
use crate::patch::{
    FilterDefinition, PatchEdge, PatchGraph, PatchId, PatchNode, PatchNodeId, PatchPortId,
    PatchSink, PatchSource, PatchValueType,
};
use crate::setup::SetupId;

#[derive(Clone)]
pub struct PixelOutputAssignment {
    pub node: ElementNodeId,
    pub controller: ControllerId,
    pub first_port: ControllerPortId,
    pub start_slot: u16,
    pub component_order: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OutputSpan {
    port: ControllerPortId,
    start_slot: u16,
    start_cell: u32,
    cells: u32,
}

fn output_spans(
    controller: &Controller,
    assignment: &PixelOutputAssignment,
    cells: u32,
    components: u16,
) -> Result<Vec<OutputSpan>, String> {
    let first = controller
        .ports
        .iter()
        .position(|port| port.id == assignment.first_port)
        .ok_or("Output port was not found.")?;
    let mut remaining = cells;
    let mut spans = Vec::new();
    for (index, port) in controller.ports.iter().enumerate().skip(first) {
        let start_slot = if index == first {
            assignment.start_slot
        } else {
            0
        };
        let available = port
            .slot_count
            .checked_sub(start_slot)
            .ok_or("The start channel is outside this output.")?
            / components;
        let count = remaining.min(u32::from(available));
        if count == 0 {
            continue;
        }
        spans.push(OutputSpan {
            port: port.id,
            start_slot,
            start_cell: cells - remaining,
            cells: count,
        });
        remaining -= count;
        if remaining == 0 {
            return Ok(spans);
        }
    }
    Err(format!(
        "This controller needs space for {remaining} more pixels. Add output ports or choose an earlier start channel."
    ))
}

struct RouteChunk {
    span: OutputSpan,
    controller: ControllerId,
    order: Vec<u16>,
    source: PatchNodeId,
    breakdown: PatchNodeId,
    reorder: Option<PatchNodeId>,
    quantize: PatchNodeId,
    sink: PatchNodeId,
}

impl RouteChunk {
    fn ids(&self) -> impl Iterator<Item = PatchNodeId> {
        [
            Some(self.source),
            Some(self.breakdown),
            self.reorder,
            Some(self.quantize),
            Some(self.sink),
        ]
        .into_iter()
        .flatten()
    }
}

pub(crate) struct PixelRoute {
    assignment: PixelOutputAssignment,
    chunks: Vec<RouteChunk>,
}

pub(super) fn next_node(patch: &PatchGraph, from: PatchNodeId) -> Result<PatchNodeId, String> {
    let mut edges = patch.edges.iter().filter(|edge| edge.from == from);
    let edge = edges.next().ok_or(
        "The light has an unfinished patch route. Complete or remove it in the patch editor first.",
    )?;
    if edges.next().is_some()
        || edge.from_port != PatchPortId(0)
        || edge.to_port != PatchPortId(0)
        || patch
            .edges
            .iter()
            .filter(|candidate| candidate.to == edge.to)
            .count()
            != 1
    {
        return Err("The light uses a shared or custom patch route. Edit that route explicitly before changing this light or its output.".into());
    }
    Ok(edge.to)
}

fn read_chunk(
    patch: &PatchGraph,
    source_id: PatchNodeId,
    source: &PatchSource,
    cells: u32,
    capability: &ColorCapability,
) -> Result<RouteChunk, String> {
    let custom = || {
        "The light uses custom patch processing. Edit that route explicitly before changing this light or its output.".to_string()
    };
    let components =
        u16::try_from(crate::patch::color_component_count(capability)).map_err(|_| custom())?;
    let range = source.selection.cells.unwrap_or(ElementCellRange {
        start: 0,
        count: cells,
    });
    if source.output
        != (PatchValueType::Color {
            width: range.count as usize,
        })
    {
        return Err(custom());
    }
    let breakdown = next_node(patch, source_id)?;
    if patch.nodes.get(&breakdown)
        != Some(&PatchNode::Filter(FilterDefinition::ColorBreakdown {
            capability: capability.clone(),
            cell_count: range.count as usize,
        }))
    {
        return Err(custom());
    }
    let next = next_node(patch, breakdown)?;
    let (reorder, order, quantize) = match patch.nodes.get(&next) {
        Some(PatchNode::Filter(FilterDefinition::ComponentReorder {
            components_per_cell,
            order,
            cell_count,
        })) if *cell_count == range.count as usize && *components_per_cell == components => {
            (Some(next), order.clone(), next_node(patch, next)?)
        }
        _ => (None, (0..components).collect(), next),
    };
    if patch.nodes.get(&quantize)
        != Some(&PatchNode::Filter(FilterDefinition::Quantize8 {
            width: range.count as usize * usize::from(components),
        }))
    {
        return Err(custom());
    }
    let sink_id = next_node(patch, quantize)?;
    let Some(PatchNode::Sink(sink)) = patch.nodes.get(&sink_id) else {
        return Err(custom());
    };
    if Some(u32::from(sink.slot_count)) != range.count.checked_mul(u32::from(components)) {
        return Err(custom());
    }
    Ok(RouteChunk {
        span: OutputSpan {
            port: sink.port,
            start_slot: sink.start_slot,
            start_cell: range.start,
            cells: range.count,
        },
        controller: sink.controller.clone(),
        order,
        source: source_id,
        breakdown,
        reorder,
        quantize,
        sink: sink_id,
    })
}

/// Recover only routes exactly represented by the existing color-light assignment
/// operation. No guessed defaults, discarded processing, or persistent metadata.
pub(crate) fn pixel_routes(
    project: &DawnProject,
    tree_id: &ElementTreeId,
    patch_id: &PatchId,
    node: ElementNodeId,
) -> Result<Vec<PixelRoute>, String> {
    let tree = project
        .element_trees
        .get(tree_id)
        .ok_or("Element tree was not found.")?;
    let Some(ElementNodeKind::Color { cells, capability }) =
        tree.nodes.get(&node).map(|node| &node.kind)
    else {
        return Err("Choose a color light.".into());
    };
    let mut chunks = Vec::new();
    {
        let patch = project
            .patches
            .get(patch_id)
            .ok_or("Patch was not found.")?;
        for (id, patch_node) in &patch.nodes {
            let PatchNode::Source(source) = patch_node else {
                continue;
            };
            if source.selection.tree != *tree_id {
                continue;
            }
            let uses_light = source.selection.node == node
                || tree
                    .flatten_selection(&source.selection)
                    .map_err(|error| format!("Invalid patch source: {error:?}"))?
                    .iter()
                    .any(|cell| cell.node == node);
            if !uses_light {
                continue;
            }
            if source.selection.node != node {
                return Err("This light also participates in a group route. Edit that route explicitly before changing its pixel count or color capability.".into());
            }
            chunks.push(Some(read_chunk(patch, *id, source, *cells, capability)?));
        }
    }
    let mut routes = Vec::new();
    while let Some(first) = chunks.iter().position(|chunk| {
        chunk
            .as_ref()
            .is_some_and(|chunk| chunk.span.start_cell == 0)
    }) {
        let chunk = chunks[first].as_ref().ok_or("Output route disappeared.")?;
        let assignment = PixelOutputAssignment {
            node,
            controller: chunk.controller.clone(),
            first_port: chunk.span.port,
            start_slot: chunk.span.start_slot,
            component_order: chunk.order.clone(),
        };
        let controller = project
            .controllers
            .get(&assignment.controller)
            .ok_or("Controller was not found.")?;
        let spans = output_spans(
            controller,
            &assignment,
            *cells,
            u16::try_from(crate::patch::color_component_count(capability))
                .map_err(|_| "Too many color components.")?,
        )?;
        let mut route_chunks = Vec::new();
        for span in spans {
            let matches = chunks
                .iter()
                .enumerate()
                .filter(|(_, chunk)| {
                    chunk.as_ref().is_some_and(|chunk| {
                        chunk.span == span
                            && chunk.controller == assignment.controller
                            && chunk.order == assignment.component_order
                    })
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            let [index] = matches.as_slice() else {
                return Err("This light has partial or custom output spans. Edit those assignments explicitly before changing its pixel count or color capability.".into());
            };
            route_chunks.push(chunks[*index].take().ok_or("Output route disappeared.")?);
        }
        routes.push(PixelRoute {
            assignment,
            chunks: route_chunks,
        });
    }
    if chunks.iter().any(Option::is_some) {
        return Err("This light has partial output spans. Edit those assignments explicitly before changing its pixel count or color capability.".into());
    }
    Ok(routes)
}

pub fn assign_pixel_output(
    project: &mut DawnProject,
    setup_id: &SetupId,
    assignment: PixelOutputAssignment,
) -> Result<(), String> {
    let setup = project
        .setups
        .get(setup_id)
        .ok_or("Setup was not found.")?
        .clone();
    if !setup.controllers.contains(&assignment.controller) {
        return Err("Choose a controller in this setup.".into());
    }
    write_route(
        project,
        &setup.elements,
        &setup.patch,
        assignment,
        Vec::new(),
        0,
    )
}

pub fn replace_pixel_outputs(
    project: &mut DawnProject,
    setup_id: &SetupId,
    assignment: PixelOutputAssignment,
) -> Result<(), String> {
    let setup = project
        .setups
        .get(setup_id)
        .ok_or("Setup was not found.")?
        .clone();
    if !setup.controllers.contains(&assignment.controller) {
        return Err("Choose a controller in this setup.".into());
    }
    let routes = pixel_routes(project, &setup.elements, &setup.patch, assignment.node)?;
    let reserved = remove_routes(project, &setup.patch, &routes)?;
    write_route(
        project,
        &setup.elements,
        &setup.patch,
        assignment,
        routes
            .into_iter()
            .next()
            .map_or_else(Vec::new, |route| route.chunks),
        reserved,
    )
}

pub fn update_color_capability(
    project: &mut DawnProject,
    tree_id: &ElementTreeId,
    node: ElementNodeId,
    capability: ColorCapability,
    component_order: Vec<u16>,
) -> Result<(), String> {
    capability
        .validate()
        .map_err(|error| format!("Invalid color capability: {error:?}"))?;
    validate_component_order(&capability, &component_order)?;
    let mut routes = project
        .patches
        .keys()
        .map(|patch| {
            pixel_routes(project, tree_id, patch, node).map(|routes| (patch.clone(), routes))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let element = project
        .element_trees
        .get_mut(tree_id)
        .and_then(|tree| tree.nodes.get_mut(&node))
        .ok_or("Light was not found.")?;
    let ElementNodeKind::Color {
        capability: target, ..
    } = &mut element.kind
    else {
        return Err("Choose a color light.".into());
    };
    *target = capability;
    for (_, routes) in &mut routes {
        for route in routes {
            route
                .assignment
                .component_order
                .clone_from(&component_order);
        }
    }
    for (patch, routes) in routes {
        resize_pixel_routes(project, tree_id, &patch, routes)?;
    }
    Ok(())
}

fn validate_component_order(capability: &ColorCapability, order: &[u16]) -> Result<u16, String> {
    let components = u16::try_from(crate::patch::color_component_count(capability))
        .map_err(|_| "Too many color components.")?;
    if components == 0 {
        return Err("A light needs color components.".into());
    }
    let mut order = order.to_vec();
    order.sort_unstable();
    if order != (0..components).collect::<Vec<_>>() {
        return Err("Pixel color order must contain each color component once.".into());
    }
    Ok(components)
}

fn remove_routes(
    project: &mut DawnProject,
    patch_id: &PatchId,
    routes: &[PixelRoute],
) -> Result<u32, String> {
    let patch = project
        .patches
        .get_mut(patch_id)
        .ok_or("Patch was not found.")?;
    let reserved = patch.nodes.keys().map(|id| id.0).max().unwrap_or(0);
    for route in routes {
        for chunk in &route.chunks {
            patch.remove_output(chunk.sink)?;
        }
    }
    Ok(reserved)
}

pub(crate) fn resize_pixel_routes(
    project: &mut DawnProject,
    tree_id: &ElementTreeId,
    patch_id: &PatchId,
    routes: Vec<PixelRoute>,
) -> Result<(), String> {
    let reserved = remove_routes(project, patch_id, &routes)?;
    for route in routes {
        write_route(
            project,
            tree_id,
            patch_id,
            route.assignment,
            route.chunks,
            reserved,
        )?;
    }
    Ok(())
}

fn write_route(
    project: &mut DawnProject,
    tree_id: &ElementTreeId,
    patch_id: &PatchId,
    assignment: PixelOutputAssignment,
    reuse: Vec<RouteChunk>,
    reserved: u32,
) -> Result<(), String> {
    let tree = project
        .element_trees
        .get(tree_id)
        .ok_or("Element tree was not found.")?;
    let Some(ElementNodeKind::Color { cells, capability }) =
        tree.nodes.get(&assignment.node).map(|node| &node.kind)
    else {
        return Err("Pixel output assignment requires a color light.".into());
    };
    let components = validate_component_order(capability, &assignment.component_order)?;
    let controller = project
        .controllers
        .get(&assignment.controller)
        .ok_or("Controller was not found.")?;
    let spans = output_spans(
        controller,
        &assignment,
        *cells,
        u16::try_from(crate::patch::color_component_count(capability))
            .map_err(|_| "Too many color components.")?,
    )?;
    let patch = project
        .patches
        .get_mut(patch_id)
        .ok_or("Patch was not found.")?;
    let mut next_id = patch
        .nodes
        .keys()
        .copied()
        .chain(reuse.iter().flat_map(RouteChunk::ids))
        .map(|id| id.0)
        .max()
        .unwrap_or(0)
        .max(reserved);
    let mut allocate = |existing: Option<PatchNodeId>| -> Result<PatchNodeId, String> {
        if let Some(id) = existing {
            return Ok(id);
        }
        next_id = next_id
            .checked_add(1)
            .ok_or("No patch identifiers remain.")?;
        Ok(PatchNodeId(next_id))
    };
    for (index, span) in spans.into_iter().enumerate() {
        let old = reuse.get(index);
        let ids = [
            allocate(old.map(|chunk| chunk.source))?,
            allocate(old.map(|chunk| chunk.breakdown))?,
            allocate(old.and_then(|chunk| chunk.reorder))?,
            allocate(old.map(|chunk| chunk.quantize))?,
            allocate(old.map(|chunk| chunk.sink))?,
        ];
        let width = span.cells as usize;
        let channel_count = u16::try_from(span.cells * u32::from(components))
            .map_err(|_| "Too many channels for an output.")?;
        if let Some((_, PatchNode::Sink(existing))) = patch.nodes.iter().find(|(_, node)| matches!(node, PatchNode::Sink(sink)
            if sink.controller == assignment.controller && sink.port == span.port
                && u32::from(sink.start_slot) < u32::from(span.start_slot) + u32::from(channel_count)
                && u32::from(span.start_slot) < u32::from(sink.start_slot) + u32::from(sink.slot_count))) {
            return Err(format!("Port {} channels {}–{} are already assigned. Choose free channels or remove the existing assignment.", span.port.0, existing.start_slot + 1, u32::from(existing.start_slot) + u32::from(existing.slot_count)));
        }
        patch.nodes.insert(
            ids[0],
            PatchNode::Source(PatchSource {
                selection: ElementSelection {
                    tree: tree_id.clone(),
                    node: assignment.node,
                    cells: Some(ElementCellRange {
                        start: span.start_cell,
                        count: span.cells,
                    }),
                },
                output: PatchValueType::Color { width },
            }),
        );
        patch.nodes.insert(
            ids[1],
            PatchNode::Filter(FilterDefinition::ColorBreakdown {
                capability: capability.clone(),
                cell_count: width,
            }),
        );
        patch.nodes.insert(
            ids[2],
            PatchNode::Filter(FilterDefinition::ComponentReorder {
                components_per_cell: components,
                order: assignment.component_order.to_vec(),
                cell_count: width,
            }),
        );
        patch.nodes.insert(
            ids[3],
            PatchNode::Filter(FilterDefinition::Quantize8 {
                width: width * usize::from(components),
            }),
        );
        patch.nodes.insert(
            ids[4],
            PatchNode::Sink(PatchSink {
                controller: assignment.controller.clone(),
                port: span.port,
                start_slot: span.start_slot,
                slot_count: channel_count,
            }),
        );
        for pair in ids.windows(2) {
            patch.edges.push(PatchEdge {
                from: pair[0],
                from_port: PatchPortId(0),
                to: pair[1],
                to_port: PatchPortId(0),
            });
        }
    }
    Ok(())
}
