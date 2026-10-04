use std::collections::BTreeSet;

use donder_language::sequence::{
    AutomationDetachmentReason, AutomationTarget, CompositionGraphNodeKind, GraphNodePosition,
    Sequence, SequenceLayerId,
};

use super::GuiMutationError;
use super::model::{composition_graph_node_mut, parse_graph_node_id};
use crate::dto::{SequenceGraphEdge, SequenceGraphNodePosition};

pub(super) fn connect_nodes(
    sequence: &mut Sequence,
    definitions: &donder_language::operator::OperatorDefinitionStore,
    connection: SequenceGraphEdge,
    previous: Option<SequenceGraphEdge>,
) -> Result<(), GuiMutationError> {
    use super::model::{ensure_graph_node_exists, graph_input_cardinality};
    use donder_language::operator::{OperatorPortCardinality, validate_composition_graph};
    use donder_language::sequence::{EffectGraphEdge, GraphPortId};

    let from = parse_graph_node_id(&connection.from_node)?;
    let to = parse_graph_node_id(&connection.to_node)?;
    if from == to {
        return Err(GuiMutationError::Invalid(
            "Graph node cannot connect to itself.".into(),
        ));
    }
    ensure_graph_node_exists(sequence, &from)?;
    ensure_graph_node_exists(sequence, &to)?;
    let graph = &mut sequence.composition_graph;
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
        let single_input = graph
            .nodes
            .iter()
            .find(|node| node.id == edge.to)
            .and_then(|node| graph_input_cardinality(definitions, &node.kind, &edge.to_port.0))
            == Some(OperatorPortCardinality::One);
        if single_input {
            graph
                .edges
                .retain(|existing| existing.to != edge.to || existing.to_port != edge.to_port);
        }
        graph.edges.push(edge);
    }
    // DesktopState discards this candidate on failure; no partial disconnect reaches history.
    validate_composition_graph(graph, definitions)
        .map_err(|error| GuiMutationError::Invalid(error.message))
}

pub(super) fn move_nodes(
    sequence: &mut Sequence,
    positions: Vec<SequenceGraphNodePosition>,
) -> Result<(), GuiMutationError> {
    for position in positions {
        if !position.x.is_finite() || !position.y.is_finite() {
            return Err(GuiMutationError::Invalid(
                "Node positions must be finite.".into(),
            ));
        }
        let id = parse_graph_node_id(&position.node_id)?;
        composition_graph_node_mut(sequence, &id)?.position = GraphNodePosition {
            x: position.x,
            y: position.y,
        };
    }
    Ok(())
}

// The caller owns the candidate session and commits the whole selection as one history entry.
pub(super) fn delete_items(
    sequence: &mut Sequence,
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
        let node = composition_graph_node_mut(sequence, id)?;
        match &node.kind {
            CompositionGraphNodeKind::Output => {
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
            if *id == 0 || !sequence.layers.iter().any(|layer| layer.id.0 == *id) {
                return Err(GuiMutationError::Invalid(
                    "Layer is missing or protected.".into(),
                ));
            }
        }
        ids.extend(
            sequence
                .composition_graph
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
                !layer_ids.contains(id) && sequence.layers.iter().any(|layer| layer.id.0 == *id)
            })
            .ok_or_else(|| {
                GuiMutationError::Invalid("Choose a surviving layer for the effects.".into())
            })?;
        for effect in &mut sequence.effects {
            if layer_ids.contains(&effect.layer_id.0) {
                effect.layer_id = SequenceLayerId(destination);
            }
        }
        sequence
            .layers
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
    sequence
        .composition_graph
        .nodes
        .retain(|node| !ids.contains(&node.id));
    sequence.composition_graph.edges.retain(|edge| {
        !ids.contains(&edge.from)
            && !ids.contains(&edge.to)
            && !removed_edges.contains(&(
                edge.from.clone(),
                edge.from_port.0.as_str(),
                edge.to.clone(),
                edge.to_port.0.as_str(),
            ))
    });
    for clip in &mut sequence.automation_clips {
        clip.detach_bindings(AutomationDetachmentReason::TargetDeleted, |target| {
            matches!(target, AutomationTarget::CompositionNodeParam { node_id, .. } if ids.contains(node_id))
        });
    }
    Ok(())
}
