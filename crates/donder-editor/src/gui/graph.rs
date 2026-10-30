//! Layers and the composition graph, which sequences and rigs share.
use std::collections::BTreeSet;

use donder_model::{
    AutomationTarget, CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind,
    EffectGraphEdge, GraphNodePosition, GraphOperatorNode, GraphPortId, ParamValue, Rig, Sequence,
    SequenceCompositionGraph, SequenceLayer, SequenceLayerId, validate_composition_graph,
};
use donder_project_io::{DocumentId, ProjectSession, ensure_document_can_reference_object};
use indexmap::IndexMap;

use super::GuiMutationError;
use super::model::{
    fresh_name, graph_operator_from_gui, identifier, param_value_from_gui, parse_color,
    parse_graph_node_id, typed_name,
};
use crate::dto::{CompositionGuiEdit, SequenceGraphEdge, SequenceGraphNodePosition};

/// An object that composes its layers through a graph.
pub(super) trait Composition {
    fn layers(&self) -> &[SequenceLayer];
    fn layers_mut(&mut self) -> &mut Vec<SequenceLayer>;
    fn graph(&self) -> &SequenceCompositionGraph;
    fn graph_mut(&mut self) -> &mut SequenceCompositionGraph;
    /// Move what plays on the `removed` layers to `destination`.
    fn move_layer_contents(&mut self, removed: &BTreeSet<u32>, destination: SequenceLayerId);
    /// Forget what refers to the removed graph nodes.
    fn forget_nodes(&mut self, removed: &BTreeSet<CompositionGraphNodeId>);
}

impl Composition for Sequence {
    fn layers(&self) -> &[SequenceLayer] {
        &self.layers
    }
    fn layers_mut(&mut self) -> &mut Vec<SequenceLayer> {
        &mut self.layers
    }
    fn graph(&self) -> &SequenceCompositionGraph {
        &self.composition_graph
    }
    fn graph_mut(&mut self) -> &mut SequenceCompositionGraph {
        &mut self.composition_graph
    }
    fn move_layer_contents(&mut self, removed: &BTreeSet<u32>, destination: SequenceLayerId) {
        for clip in &mut self.clips {
            if removed.contains(&clip.layer_id.0) {
                std::sync::Arc::make_mut(clip).layer_id = destination.clone();
            }
        }
    }
    fn forget_nodes(&mut self, removed: &BTreeSet<CompositionGraphNodeId>) {
        for clip in &mut self.automation_clips {
            clip.remove_bindings(|target| {
                matches!(target, AutomationTarget::CompositionNodeParam { node_id, .. } if removed.contains(node_id))
            });
        }
    }
}

impl Composition for Rig {
    fn layers(&self) -> &[SequenceLayer] {
        &self.layers
    }
    fn layers_mut(&mut self) -> &mut Vec<SequenceLayer> {
        &mut self.layers
    }
    fn graph(&self) -> &SequenceCompositionGraph {
        &self.composition_graph
    }
    fn graph_mut(&mut self) -> &mut SequenceCompositionGraph {
        &mut self.composition_graph
    }
    fn move_layer_contents(&mut self, removed: &BTreeSet<u32>, destination: SequenceLayerId) {
        for clip in self.clips_mut() {
            if removed.contains(&clip.layer_id.0) {
                clip.layer_id = destination.clone();
            }
        }
    }
    fn forget_nodes(&mut self, _removed: &BTreeSet<CompositionGraphNodeId>) {}
}

