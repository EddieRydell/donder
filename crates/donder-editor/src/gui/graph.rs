//! Layers and the composition graph, and the clips and mark collections on
//! them, which sequences and consoles share.
use std::collections::BTreeSet;

use donder_model::{
    Clip, ClipContent, ClipId, Composition, CompositionGraphNode, CompositionGraphNodeId,
    CompositionGraphNodeKind, Effect, EffectGraphEdge, FixtureId, GraphNodePosition,
    GraphOperatorNode, Layer, LayerId, Mark, MarkCollection, MarkCollectionId, Placement,
};
use donder_project_io::ProjectSession;
use donder_runtime_types::TargetScope;

use super::model::{
    chosen_effect, fresh_name, graph_operator_from_gui, identifier, parse_color,
    required_param_values, typed_name,
};
use super::{GuiMutationError, next_id};
use crate::dto::{
    CompositionGuiEdit, NewMarkCollection, SequenceEffectReference, SequenceGraphEdge,
    SequenceGraphNodePosition,
};

/// A new clip of `composition` over `target`: the next clip id and a fresh
/// name after `name`.
pub(super) fn new_clip(
    composition: &Composition,
    name: &str,
    target: FixtureId,
    placement: Placement,
    content: ClipContent,
) -> Result<Clip, GuiMutationError> {
    Ok(Clip {
        id: ClipId(next_id(composition.clips.iter().map(|clip| clip.id.0))?),
        name: fresh_name(name, |candidate| {
            composition
                .clips
                .iter()
                .any(|clip| clip.name.as_str() == candidate)
        }),
        description: None,
        target,
        placement,
        content,
    })
}

/// A new clip of `composition` playing the effect `reference` names on its
/// first layer, named after the effect.
pub(super) fn new_effect_clip(
    session: &ProjectSession,
    composition: &Composition,
    target: FixtureId,
    placement: Placement,
    (reference, scope, initial_color): (SequenceEffectReference, TargetScope, &str),
) -> Result<Clip, GuiMutationError> {
    let Some(layer) = composition.layers.first() else {
        unreachable!("validation proved every sequence and console has a layer");
    };
    let (definition, effect_name, params) = chosen_effect(session, reference, initial_color)?;
    new_clip(
        composition,
        &effect_name,
        target,
        placement,
        ClipContent::Effect(Effect {
            layer: layer.id.clone(),
            scope,
            definition,
            params,
        }),
    )
}

/// Apply a layer or graph edit to `composition`, which `owner` declares.
pub(super) fn edit_composition(
    session: &mut ProjectSession,
    composition: &mut Composition,
    edit: CompositionGuiEdit,
) -> Result<Vec<String>, GuiMutationError> {
    match edit {
        CompositionGuiEdit::AddMarkCollections { collections } => {
            return add_mark_collections(&mut composition.marks, collections);
        }
        CompositionGuiEdit::CreateLayer { name, color } => {
            return create_layer(composition, name, color, None, true).map(|name| vec![name]);
        }
        CompositionGuiEdit::CreateLayerAt { name, color, x, y } => {
            return create_layer(composition, name, color, Some((x, y)), false)
                .map(|name| vec![name]);
        }
        CompositionGuiEdit::SetLayerEnabled { id, enabled } => {
            layer_mut(composition, id)?.enabled = enabled;
        }
        CompositionGuiEdit::AddGraphOperatorNode {
            operator,
            x,
            y,
            initial_color,
        } => {
            let initial_color = parse_color(&initial_color)?;
            let operator = graph_operator_from_gui(session, &operator)?;
            // The session's inventory declares exactly its project's objects.
            let definition = session
                .project()
                .reusable::<donder_model::ScriptDefinition>()[&operator]
                .clone();
            let params = required_param_values(definition.params(), initial_color)?;
            let id = next_node_id(composition)?;
            let name = fresh_name(definition.name(), |candidate| {
                node_name_taken(composition, candidate)
            });
            composition.graph.nodes.push(CompositionGraphNode {
                id,
                position: GraphNodePosition { x, y },
                kind: CompositionGraphNodeKind::Operator(GraphOperatorNode {
                    name,
                    operator,
                    params,
                }),
            });
        }
        CompositionGuiEdit::MoveGraphNodes { positions } => {
            move_nodes(composition, positions)?;
        }
        CompositionGuiEdit::DeleteGraphItems {
            node_ids,
            layer_ids,
            edges,
        } => {
            delete_items(composition, node_ids, layer_ids, edges)?;
        }
        CompositionGuiEdit::ConnectGraphNodes { connection } => {
            connect_nodes(composition, connection, None)?;
        }
        CompositionGuiEdit::ReconnectGraphEdge {
            previous,
            connection,
        } => {
            connect_nodes(composition, connection, Some(previous))?;
        }
    }
    Ok(Vec::new())
}

