use crate::effect::EffectParamValue;
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
    pub params: IndexMap<Identifier, EffectParamValue>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperatorPortDefinition {
    pub source_name: String,
    pub display_name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OperatorDefinition {
    pub(crate) id: OperatorDefinitionId,
    pub(crate) compiled: Box<CompiledDefinition>,
    pub(crate) inputs: Vec<OperatorPortDefinition>,
    pub(crate) output: OperatorPortDefinition,
}

impl OperatorDefinition {
    pub fn new(id: OperatorDefinitionId, compiled: CompiledDefinition) -> Self {
        Self {
            id,
            inputs: compiled
                .inputs()
                .iter()
                .map(|OperatorInputDecl { name }| port(name.as_str(), name.as_str()))
                .collect(),
            output: port("output", "Output"),
            compiled: Box::new(compiled),
        }
    }
    pub fn id(&self) -> &OperatorDefinitionId {
        &self.id
    }
    pub fn name(&self) -> &str {
        self.compiled.name().as_str()
    }
    pub fn inputs(&self) -> &[OperatorPortDefinition] {
        &self.inputs
    }
    pub fn output(&self) -> &OperatorPortDefinition {
        &self.output
    }
    pub fn params(&self) -> &[ParamDecl] {
        self.compiled.params()
    }
    pub fn compiled(&self) -> &CompiledDefinition {
        &self.compiled
    }
    pub fn description(&self) -> Option<&str> {
        self.compiled.description()
    }
}

fn port(name: &str, display_name: &str) -> OperatorPortDefinition {
    OperatorPortDefinition {
        source_name: name.to_string(),
        display_name: display_name.to_string(),
    }
}
/// Why a composition graph cannot be evaluated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphValidationError {
    DuplicateNode(CompositionGraphNodeId),
    MissingOperator(CompositionGraphNodeId),
    OutputCount(usize),
    MissingEdgeNode(CompositionGraphNodeId),
    UnknownOutputPort {
        node: CompositionGraphNodeId,
        port: String,
    },
    UnknownInputPort {
        node: CompositionGraphNodeId,
        port: String,
    },
    DuplicateEdge(CompositionGraphNodeId),
    UnconnectedInput {
        node: CompositionGraphNodeId,
        port: String,
    },
    Cycle,
    UnknownParam {
        node: CompositionGraphNodeId,
        param: Identifier,
    },
    WrongParamType {
        node: CompositionGraphNodeId,
        param: Identifier,
    },
    MissingParam {
        node: CompositionGraphNodeId,
        param: Identifier,
    },
}

impl GraphValidationError {
    /// The node the error concerns.
    pub fn node(&self) -> Option<CompositionGraphNodeId> {
        match self {
            Self::DuplicateNode(node)
            | Self::MissingOperator(node)
            | Self::MissingEdgeNode(node)
            | Self::DuplicateEdge(node)
            | Self::UnknownOutputPort { node, .. }
            | Self::UnknownInputPort { node, .. }
            | Self::UnconnectedInput { node, .. }
            | Self::UnknownParam { node, .. }
            | Self::WrongParamType { node, .. }
            | Self::MissingParam { node, .. } => Some(node.clone()),
            Self::OutputCount(_) | Self::Cycle => None,
        }
    }
}

impl std::fmt::Display for GraphValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateNode(node) => write!(f, "graph node id {} is used twice", node.0),
            Self::MissingOperator(_) => f.write_str("the operator definition is missing"),
            Self::OutputCount(count) => write!(
                f,
                "a composition graph has exactly one output node, not {count}"
            ),
            Self::MissingEdgeNode(_) => f.write_str("an edge names a missing graph node"),
            Self::UnknownOutputPort { port, .. } => write!(f, "no output port `{port}`"),
            Self::UnknownInputPort { port, .. } => write!(f, "no input port `{port}`"),
            Self::DuplicateEdge(_) => f.write_str("an edge appears twice"),
            Self::UnconnectedInput { port, .. } => write!(f, "input `{port}` is not connected"),
            Self::Cycle => f.write_str("the composition graph contains a cycle"),
            Self::UnknownParam { param, .. } => write!(f, "no parameter `{}`", param.as_str()),
            Self::WrongParamType { param, .. } => {
                write!(f, "parameter `{}` has the wrong type", param.as_str())
            }
            Self::MissingParam { param, .. } => {
                write!(f, "required parameter `{}` is missing", param.as_str())
            }
        }
    }
}