/// Apply a layer or graph edit to `composition`, which `owner` declares.
pub(super) fn edit_composition(
    session: &mut ProjectSession,
    owner: &DocumentId,
    composition: &mut impl Composition,
    edit: CompositionGuiEdit,
) -> Result<Vec<String>, GuiMutationError> {
    match edit {
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
            let definition = session
                .project
                .get::<donder_model::OperatorDefinition>(operator)
                .cloned()
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Operator definition was not found.".to_string())
                })?;
            ensure_document_can_reference_object(session, owner, operator.into())
                .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
            let mut params = IndexMap::new();
            for declaration in definition.params() {
                if declaration.default.is_none() {
                    let value = ParamValue::initial_for_param(declaration, initial_color)
                        .ok_or_else(|| {
                            GuiMutationError::Invalid(
                                "A valid required operator parameter could not be created."
                                    .to_string(),
                            )
                        })?;
                    params.insert(declaration.name.clone(), value);
                }
            }
            let next_id = next_node_id(composition);
            let name = fresh_name(definition.name(), |candidate| {
                name_taken(composition, candidate)
            });
            composition.graph_mut().nodes.push(CompositionGraphNode {
                id: CompositionGraphNodeId(next_id),
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
            migrate_to_layer_id,
        } => {
            delete_items(composition, node_ids, layer_ids, edges, migrate_to_layer_id)?;
        }
        CompositionGuiEdit::ConnectGraphNodes {
            from_node,
            from_port,
            to_node,
            to_port,
        } => {
            let definitions = session
                .project
                .reusable::<donder_model::OperatorDefinition>()
                .clone();
            connect_nodes(
                composition,
                &definitions,
                SequenceGraphEdge {
                    from_node,
                    from_port,
                    to_node,
                    to_port,
                },
                None,
            )?;
        }
        CompositionGuiEdit::ReconnectGraphEdge {
            previous,
            connection,
        } => {
            let definitions = session
                .project
                .reusable::<donder_model::OperatorDefinition>()
                .clone();
            connect_nodes(composition, &definitions, connection, Some(previous))?;
        }
        CompositionGuiEdit::UpdateGraphOperatorParam {
            node_id,
            name,
            value,
        } => {
            let value = param_value_from_gui(session, owner, value)?;
            let definitions = session
                .project
                .reusable::<donder_model::OperatorDefinition>()
                .clone();
            let node_id = parse_graph_node_id(&node_id)?;
            let mut graph = composition.graph().clone();
            let node = graph
                .nodes
                .iter_mut()
                .find(|node| node.id == node_id)
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Graph node was not found.".to_string())
                })?;
            let CompositionGraphNodeKind::Operator(operator) = &mut node.kind else {
                return Err(GuiMutationError::Invalid(
                    "Graph node is not an operator.".to_string(),
                ));
            };
            operator.params.insert(identifier(&name)?, value);
            validate_composition_graph(&graph, &definitions)
                .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
            *composition.graph_mut() = graph;
        }
    }
    Ok(Vec::new())
}

pub(super) fn layer_mut(
    composition: &mut impl Composition,
    id: u32,
) -> Result<&mut SequenceLayer, GuiMutationError> {
    composition
        .layers_mut()
        .iter_mut()
        .find(|layer| layer.id.0 == id)
        .ok_or_else(|| GuiMutationError::Invalid("Layer was not found.".to_string()))
}

pub(super) fn node_mut<'a>(
    composition: &'a mut impl Composition,
    id: &CompositionGraphNodeId,
) -> Result<&'a mut CompositionGraphNode, GuiMutationError> {
    composition
        .graph_mut()
        .nodes
        .iter_mut()
        .find(|node| node.id == *id)
        .ok_or_else(|| GuiMutationError::Invalid("Graph node was not found.".to_string()))
}

