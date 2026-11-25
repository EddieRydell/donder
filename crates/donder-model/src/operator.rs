use crate::composition::{
    CompositionGraph, CompositionGraphNodeId, CompositionGraphNodeKind, EffectGraphEdge,
};
use crate::effect::ParamValue;
use crate::effect::{DefinitionProblem, ScriptDefinition, ScriptId};
use donder_language::compiler::DeclarationKind;
use donder_runtime_types::{Identifier, Type};
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq)]
pub struct GraphOperatorNode {
    /// Unique among the sequence's layers and graph nodes; edges use it.
    pub name: donder_runtime_types::Identifier,
    pub operator: ScriptId,
    pub params: IndexMap<Identifier, ParamValue>,
}

/// Why a composition graph cannot be evaluated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphValidationError {
    DuplicateNode(CompositionGraphNodeId),
    /// An operator node names no operator.
    Definition(CompositionGraphNodeId, DefinitionProblem),
    RootCount(usize),
    MissingEdgeNode(CompositionGraphNodeId),
    /// An edge leaves the root, which has no output.
    EdgeFromRoot(CompositionGraphNodeId),
    /// An edge names an input its target does not have.
    UnknownInput {
        node: CompositionGraphNodeId,
        input: Option<Identifier>,
    },
    DuplicateEdge(CompositionGraphNodeId),
    Cycle,
}

impl GraphValidationError {
    /// The node the error concerns.
    pub(crate) fn node(&self) -> Option<CompositionGraphNodeId> {
        match self {
            Self::DuplicateNode(node)
            | Self::Definition(node, _)
            | Self::MissingEdgeNode(node)
            | Self::DuplicateEdge(node)
            | Self::EdgeFromRoot(node)
            | Self::UnknownInput { node, .. } => Some(node.clone()),
            Self::RootCount(_) | Self::Cycle => None,
        }
    }
}

impl std::fmt::Display for GraphValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateNode(node) => write!(f, "graph node id {} is used twice", node.0),
            Self::Definition(_, problem) => problem.fmt(f),
            Self::RootCount(count) => write!(f, "the graph has {count} outputs instead of one"),
            Self::MissingEdgeNode(_) => f.write_str("a connection's node is missing"),
            Self::EdgeFromRoot(_) => f.write_str("the root has no output"),
            Self::UnknownInput {
                input: Some(input), ..
            } => write!(f, "no input {}", input.as_str()),
            Self::UnknownInput { input: None, .. } => {
                f.write_str("a connection to an operator names no input")
            }
            Self::DuplicateEdge(_) => f.write_str("a connection appears twice"),
            Self::Cycle => f.write_str("the graph loops back on itself"),
        }
    }
}

pub fn validate_composition_graph(
    graph: &CompositionGraph,
    definitions: &IndexMap<ScriptId, ScriptDefinition>,
) -> Result<(), GraphValidationError> {
    let mut node_ids = HashSet::new();
    let mut root_count = 0usize;
    for node in &graph.nodes {
        if !node_ids.insert(&node.id) {
            return Err(GraphValidationError::DuplicateNode(node.id.clone()));
        }
        match &node.kind {
            CompositionGraphNodeKind::Operator(operator) => {
                crate::effect::script_of_kind(
                    definitions,
                    operator.operator,
                    DeclarationKind::Operator,
                )
                .map_err(|problem| GraphValidationError::Definition(node.id.clone(), problem))?;
            }
            CompositionGraphNodeKind::Root => root_count += 1,
            CompositionGraphNodeKind::Layer { .. } => {}
        }
    }
    if root_count != 1 {
        return Err(GraphValidationError::RootCount(root_count));
    }

    let nodes = graph
        .nodes
        .iter()
        .map(|node| (&node.id, node))
        .collect::<HashMap<_, _>>();
    let mut edges = HashSet::new();
    for edge in &graph.edges {
        validate_edge(edge, &nodes, definitions, &mut edges)?;
    }
    validate_acyclic(graph)
}

/// Nodes that can contribute to the root; preparation skips the rest.
pub fn composition_graph_root_dependencies(
    graph: &CompositionGraph,
) -> HashSet<crate::composition::CompositionGraphNodeId> {
    let mut incoming = HashMap::<_, Vec<_>>::new();
    for edge in &graph.edges {
        incoming.entry(&edge.to).or_default().push(&edge.from);
    }
    let mut pending = graph
        .nodes
        .iter()
        .filter_map(|node| matches!(node.kind, CompositionGraphNodeKind::Root).then_some(&node.id))
        .collect::<Vec<_>>();
    let mut required = HashSet::new();
    while let Some(id) = pending.pop() {
        if required.insert(id.clone())
            && let Some(inputs) = incoming.get(id)
        {
            pending.extend(inputs.iter().copied());
        }
    }
    required
}