/// Each name becomes a unique `snake_case` name; the names made, in order.
fn add_mark_collections(
    existing: &mut Vec<MarkCollection>,
    collections: Vec<NewMarkCollection>,
) -> Result<Vec<String>, GuiMutationError> {
    let mut created = Vec::new();
    for collection in collections {
        typed_name(&collection.name)?;
        let name = fresh_name(&collection.name, |candidate| {
            existing
                .iter()
                .any(|existing| existing.name.as_str() == candidate)
        });
        let mut marks = collection
            .marks_seconds
            .iter()
            .map(|&seconds| super::checked_gui_time(seconds).map(Mark::at))
            .collect::<Result<Vec<_>, _>>()?;
        marks.sort_by_key(|mark| mark.time.0);
        created.push(name.as_str().to_string());
        let id = MarkCollectionId(next_id(existing.iter().map(|existing| existing.id.0))?);
        existing.push(MarkCollection {
            id,
            name,
            description: None,
            display_color: parse_color(&collection.color)?,
            marks,
        });
    }
    Ok(created)
}

pub(super) fn layer_mut(
    composition: &mut Composition,
    id: u32,
) -> Result<&mut Layer, GuiMutationError> {
    composition
        .layers
        .iter_mut()
        .find(|layer| layer.id.0 == id)
        .ok_or_else(|| GuiMutationError::Invalid("Layer was not found.".to_string()))
}

pub(super) fn node_mut<'a>(
    composition: &'a mut Composition,
    id: &CompositionGraphNodeId,
) -> Result<&'a mut CompositionGraphNode, GuiMutationError> {
    composition
        .graph
        .nodes
        .iter_mut()
        .find(|node| node.id == *id)
        .ok_or_else(|| GuiMutationError::Invalid("Graph node was not found.".to_string()))
}

fn ensure_node_exists(
    composition: &Composition,
    node_id: &CompositionGraphNodeId,
) -> Result<(), GuiMutationError> {
    if composition
        .graph
        .nodes
        .iter()
        .any(|node| node.id == *node_id)
    {
        Ok(())
    } else {
        Err(GuiMutationError::Invalid(
            "Graph node was not found.".to_string(),
        ))
    }
}

fn next_node_id(composition: &Composition) -> Result<CompositionGraphNodeId, GuiMutationError> {
    next_id(composition.graph.nodes.iter().map(|node| node.id.0)).map(CompositionGraphNodeId)
}

/// Whether a name is used by an operator node or the root node: they share
/// the graph's namespace.
fn node_name_taken(composition: &Composition, name: &str) -> bool {
    name == donder_language::data::ROOT_NODE
        || composition.graph.nodes.iter().any(|node| {
            matches!(&node.kind, CompositionGraphNodeKind::Operator(operator) if operator.name.as_str() == name)
        })
}

fn layer_name_taken(composition: &Composition, name: &str) -> bool {
    composition
        .layers
        .iter()
        .any(|layer| layer.name.as_str() == name)
}

fn create_layer(
    composition: &mut Composition,
    name: String,
    color: String,
    position: Option<(f32, f32)>,
    connect_to_root: bool,
) -> Result<String, GuiMutationError> {
    let next_layer_id = next_id(composition.layers.iter().map(|layer| layer.id.0))?;
    let Some(root) = composition
        .graph
        .nodes
        .iter()
        .find(|node| matches!(node.kind, CompositionGraphNodeKind::Root))
    else {
        unreachable!("validation proved every composition graph has a root");
    };
    let root_node_id = root.id.clone();
    let layer_node_id = next_node_id(composition)?;
    typed_name(&name)?;
    let name = fresh_name(&name, |candidate| layer_name_taken(composition, candidate));
    let created = name.as_str().to_string();
    composition.layers.push(Layer {
        id: LayerId(next_layer_id),
        name,
        description: None,
        color: parse_color(&color)?,
        enabled: true,
    });
    let (x, y) = position.unwrap_or((80.0, 120.0 + next_layer_id as f32 * 80.0));
    let graph = &mut composition.graph;
    graph.nodes.push(CompositionGraphNode {
        id: layer_node_id.clone(),
        position: GraphNodePosition { x, y },
        kind: CompositionGraphNodeKind::Layer {
            layer_id: LayerId(next_layer_id),
        },
    });
    if connect_to_root {
        graph.edges.push(EffectGraphEdge {
            from: layer_node_id,
            to: root_node_id,
            input: None,
        });
    }
    Ok(created)
}