fn ensure_node_exists(
    composition: &impl Composition,
    node_id: &CompositionGraphNodeId,
) -> Result<(), GuiMutationError> {
    if composition
        .graph()
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

fn next_node_id(composition: &impl Composition) -> u32 {
    composition
        .graph()
        .nodes
        .iter()
        .map(|node| node.id.0)
        .max()
        .unwrap_or(0)
        + 1
}

/// Whether a name is used by a layer, graph node or the root node: they
/// share the graph's namespace.
fn name_taken(composition: &impl Composition, name: &str) -> bool {
    name == "root"
        || composition
            .layers()
            .iter()
            .any(|layer| layer.name.as_str() == name)
        || composition.graph().nodes.iter().any(|node| {
            matches!(&node.kind, CompositionGraphNodeKind::Operator(operator) if operator.name.as_str() == name)
        })
}

fn create_layer(
    composition: &mut impl Composition,
    name: String,
    color: String,
    position: Option<(f32, f32)>,
    connect_to_root: bool,
) -> Result<String, GuiMutationError> {
    let next_layer_id = composition
        .layers()
        .iter()
        .map(|layer| layer.id.0)
        .max()
        .unwrap_or(0)
        + 1;
    let root_node_id = composition
        .graph()
        .nodes
        .iter()
        .find(|node| matches!(node.kind, CompositionGraphNodeKind::Root))
        .map(|node| node.id.clone())
        .ok_or_else(|| {
            GuiMutationError::Invalid("Composition graph root was not found.".to_string())
        })?;
    let layer_node_id = CompositionGraphNodeId(next_node_id(composition));
    typed_name(&name)?;
    let name = fresh_name(&name, |candidate| name_taken(composition, candidate));
    let created = name.as_str().to_string();
    composition.layers_mut().push(SequenceLayer {
        id: SequenceLayerId(next_layer_id),
        name,
        description: None,
        color: parse_color(&color)?,
        enabled: true,
    });
    let (x, y) = position.unwrap_or((80.0, 120.0 + next_layer_id as f32 * 80.0));
    let graph = composition.graph_mut();
    graph.nodes.push(CompositionGraphNode {
        id: layer_node_id.clone(),
        position: GraphNodePosition { x, y },
        kind: CompositionGraphNodeKind::Layer {
            layer_id: SequenceLayerId(next_layer_id),
        },
    });
    if connect_to_root {
        graph.edges.push(EffectGraphEdge {
            from: layer_node_id,
            from_port: GraphPortId("output".to_string()),
            to: root_node_id,
            to_port: GraphPortId("input".to_string()),
        });
    }
    Ok(created)
}

fn connect_nodes(
    composition: &mut impl Composition,
    definitions: &indexmap::IndexMap<
        donder_model::OperatorDefinitionId,
        donder_model::OperatorDefinition,
    >,
    connection: SequenceGraphEdge,
    previous: Option<SequenceGraphEdge>,
) -> Result<(), GuiMutationError> {
    let from = parse_graph_node_id(&connection.from_node)?;
    let to = parse_graph_node_id(&connection.to_node)?;
    if from == to {
        return Err(GuiMutationError::Invalid(
            "Graph node cannot connect to itself.".into(),
        ));
    }
    ensure_node_exists(composition, &from)?;
    ensure_node_exists(composition, &to)?;
    let graph = composition.graph_mut();
    if let Some(previous) = previous {
        let old_from = parse_graph_node_id(&previous.from_node)?;
        let old_to = parse_graph_node_id(&previous.to_node)?;
        let index = graph
            .edges
            .iter()
            .position(|edge| {
                edge.from == old_from
                    && edge.to == old_to
                    && edge.from_port.0 == previous.from_port
                    && edge.to_port.0 == previous.to_port
            })
            .ok_or_else(|| GuiMutationError::Invalid("The connection no longer exists.".into()))?;
        graph.edges.remove(index);
    }
    let edge = EffectGraphEdge {
        from,
        to,
        from_port: GraphPortId(connection.from_port),
        to_port: GraphPortId(connection.to_port),
    };
    if !graph.edges.contains(&edge) {
        graph.edges.push(edge);
    }
    // DesktopState discards this candidate on failure; no partial disconnect reaches history.
    validate_composition_graph(graph, definitions)
        .map_err(|error| GuiMutationError::Invalid(error.to_string()))
}

fn move_nodes(
    composition: &mut impl Composition,
    positions: Vec<SequenceGraphNodePosition>,
) -> Result<(), GuiMutationError> {
    for position in positions {
        if !position.x.is_finite() || !position.y.is_finite() {
            return Err(GuiMutationError::Invalid(
                "Node positions must be finite.".into(),
            ));
        }
        let id = parse_graph_node_id(&position.node_id)?;
        node_mut(composition, &id)?.position = GraphNodePosition {
            x: position.x,
            y: position.y,
        };
    }
    Ok(())
}

// The caller owns the candidate session and commits the whole selection as one history entry.
fn delete_items(
    composition: &mut impl Composition,
    node_ids: Vec<String>,
    layer_ids: Vec<u32>,
    edges: Vec<SequenceGraphEdge>,
    migrate_to_layer_id: Option<u32>,
) -> Result<(), GuiMutationError> {
    let mut ids = node_ids
        .iter()
        .map(|id| parse_graph_node_id(id))
        .collect::<Result<BTreeSet<_>, _>>()?;
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
                if layer_id.0 == 0 {
                    return Err(GuiMutationError::Invalid(
                        "Default layer cannot be deleted.".into(),
                    ));
                }
                layer_ids.insert(layer_id.0);
            }
            CompositionGraphNodeKind::Operator(_) => {}
        }
    }
    if !layer_ids.is_empty() {
        for id in &layer_ids {
            if *id == 0 || !composition.layers().iter().any(|layer| layer.id.0 == *id) {
                return Err(GuiMutationError::Invalid(
                    "Layer is missing or protected.".into(),
                ));
            }
        }
        ids.extend(
            composition
                .graph()
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
        let destination = migrate_to_layer_id
            .filter(|id| {
                !layer_ids.contains(id)
                    && composition.layers().iter().any(|layer| layer.id.0 == *id)
            })
            .ok_or_else(|| {
                GuiMutationError::Invalid("Choose a surviving layer for the clips.".into())
            })?;
        composition.move_layer_contents(&layer_ids, SequenceLayerId(destination));
        composition
            .layers_mut()
            .retain(|layer| !layer_ids.contains(&layer.id.0));
    }
    let removed_edges = edges
        .iter()
        .map(|edge| {
            Ok((
                parse_graph_node_id(&edge.from_node)?,
                edge.from_port.as_str(),
                parse_graph_node_id(&edge.to_node)?,
                edge.to_port.as_str(),
            ))
        })
        .collect::<Result<BTreeSet<_>, GuiMutationError>>()?;
    let graph = composition.graph_mut();
    graph.nodes.retain(|node| !ids.contains(&node.id));
    graph.edges.retain(|edge| {
        !ids.contains(&edge.from)
            && !ids.contains(&edge.to)
            && !removed_edges.contains(&(
                edge.from.clone(),
                edge.from_port.0.as_str(),
                edge.to.clone(),
                edge.to_port.0.as_str(),
            ))
    });
    composition.forget_nodes(&ids);
    Ok(())
}