pub fn validate_composition_graph(
    graph: &SequenceCompositionGraph,
    definitions: &IndexMap<OperatorDefinitionId, OperatorDefinition>,
) -> Result<(), GraphValidationError> {
    let mut node_ids = HashSet::new();
    let mut output_count = 0usize;
    for node in &graph.nodes {
        if !node_ids.insert(&node.id) {
            return Err(GraphValidationError::DuplicateNode(node.id.clone()));
        }
        match &node.kind {
            CompositionGraphNodeKind::Operator(operator) => {
                let definition = definitions
                    .get(&operator.operator)
                    .ok_or_else(|| GraphValidationError::MissingOperator(node.id.clone()))?;
                validate_operator_params(&node.id, operator, definition)?;
            }
            CompositionGraphNodeKind::Output => output_count += 1,
            CompositionGraphNodeKind::Layer { .. } => {}
        }
    }
    if output_count != 1 {
        return Err(GraphValidationError::OutputCount(output_count));
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
    let connected_inputs = graph
        .edges
        .iter()
        .map(|edge| (&edge.to, edge.to_port.0.as_str()))
        .collect::<HashSet<_>>();
    let output_dependencies = composition_graph_output_dependencies(graph);
    for node in &graph.nodes {
        if output_dependencies.contains(&node.id)
            && let CompositionGraphNodeKind::Operator(operator) = &node.kind
        {
            let definition = definitions
                .get(&operator.operator)
                .ok_or_else(|| GraphValidationError::MissingOperator(node.id.clone()))?;
            for input in &definition.inputs {
                if !connected_inputs.contains(&(&node.id, input.source_name.as_str())) {
                    return Err(GraphValidationError::UnconnectedInput {
                        node: node.id.clone(),
                        port: input.source_name.clone(),
                    });
                }
            }
        }
    }
    validate_acyclic(graph)
}

/// Nodes that can contribute to the output. Disconnected authoring branches
/// retain their definitions and edges, but need not have all inputs wired yet.
pub fn composition_graph_output_dependencies(
    graph: &SequenceCompositionGraph,
) -> HashSet<crate::sequence::CompositionGraphNodeId> {
    let mut incoming = HashMap::<_, Vec<_>>::new();
    for edge in &graph.edges {
        incoming.entry(&edge.to).or_default().push(&edge.from);
    }
    let mut pending = graph
        .nodes
        .iter()
        .filter_map(|node| {
            matches!(node.kind, CompositionGraphNodeKind::Output).then_some(&node.id)
        })
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
    edges: &mut HashSet<(
        &'a crate::sequence::CompositionGraphNodeId,
        &'a str,
        &'a crate::sequence::CompositionGraphNodeId,
        &'a str,
    )>,
) -> Result<(), GraphValidationError> {
    let from = nodes
        .get(&edge.from)
        .ok_or_else(|| GraphValidationError::MissingEdgeNode(edge.from.clone()))?;
    let to = nodes
        .get(&edge.to)
        .ok_or_else(|| GraphValidationError::MissingEdgeNode(edge.to.clone()))?;
    if !output_ports(from, definitions)?
        .iter()
        .any(|port| port == &edge.from_port.0)
    {
        return Err(GraphValidationError::UnknownOutputPort {
            node: edge.from.clone(),
            port: edge.from_port.0.clone(),
        });
    }
    if !input_ports(to, definitions)?
        .iter()
        .any(|port| port.source_name == edge.to_port.0)
    {
        return Err(GraphValidationError::UnknownInputPort {
            node: edge.to.clone(),
            port: edge.to_port.0.clone(),
        });
    }
    if !edges.insert((
        &edge.from,
        edge.from_port.0.as_str(),
        &edge.to,
        edge.to_port.0.as_str(),
    )) {
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

pub(crate) fn input_ports(
    node: &crate::sequence::CompositionGraphNode,
    definitions: &IndexMap<OperatorDefinitionId, OperatorDefinition>,
) -> Result<Vec<OperatorPortDefinition>, GraphValidationError> {
    match &node.kind {
        CompositionGraphNodeKind::Layer { .. } => Ok(Vec::new()),
        CompositionGraphNodeKind::Operator(operator) => definitions
            .get(&operator.operator)
            .map(|definition| definition.inputs.clone())
            .ok_or_else(|| GraphValidationError::MissingOperator(node.id.clone())),
        CompositionGraphNodeKind::Output => Ok(vec![port("input", "Input")]),
    }
}

pub(crate) fn output_ports(
    node: &crate::sequence::CompositionGraphNode,
    definitions: &IndexMap<OperatorDefinitionId, OperatorDefinition>,
) -> Result<Vec<String>, GraphValidationError> {
    match &node.kind {
        CompositionGraphNodeKind::Layer { .. } => Ok(vec!["output".to_string()]),
        CompositionGraphNodeKind::Operator(operator) => definitions
            .get(&operator.operator)
            .map(|definition| vec![definition.output.source_name.clone()])
            .ok_or_else(|| GraphValidationError::MissingOperator(node.id.clone())),
        CompositionGraphNodeKind::Output => Ok(Vec::new()),
    }
}

fn validate_operator_params(
    id: &CompositionGraphNodeId,
    node: &GraphOperatorNode,
    definition: &OperatorDefinition,
) -> Result<(), GraphValidationError> {
    for (name, value) in &node.params {
        let declaration = definition
            .params()
            .iter()
            .find(|param| param.name == *name)
            .ok_or_else(|| GraphValidationError::UnknownParam {
                node: id.clone(),
                param: name.clone(),
            })?;
        if !effect_param_matches_type(value, &declaration.ty) {
            return Err(GraphValidationError::WrongParamType {
                node: id.clone(),
                param: name.clone(),
            });
        }
    }
    for declaration in definition.params() {
        if declaration.default.is_none() && !node.params.contains_key(&declaration.name) {
            return Err(GraphValidationError::MissingParam {
                node: id.clone(),
                param: declaration.name.clone(),
            });
        }
    }
    Ok(())
}

pub fn effect_param_matches_type(value: &EffectParamValue, ty: &Type) -> bool {
    match (value, ty) {
        (EffectParamValue::Int(_), Type::Int)
        | (EffectParamValue::Float(_), Type::Float)
        | (EffectParamValue::Bool(_), Type::Bool)
        | (EffectParamValue::Color(_), Type::Color)
        | (EffectParamValue::Marks(_), Type::Marks)
        | (EffectParamValue::Curve(_), Type::Curve)
        | (EffectParamValue::Gradient(_), Type::Gradient) => true,
        (EffectParamValue::Enum(value), Type::Enum(options)) => options.contains(value),
        (EffectParamValue::Array(values), Type::Array(item_type)) => values
            .iter()
            .all(|value| effect_param_matches_type(value, item_type)),
        _ => false,
    }
}
