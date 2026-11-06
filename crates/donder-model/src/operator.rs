use crate::effect::ParamValue;
pub(crate) use crate::identity::OperatorDefinitionId;
use crate::sequence::{
    CompositionGraphNodeId, CompositionGraphNodeKind, EffectGraphEdge, SequenceCompositionGraph,
};
use donder_language::compiler::{CompiledDefinition, OperatorInputDecl, ParamDecl};
use donder_runtime_types::{Identifier, Type};
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq)]
pub struct GraphOperatorNode {
    /// Unique among the sequence's layers and graph nodes; edges use it.
    pub name: donder_runtime_types::Identifier,
    pub operator: OperatorDefinitionId,
    pub params: IndexMap<Identifier, ParamValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OperatorDefinition {
    pub(crate) id: OperatorDefinitionId,
    compiled: Box<CompiledDefinition>,
}

impl OperatorDefinition {
    pub fn new(id: OperatorDefinitionId, compiled: CompiledDefinition) -> Self {
        Self {
            id,
            compiled: Box::new(compiled),
        }
    }
    pub fn id(&self) -> &OperatorDefinitionId {
        &self.id
    }
    pub fn name(&self) -> &str {
        self.compiled.name().as_str()
    }
    /// The operator's inputs, in declaration order.
    pub fn inputs(&self) -> impl Iterator<Item = &Identifier> {
        self.compiled
            .inputs()
            .iter()
            .map(|OperatorInputDecl { name }| name)
    }
    pub fn params(&self) -> &[ParamDecl] {
        self.compiled.params()
    }
    pub(crate) fn compiled(&self) -> &CompiledDefinition {
        &self.compiled
    }
    pub fn description(&self) -> Option<&str> {
        self.compiled.description()
    }
}

/// Why a composition graph cannot be evaluated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphValidationError {
    DuplicateNode(CompositionGraphNodeId),
    MissingOperator(CompositionGraphNodeId),
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
            | Self::MissingOperator(node)
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
            Self::MissingOperator(_) => f.write_str("the operator definition is missing"),
            Self::RootCount(count) => write!(
                f,
                "a composition graph has exactly one root node, not {count}"
            ),
            Self::MissingEdgeNode(_) => f.write_str("an edge names a missing graph node"),
            Self::EdgeFromRoot(_) => f.write_str("the root has no output"),
            Self::UnknownInput {
                input: Some(input), ..
            } => write!(f, "no input `{}`", input.as_str()),
            Self::UnknownInput { input: None, .. } => {
                f.write_str("an edge to an operator names its input")
            }
            Self::DuplicateEdge(_) => f.write_str("an edge appears twice"),
            Self::Cycle => f.write_str("the composition graph contains a cycle"),
        }
    }
}

pub fn validate_composition_graph(
    graph: &SequenceCompositionGraph,
    definitions: &IndexMap<OperatorDefinitionId, OperatorDefinition>,
) -> Result<(), GraphValidationError> {
    let mut node_ids = HashSet::new();
    let mut root_count = 0usize;
    for node in &graph.nodes {
        if !node_ids.insert(&node.id) {
            return Err(GraphValidationError::DuplicateNode(node.id.clone()));
        }
        match &node.kind {
            CompositionGraphNodeKind::Operator(operator) => {
                if !definitions.contains_key(&operator.operator) {
                    return Err(GraphValidationError::MissingOperator(node.id.clone()));
                }
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
    graph: &SequenceCompositionGraph,
) -> HashSet<crate::sequence::CompositionGraphNodeId> {
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
        &'a crate::sequence::CompositionGraphNodeId,
        &'a crate::sequence::CompositionGraphNode,
    >,
    definitions: &IndexMap<OperatorDefinitionId, OperatorDefinition>,
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
        (CompositionGraphNodeKind::Operator(operator), Some(input)) => definitions
            .get(&operator.operator)
            .ok_or_else(|| GraphValidationError::MissingOperator(to.id.clone()))?
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

fn validate_acyclic(graph: &SequenceCompositionGraph) -> Result<(), GraphValidationError> {
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

pub fn param_matches_type(value: &ParamValue, ty: &Type) -> bool {
    match (value, ty) {
        (ParamValue::Int(_), Type::Int)
        | (ParamValue::Float(_), Type::Float)
        | (ParamValue::Bool(_), Type::Bool)
        | (ParamValue::Color(_), Type::Color)
        | (ParamValue::Marks(_), Type::Marks)
        | (ParamValue::Curve(_), Type::Curve)
        | (ParamValue::Gradient(_), Type::Gradient) => true,
        (ParamValue::Enum(value), Type::Enum(options)) => options.contains(value),
        (ParamValue::Array(values), Type::Array(item_type)) => values
            .iter()
            .all(|value| param_matches_type(value, item_type)),
        _ => false,
    }
}
