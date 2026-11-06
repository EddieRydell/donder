//! Layers and the composition graph, and the clips and mark collections on
//! them, which sequences and consoles share.
use std::collections::BTreeSet;

use donder_model::{
    Action, AutomationTarget, CompositionGraphNode, CompositionGraphNodeId,
    CompositionGraphNodeKind, Console, EffectDefinitionId, EffectGraphEdge, GraphNodePosition,
    GraphOperatorNode, MarkCollectionKey, ParamValue, Sequence, SequenceCompositionGraph,
    SequenceLayer, SequenceLayerId,
};
use donder_project_io::{DocumentId, ProjectSession, ensure_document_can_reference_object};
use donder_runtime_types::Identifier;
use indexmap::IndexMap;

use super::model::{
    fresh_name, graph_operator_from_gui, identifier, imported_effect, names_marks,
    param_value_from_gui, parse_color, replace_marks, required_param_values, typed_name,
};
use super::{GuiMutationError, next_id};
use crate::dto::{
    CompositionGuiEdit, SequenceEffectReference, SequenceGraphEdge, SequenceGraphNodePosition,
};

type Params = IndexMap<Identifier, ParamValue>;

/// An object that composes its layers through a graph: a sequence or a
/// console, whose clips play on the layers and may read its mark collections.
pub(super) trait Composition {
    fn layers(&self) -> &[SequenceLayer];
    fn layers_mut(&mut self) -> &mut Vec<SequenceLayer>;
    fn graph(&self) -> &SequenceCompositionGraph;
    fn graph_mut(&mut self) -> &mut SequenceCompositionGraph;
    /// The id and name of every clip.
    fn clips(&self) -> impl Iterator<Item = (u32, &Identifier)>;
    /// Edit the parameters of the clips `touches` selects; no other clip changes.
    fn edit_clip_params(
        &mut self,
        touches: &dyn Fn(&Params) -> bool,
        edit: &mut dyn FnMut(&mut Params),
    );
    /// Rename the mark collection named `key` to `to`, or remove it when `to`
    /// is `None`, with what besides parameters names it: the old key.
    fn replace_mark_collection(
        &mut self,
        key: &str,
        to: Option<&MarkCollectionKey>,
    ) -> Result<MarkCollectionKey, GuiMutationError>;
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
    fn clips(&self) -> impl Iterator<Item = (u32, &Identifier)> {
        self.clips.iter().map(|clip| (clip.id.0, &clip.name))
    }
    fn edit_clip_params(
        &mut self,
        touches: &dyn Fn(&Params) -> bool,
        edit: &mut dyn FnMut(&mut Params),
    ) {
        // Clips are shared with earlier snapshots; only an edited clip is copied.
        for clip in &mut self.clips {
            if touches(&clip.param_overrides) {
                edit(&mut std::sync::Arc::make_mut(clip).param_overrides);
            }
        }
    }
    fn replace_mark_collection(
        &mut self,
        key: &str,
        to: Option<&MarkCollectionKey>,
    ) -> Result<MarkCollectionKey, GuiMutationError> {
        let index = self
            .mark_collections
            .iter()
            .position(|collection| collection.key.name.as_str() == key)
            .ok_or_else(|| GuiMutationError::Invalid("Mark collection was not found.".into()))?;
        Ok(match to {
            Some(to) => std::mem::replace(&mut self.mark_collections[index].key, to.clone()),
            None => self.mark_collections.remove(index).key,
        })
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

impl Composition for Console {
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
    fn clips(&self) -> impl Iterator<Item = (u32, &Identifier)> {
        self.clips.iter().map(|clip| (clip.id.0, &clip.name))
    }
    fn edit_clip_params(
        &mut self,
        touches: &dyn Fn(&Params) -> bool,
        edit: &mut dyn FnMut(&mut Params),
    ) {
        for clip in &mut self.clips {
            if touches(&clip.param_overrides) {
                edit(&mut clip.param_overrides);
            }
        }
    }
    fn replace_mark_collection(
        &mut self,
        key: &str,
        to: Option<&MarkCollectionKey>,
    ) -> Result<MarkCollectionKey, GuiMutationError> {
        let index = self
            .live_marks
            .iter()
            .position(|marks| marks.key.name.as_str() == key)
            .ok_or_else(|| {
                GuiMutationError::Invalid("Live mark collection was not found.".into())
            })?;
        let from = match to {
            Some(to) => std::mem::replace(&mut self.live_marks[index].key, to.clone()),
            None => self.live_marks.remove(index).key,
        };
        // Actions adding marks follow a rename and go with a removal.
        for on in &mut self.events {
            on.actions.retain_mut(|action| match (action, to) {
                (Action::Mark(key), Some(to)) if *key == from => {
                    *key = to.clone();
                    true
                }
                (Action::Mark(key), None) => *key != from,
                _ => true,
            });
        }
        Ok(from)
    }
    fn move_layer_contents(&mut self, removed: &BTreeSet<u32>, destination: SequenceLayerId) {
        for clip in &mut self.clips {
            if removed.contains(&clip.layer_id.0) {
                clip.layer_id = destination.clone();
            }
        }
    }
    fn forget_nodes(&mut self, removed: &BTreeSet<CompositionGraphNodeId>) {
        for node in removed {
            self.forget_node(node);
        }
    }
}

/// Rename the mark collection named `key` to `to`, or remove it when `to` is
/// `None`, moving every clip and operator parameter that names it.
pub(super) fn replace_mark_collection(
    composition: &mut impl Composition,
    key: &str,
    to: Option<MarkCollectionKey>,
) -> Result<(), GuiMutationError> {
    let to = to.as_ref();
    let from = composition.replace_mark_collection(key, to)?;
    composition.edit_clip_params(
        &|params| params.values().any(|value| names_marks(value, &from)),
        &mut |params| {
            for value in params.values_mut() {
                replace_marks(value, &from, to);
            }
        },
    );
    for node in &mut composition.graph_mut().nodes {
        if let CompositionGraphNodeKind::Operator(operator) = &mut node.kind {
            for value in operator.params.values_mut() {
                replace_marks(value, &from, to);
            }
        }
    }
    Ok(())
}

/// What a new clip of an effect starts with: the next clip id, a name after
/// the effect, the composition's first layer and the values of the
/// parameters without defaults.
pub(super) struct NewClip {
    pub(super) id: u32,
    pub(super) name: Identifier,
    pub(super) layer_id: SequenceLayerId,
    pub(super) definition: EffectDefinitionId,
    pub(super) param_overrides: Params,
}

/// A new clip on `composition` of the effect `reference` names, which
/// `owner` then imports.
pub(super) fn new_clip(
    session: &mut ProjectSession,
    owner: &DocumentId,
    composition: &impl Composition,
    reference: SequenceEffectReference,
    initial_color: &str,
) -> Result<NewClip, GuiMutationError> {
    let layer_id = composition
        .layers()
        .first()
        .map(|layer| layer.id.clone())
        .ok_or_else(|| GuiMutationError::Invalid("A clip needs a layer to go on.".into()))?;
    let (definition, effect_name, param_overrides) =
        imported_effect(session, owner, reference, initial_color)?;
    Ok(NewClip {
        id: next_id(composition.clips().map(|(id, _)| id))?,
        name: fresh_name(&effect_name, |candidate| {
            composition
                .clips()
                .any(|(_, name)| name.as_str() == candidate)
        }),
        layer_id,
        definition,
        param_overrides,
    })
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
                .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
            let params = required_param_values(definition.params(), initial_color)?;
            let id = next_node_id(composition)?;
            let name = fresh_name(definition.name(), |candidate| {
                name_taken(composition, candidate)
            });
            composition.graph_mut().nodes.push(CompositionGraphNode {
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
            migrate_to_layer_id,
        } => {
            delete_items(composition, node_ids, layer_ids, edges, migrate_to_layer_id)?;
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
        CompositionGuiEdit::UpdateGraphOperatorParam {
            node_id,
            name,
            value,
        } => {
            let value = param_value_from_gui(session, owner, value)?;
            let node = node_mut(composition, &CompositionGraphNodeId(node_id))?;
            let CompositionGraphNodeKind::Operator(operator) = &mut node.kind else {
                return Err(GuiMutationError::Invalid(
                    "Graph node is not an operator.".to_string(),
                ));
            };
            operator.params.insert(identifier(&name)?, value);
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

fn next_node_id(
    composition: &impl Composition,
) -> Result<CompositionGraphNodeId, GuiMutationError> {
    next_id(composition.graph().nodes.iter().map(|node| node.id.0)).map(CompositionGraphNodeId)
}

/// Whether a name is used by a layer, graph node or the root node: they
/// share the graph's namespace.
fn name_taken(composition: &impl Composition, name: &str) -> bool {
    name == donder_language::data::ROOT_NODE
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
    let next_layer_id = next_id(composition.layers().iter().map(|layer| layer.id.0))?;
    let root_node_id = composition
        .graph()
        .nodes
        .iter()
        .find(|node| matches!(node.kind, CompositionGraphNodeKind::Root))
        .map(|node| node.id.clone())
        .ok_or_else(|| {
            GuiMutationError::Invalid("Composition graph root was not found.".to_string())
        })?;
    let layer_node_id = next_node_id(composition)?;
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
            to: root_node_id,
            input: None,
        });
    }
    Ok(created)
}

fn connect_nodes(
    composition: &mut impl Composition,
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
    let graph = composition.graph_mut();
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
    composition: &mut impl Composition,
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
fn delete_items(
    composition: &mut impl Composition,
    node_ids: Vec<u32>,
    layer_ids: Vec<u32>,
    edges: Vec<SequenceGraphEdge>,
    migrate_to_layer_id: Option<u32>,
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
            if !composition.layers().iter().any(|layer| layer.id.0 == *id) {
                return Err(GuiMutationError::Invalid("Layer was not found.".into()));
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
        .into_iter()
        .map(graph_edge)
        .collect::<Result<Vec<_>, _>>()?;
    let graph = composition.graph_mut();
    graph.nodes.retain(|node| !ids.contains(&node.id));
    graph.edges.retain(|edge| {
        !ids.contains(&edge.from) && !ids.contains(&edge.to) && !removed_edges.contains(edge)
    });
    composition.forget_nodes(&ids);
    Ok(())
}

fn graph_edge(edge: SequenceGraphEdge) -> Result<EffectGraphEdge, GuiMutationError> {
    Ok(EffectGraphEdge {
        from: CompositionGraphNodeId(edge.from_node),
        to: CompositionGraphNodeId(edge.to_node),
        input: edge.input.as_deref().map(identifier).transpose()?,
    })
}