fn validate_edge<'a>(
    edge: &'a EffectGraphEdge,
    nodes: &HashMap<
        &'a crate::composition::CompositionGraphNodeId,
        &'a crate::composition::CompositionGraphNode,
    >,
    definitions: &IndexMap<ScriptId, ScriptDefinition>,
    edges: &mut HashSet<&'a EffectGraphEdge>,
) -> Result<(), GraphValidationError> {
    let from = nodes
        .get(&edge.from)
        .ok_or_else(|| GraphValidationError::MissingEdgeNode(edge.from.clone()))?;
    let to = nodes
        .get(&edge.to)
        .ok_or_else(|| GraphValidationError::MissingEdgeNode(edge.to.clone()))?;
    if matches!(from.kind, CompositionGraphNodeKind::Root) {
        return Err(GraphValidationError::EdgeFromRoot(edge.from.clone()));
    }
    let known = match (&to.kind, &edge.input) {
        (CompositionGraphNodeKind::Root, None) => true,
        // Every operator node's definition was found before edges are checked.
        (CompositionGraphNodeKind::Operator(operator), Some(input)) => definitions
            [&operator.operator]
            .inputs()
            .any(|declared| declared == input),
        _ => false,
    };
    if !known {
        return Err(GraphValidationError::UnknownInput {
            node: edge.to.clone(),
            input: edge.input.clone(),
        });
    }
    if !edges.insert(edge) {
        return Err(GraphValidationError::DuplicateEdge(edge.to.clone()));
    }
    Ok(())
}

fn validate_acyclic(graph: &CompositionGraph) -> Result<(), GraphValidationError> {
    let indexes = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (&node.id, index))
        .collect::<HashMap<_, _>>();
    let mut indegree = vec![0usize; graph.nodes.len()];
    let mut outgoing = vec![Vec::new(); graph.nodes.len()];
    for edge in &graph.edges {
        let Some(&from) = indexes.get(&edge.from) else {
            continue;
        };
        let Some(&to) = indexes.get(&edge.to) else {
            continue;
        };
        indegree[to] += 1;
        outgoing[from].push(to);
    }
    let mut ready = indegree
        .iter()
        .enumerate()
        .filter_map(|(index, degree)| (*degree == 0).then_some(index))
        .collect::<Vec<_>>();
    let mut visited = 0usize;
    while let Some(index) = ready.pop() {
        visited += 1;
        for next in &outgoing[index] {
            indegree[*next] = indegree[*next].saturating_sub(1);
            if indegree[*next] == 0 {
                ready.push(*next);
            }
        }
    }
    if visited == graph.nodes.len() {
        Ok(())
    } else {
        Err(GraphValidationError::Cycle)
    }
}

/// Whether `value` is a value of `declaration`: of its type, and for an
/// enum option with fields, every field of the option in order.
pub fn param_matches(
    value: &ParamValue,
    declaration: &donder_language::compiler::ParamDecl,
) -> bool {
    match value {
        ParamValue::Enum { option, fields } => {
            let declared = declaration.fields_of(option);
            matches!(&declaration.ty, Type::Enum(options) if options.contains(option))
                && fields.len() == declared.len()
                && fields.iter().zip(declared).all(|((name, value), field)| {
                    name == &field.name && param_matches(value, field)
                })
        }
        value => param_matches_type(value, &declaration.ty),
    }
}

fn param_matches_type(value: &ParamValue, ty: &Type) -> bool {
    match (value, ty) {
        (ParamValue::Int(_), Type::Int)
        | (ParamValue::Float(_), Type::Float)
        | (ParamValue::Bool(_), Type::Bool)
        | (ParamValue::Color(_), Type::Color)
        | (ParamValue::Marks(_), Type::Marks)
        | (ParamValue::Curve(_), Type::Curve)
        | (ParamValue::Gradient(_), Type::Gradient)
        | (ParamValue::Text(_), Type::Text)
        | (ParamValue::Font(_), Type::Font)
        | (ParamValue::Image(_), Type::Image) => true,
        (ParamValue::Enum { option, fields }, Type::Enum(options)) => {
            fields.is_empty() && options.contains(option)
        }
        (ParamValue::Array(values), Type::Array(item_type)) => values
            .iter()
            .all(|value| param_matches_type(value, item_type)),
        _ => false,
    }
}
