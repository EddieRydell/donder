use crate::effect::EffectParamValue;
use crate::identity::SourceIdentity;
use crate::sequence::{CompositionGraphNodeKind, EffectGraphEdge, SequenceCompositionGraph};
use donder_language::compiler::{CompiledOperator, OperatorInputDecl, ParamDecl};
use donder_runtime_types::{Identifier, Type};
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct OperatorDefinitionId(pub SourceIdentity);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperatorRef {
    Custom(OperatorDefinitionId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphOperatorNode {
    /// Unique among the sequence's layers and graph nodes; edges use it.
    pub name: donder_runtime_types::Identifier,
    pub operator: OperatorRef,
    pub params: IndexMap<Identifier, EffectParamValue>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperatorPortCardinality {
    One,
    Many,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperatorPortDefinition {
    pub source_name: String,
    pub display_name: String,
    pub cardinality: OperatorPortCardinality,
}

#[derive(Clone, Debug, PartialEq)]
pub enum OperatorImplementation {
    Dsl(Box<CompiledOperator>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct OperatorDefinition {
    pub(crate) id: OperatorRef,
    pub source_name: String,
    pub declaration_name: String,
    pub display_name: String,
    pub(crate) inputs: Vec<OperatorPortDefinition>,
    pub(crate) output: OperatorPortDefinition,
    pub(crate) params: Vec<ParamDecl>,
    pub(crate) implementation: OperatorImplementation,
}

impl OperatorDefinition {
    pub fn id(&self) -> &OperatorRef {
        &self.id
    }
    pub fn inputs(&self) -> &[OperatorPortDefinition] {
        &self.inputs
    }
    pub fn output(&self) -> &OperatorPortDefinition {
        &self.output
    }
    pub fn params(&self) -> &[ParamDecl] {
        &self.params
    }
    pub fn implementation(&self) -> &OperatorImplementation {
        &self.implementation
    }
    pub fn description(&self) -> Option<&str> {
        match &self.implementation {
            OperatorImplementation::Dsl(compiled) => compiled.description(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OperatorDefinitionStore {
    pub definitions: IndexMap<OperatorDefinitionId, OperatorDefinition>,
}

impl OperatorDefinitionStore {
    pub fn get(&self, id: &OperatorDefinitionId) -> Option<&OperatorDefinition> {
        self.definitions.get(id)
    }

    pub fn insert(
        &mut self,
        id: OperatorDefinitionId,
        definition: OperatorDefinition,
    ) -> Option<OperatorDefinition> {
        self.definitions.insert(id, definition)
    }

    pub fn resolve(&self, reference: &OperatorRef) -> Option<&OperatorDefinition> {
        match reference {
            OperatorRef::Custom(id) => self.get(id),
        }
    }
}

fn port(
    name: &str,
    display_name: &str,
    cardinality: OperatorPortCardinality,
) -> OperatorPortDefinition {
    OperatorPortDefinition {
        source_name: name.to_string(),
        display_name: display_name.to_string(),
        cardinality,
    }
}

pub fn custom_operator_definition(
    id: OperatorDefinitionId,
    compiled: CompiledOperator,
) -> OperatorDefinition {
    let declaration_name = compiled.name().as_str().to_string();
    OperatorDefinition {
        source_name: id.0.object().to_string(),
        id: OperatorRef::Custom(id),
        display_name: declaration_name.clone(),
        declaration_name,
        inputs: compiled
            .inputs()
            .iter()
            .map(|OperatorInputDecl { name }| {
                port(name.as_str(), name.as_str(), OperatorPortCardinality::One)
            })
            .collect(),
        output: port("output", "Output", OperatorPortCardinality::Many),
        params: compiled.params().to_vec(),
        implementation: OperatorImplementation::Dsl(Box::new(compiled)),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphValidationError {
    pub message: String,
}

pub fn validate_composition_graph(
    graph: &SequenceCompositionGraph,
    definitions: &OperatorDefinitionStore,
) -> Result<(), GraphValidationError> {
    let mut node_ids = HashSet::new();
    let mut output_count = 0usize;
    for node in &graph.nodes {
        if !node_ids.insert(&node.id) {
            return graph_error(format!("duplicate composition graph node {}", node.id.0));
        }
        match &node.kind {
            CompositionGraphNodeKind::Operator(operator) => {
                let definition = definitions.resolve(&operator.operator).ok_or_else(|| {
                    GraphValidationError {
                        message: format!(
                            "missing operator definition `{}`",
                            operator_reference_name(&operator.operator)
                        ),
                    }
                })?;
                validate_operator_params(operator, definition)?;
            }
            CompositionGraphNodeKind::Output => output_count += 1,
            CompositionGraphNodeKind::Layer { .. } => {}
        }
    }
    if output_count != 1 {
        return graph_error("composition graph must have exactly one output node".to_string());
    }

    let nodes = graph
        .nodes
        .iter()
        .map(|node| (&node.id, node))
        .collect::<HashMap<_, _>>();
    let mut edges = HashSet::new();
    let mut occupied_inputs = HashSet::new();
    for edge in &graph.edges {
        validate_edge(edge, &nodes, definitions, &mut edges, &mut occupied_inputs)?;
    }
    let output_dependencies = composition_graph_output_dependencies(graph);
    for node in &graph.nodes {
        if output_dependencies.contains(&node.id)
            && let CompositionGraphNodeKind::Operator(operator) = &node.kind
        {
            let definition =
                definitions
                    .resolve(&operator.operator)
                    .ok_or_else(|| GraphValidationError {
                        message: format!(
                            "missing operator definition `{}`",
                            operator_reference_name(&operator.operator)
                        ),
                    })?;
            for input in &definition.inputs {
                if !occupied_inputs.contains(&(&node.id, input.source_name.as_str())) {
                    return graph_error(format!(
                        "composition graph input port `{}` is not connected",
                        input.source_name
                    ));
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
    definitions: &OperatorDefinitionStore,
    edges: &mut HashSet<(
        &'a crate::sequence::CompositionGraphNodeId,
        &'a str,
        &'a crate::sequence::CompositionGraphNodeId,
        &'a str,
    )>,
    occupied_inputs: &mut HashSet<(&'a crate::sequence::CompositionGraphNodeId, &'a str)>,
) -> Result<(), GraphValidationError> {
    let from = nodes.get(&edge.from).ok_or_else(|| GraphValidationError {
        message: format!(
            "edge references missing composition graph node {}",
            edge.from.0
        ),
    })?;
    let to = nodes.get(&edge.to).ok_or_else(|| GraphValidationError {
        message: format!(
            "edge references missing composition graph node {}",
            edge.to.0
        ),
    })?;
    if !output_ports(&from.kind, definitions)?
        .iter()
        .any(|port| port == &edge.from_port.0)
    {
        return graph_error(format!(
            "unknown composition graph output port `{}`",
            edge.from_port.0
        ));
    }
    let inputs = input_ports(&to.kind, definitions)?;
    let input = inputs
        .iter()
        .find(|port| port.source_name == edge.to_port.0)
        .ok_or_else(|| GraphValidationError {
            message: format!("unknown composition graph input port `{}`", edge.to_port.0),
        })?;
    if !edges.insert((
        &edge.from,
        edge.from_port.0.as_str(),
        &edge.to,
        edge.to_port.0.as_str(),
    )) {
        return graph_error("duplicate composition graph edge".to_string());
    }
    if input.cardinality == OperatorPortCardinality::One
        && !occupied_inputs.insert((&edge.to, edge.to_port.0.as_str()))
    {
        return graph_error(format!(
            "composition graph input port `{}` accepts one connection",
            edge.to_port.0
        ));
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
        graph_error("composition graph contains a cycle".to_string())
    }
}

pub(crate) fn input_ports(
    kind: &CompositionGraphNodeKind,
    definitions: &OperatorDefinitionStore,
) -> Result<Vec<OperatorPortDefinition>, GraphValidationError> {
    match kind {
        CompositionGraphNodeKind::Layer { .. } => Ok(Vec::new()),
        CompositionGraphNodeKind::Operator(operator) => definitions
            .resolve(&operator.operator)
            .map(|definition| definition.inputs.clone())
            .ok_or_else(|| GraphValidationError {
                message: format!(
                    "missing operator definition `{}`",
                    operator_reference_name(&operator.operator)
                ),
            }),
        CompositionGraphNodeKind::Output => {
            Ok(vec![port("input", "Input", OperatorPortCardinality::Many)])
        }
    }
}

pub(crate) fn output_ports(
    kind: &CompositionGraphNodeKind,
    definitions: &OperatorDefinitionStore,
) -> Result<Vec<String>, GraphValidationError> {
    match kind {
        CompositionGraphNodeKind::Layer { .. } => Ok(vec!["output".to_string()]),
        CompositionGraphNodeKind::Operator(operator) => definitions
            .resolve(&operator.operator)
            .map(|definition| vec![definition.output.source_name.clone()])
            .ok_or_else(|| GraphValidationError {
                message: format!(
                    "missing operator definition `{}`",
                    operator_reference_name(&operator.operator)
                ),
            }),
        CompositionGraphNodeKind::Output => Ok(Vec::new()),
    }
}

fn validate_operator_params(
    node: &GraphOperatorNode,
    definition: &OperatorDefinition,
) -> Result<(), GraphValidationError> {
    for (name, value) in &node.params {
        let declaration = definition
            .params
            .iter()
            .find(|param| param.name == *name)
            .ok_or_else(|| GraphValidationError {
                message: format!(
                    "unknown parameter `{}` for operator {}",
                    name.as_str(),
                    definition.source_name
                ),
            })?;
        if !effect_param_matches_type(value, &declaration.ty) {
            return graph_error(format!(
                "parameter `{}` has the wrong type for operator {}",
                name.as_str(),
                definition.source_name
            ));
        }
    }
    for declaration in &definition.params {
        if declaration.default.is_none() && !node.params.contains_key(&declaration.name) {
            return graph_error(format!(
                "operator {} is missing required parameter `{}`",
                definition.source_name,
                declaration.name.as_str()
            ));
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

pub(crate) fn operator_reference_name(reference: &OperatorRef) -> &str {
    match reference {
        OperatorRef::Custom(id) => id.0.object(),
    }
}

fn graph_error<T>(message: String) -> Result<T, GraphValidationError> {
    Err(GraphValidationError { message })
}