fn connect_nodes(
    composition: &mut Composition,
    connection: SequenceGraphEdge,
    previous: Option<SequenceGraphEdge>,
) -> Result<(), GuiMutationError> {
    let edge = graph_edge(connection)?;
    let (from, to) = (edge.from.clone(), edge.to.clone());
    if from == to {
        return Err(GuiMutationError::Invalid(
            "Graph node cannot connect to itself.".into(),
        ));
    }
    ensure_node_exists(composition, &from)?;
    ensure_node_exists(composition, &to)?;
    let graph = &mut composition.graph;
    if let Some(previous) = previous {
        let previous = graph_edge(previous)?;
        let index = graph
            .edges
            .iter()
            .position(|edge| *edge == previous)
            .ok_or_else(|| GuiMutationError::Invalid("The connection no longer exists.".into()))?;
        graph.edges.remove(index);
    }
    if !graph.edges.contains(&edge) {
        graph.edges.push(edge);
    }
    Ok(())
}

fn move_nodes(
    composition: &mut Composition,
    positions: Vec<SequenceGraphNodePosition>,
) -> Result<(), GuiMutationError> {
    for position in positions {
        if !position.x.is_finite() || !position.y.is_finite() {
            return Err(GuiMutationError::Invalid(
                "Node positions must be finite.".into(),
            ));
        }
        let id = CompositionGraphNodeId(position.node_id);
        node_mut(composition, &id)?.position = GraphNodePosition {
            x: position.x,
            y: position.y,
        };
    }
    Ok(())
}

// The caller owns the candidate session and commits the whole selection as one history entry.
// What played on deleted layers and what referred to deleted nodes go as repairs.
fn delete_items(
    composition: &mut Composition,
    node_ids: Vec<u32>,
    layer_ids: Vec<u32>,
    edges: Vec<SequenceGraphEdge>,
) -> Result<(), GuiMutationError> {
    let mut ids = node_ids
        .iter()
        .map(|id| CompositionGraphNodeId(*id))
        .collect::<BTreeSet<_>>();
    let mut layer_ids = layer_ids.into_iter().collect::<BTreeSet<_>>();
    for id in &ids {
        let node = node_mut(composition, id)?;
        match &node.kind {
            CompositionGraphNodeKind::Root => {
                return Err(GuiMutationError::Invalid(
                    "Output node cannot be deleted.".into(),
                ));
            }
            CompositionGraphNodeKind::Layer { layer_id } => {
                layer_ids.insert(layer_id.0);
            }
            CompositionGraphNodeKind::Operator(_) => {}
        }
    }
    if !layer_ids.is_empty() {
        for id in &layer_ids {
            if !composition.layers.iter().any(|layer| layer.id.0 == *id) {
                return Err(GuiMutationError::Invalid("Layer was not found.".into()));
            }
        }
        ids.extend(
            composition
                .graph
                .nodes
                .iter()
                .filter_map(|node| match &node.kind {
                    CompositionGraphNodeKind::Layer { layer_id }
                        if layer_ids.contains(&layer_id.0) =>
                    {
                        Some(node.id.clone())
                    }
                    _ => None,
                }),
        );
        composition
            .layers
            .retain(|layer| !layer_ids.contains(&layer.id.0));
    }
    let removed_edges = edges
        .into_iter()
        .map(graph_edge)
        .collect::<Result<Vec<_>, _>>()?;
    let graph = &mut composition.graph;
    graph.nodes.retain(|node| !ids.contains(&node.id));
    graph.edges.retain(|edge| {
        !ids.contains(&edge.from) && !ids.contains(&edge.to) && !removed_edges.contains(edge)
    });
    Ok(())
}

fn graph_edge(edge: SequenceGraphEdge) -> Result<EffectGraphEdge, GuiMutationError> {
    Ok(EffectGraphEdge {
        from: CompositionGraphNodeId(edge.from_node),
        to: CompositionGraphNodeId(edge.to_node),
        input: edge.input.as_deref().map(identifier).transpose()?,
    })
}
