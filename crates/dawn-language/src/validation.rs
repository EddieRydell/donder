use std::collections::HashSet;

use crate::control::{ControlValidationError, resolve_controls};
use crate::dsl::Type;
use crate::effect::{CurveSource, EffectParamValue, GradientSource};
use crate::element::ElementTreeValidationError;
use crate::model::DawnProject;
use crate::operator::{effect_param_matches_type, validate_composition_graph};
use crate::preview::PreviewValidationError;
use crate::sequence::{
    AutomationMapping, AutomationTarget, CompositionGraphNodeKind, MarkCollectionKey, Sequence,
};

pub const MAX_SEQUENCE_FRAME_COUNT: u32 = 250_000;
pub const MAX_SEQUENCE_FRAME_RATE: u32 = 1_000;

#[derive(Clone, Debug, PartialEq)]
pub enum ProjectValidationError {
    MissingSetup,
    MissingElementTree,
    MissingPreviewLayout,
    MissingPatch,
    MissingController,
    MissingFixtureProfile,
    InvalidRelationship(String),
    ElementTree(ElementTreeValidationError),
    FixtureProfile(crate::fixture_profile::FixtureProfileValidationError),
    Controller(crate::controller::ControllerValidationError),
    Patch(crate::patch::PatchValidationError),
    Preview(PreviewValidationError),
    Control(ControlValidationError),
    Sequence(SequenceValidationError),
}

impl std::fmt::Display for ProjectValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSetup => write!(f, "The project setup is missing."),
            Self::MissingElementTree => write!(f, "The setup element tree is missing."),
            Self::MissingPreviewLayout => write!(f, "The setup preview layout is missing."),
            Self::MissingPatch => write!(f, "The setup patch is missing."),
            Self::MissingController => write!(f, "A referenced controller is missing."),
            Self::MissingFixtureProfile => write!(f, "A referenced fixture profile is missing."),
            Self::InvalidRelationship(message) => f.write_str(message),
            Self::Patch(error) => std::fmt::Display::fmt(error, f),
            Self::Sequence(error) => f.write_str(&error.message),
            Self::ElementTree(error) => write!(f, "Invalid element tree: {error:?}"),
            Self::FixtureProfile(error) => std::fmt::Display::fmt(error, f),
            Self::Controller(error) => write!(f, "Invalid controller: {error:?}"),
            Self::Preview(error) => write!(f, "Invalid preview: {error:?}"),
            Self::Control(error) => write!(f, "Invalid control: {error:?}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SequenceValidationError {
    pub message: String,
}

pub fn validate_project(project: &DawnProject) -> Result<(), ProjectValidationError> {
    if !project.setups.contains_key(&project.root.setup) {
        return Err(ProjectValidationError::MissingSetup);
    }
    for tree in project.element_trees.values() {
        validate_element_tree(project, tree)?;
    }
    for profile in project.definitions.fixture_profiles.definitions.values() {
        profile
            .validate()
            .map_err(ProjectValidationError::FixtureProfile)?;
    }
    for controller in project.controllers.values() {
        controller
            .validate()
            .map_err(ProjectValidationError::Controller)?;
    }
    for layout in project.preview_layouts.values() {
        validate_layout(project, layout)?;
    }
    for patch in project.patches.values() {
        validate_patch(project, patch)?;
    }
    for setup in project.setups.values() {
        validate_setup(project, setup)?;
    }
    for sequence in project.sequences.values() {
        validate_sequence(project, sequence).map_err(ProjectValidationError::Sequence)?;
    }
    Ok(())
}

fn validate_element_tree(
    project: &DawnProject,
    tree: &crate::element::ElementTree,
) -> Result<(), ProjectValidationError> {
    tree.validate()
        .map_err(ProjectValidationError::ElementTree)?;

    for node in tree.nodes.values() {
        if let crate::element::ElementNodeKind::Fixture { profile } = &node.kind {
            project
                .definitions
                .fixture_profiles
                .definitions
                .get(profile)
                .ok_or(ProjectValidationError::MissingFixtureProfile)?;
        }
    }

    Ok(())
}

fn validate_layout(
    project: &DawnProject,
    layout: &crate::preview::PreviewLayout,
) -> Result<(), ProjectValidationError> {
    let tree = project
        .element_trees
        .get(&layout.element_tree)
        .ok_or(ProjectValidationError::MissingElementTree)?;
    let cells = tree
        .flattened_cells()
        .map_err(|_| ProjectValidationError::MissingElementTree)?
        .into_iter()
        .collect::<HashSet<_>>();
    let mut props = HashSet::new();
    for prop in &layout.props {
        if !props.insert(prop.id) {
            return Err(ProjectValidationError::Preview(
                PreviewValidationError::DuplicateProp(prop.id),
            ));
        }
        let definition = project
            .definitions
            .props
            .definitions
            .get(&prop.definition)
            .ok_or_else(|| {
                ProjectValidationError::Preview(PreviewValidationError::MissingDefinition(
                    prop.definition.clone(),
                ))
            })?;
        let expected = definition.geometry.point_count();
        if prop.bindings.len() != expected {
            return Err(ProjectValidationError::Preview(
                PreviewValidationError::BindingCount {
                    prop: prop.id,
                    expected,
                    actual: prop.bindings.len(),
                },
            ));
        }
        for address in &prop.bindings {
            if !cells.contains(address) {
                return Err(ProjectValidationError::Preview(
                    PreviewValidationError::MissingElementCell {
                        prop: prop.id,
                        address: *address,
                    },
                ));
            }
        }
    }

    Ok(())
}

fn validate_patch(
    project: &DawnProject,
    patch: &crate::patch::PatchGraph,
) -> Result<(), ProjectValidationError> {
    patch.validate().map_err(ProjectValidationError::Patch)?;
    for (node_id, node) in &patch.nodes {
        match node {
            crate::patch::PatchNode::Source(source) => {
                let tree = project
                    .element_trees
                    .get(&source.selection.tree)
                    .ok_or(ProjectValidationError::MissingElementTree)?;
                source
                    .validate_selection(tree)
                    .map_err(ProjectValidationError::InvalidRelationship)?;
            }
            crate::patch::PatchNode::Sink(sink) => {
                let controller = project
                    .controllers
                    .get(&sink.controller)
                    .ok_or(ProjectValidationError::MissingController)?;
                let port = controller
                    .ports
                    .iter()
                    .find(|port| port.id == sink.port)
                    .ok_or_else(|| {
                        ProjectValidationError::InvalidRelationship(
                            "patch sink controller port is missing".to_string(),
                        )
                    })?;
                let end = sink
                    .start_slot
                    .checked_add(sink.slot_count)
                    .ok_or_else(|| {
                        ProjectValidationError::InvalidRelationship(
                            "patch sink slot range overflowed".to_string(),
                        )
                    })?;
                if end > port.slot_count {
                    return Err(ProjectValidationError::InvalidRelationship(
                        "patch sink exceeds its controller port".to_string(),
                    ));
                }
            }
            crate::patch::PatchNode::Filter(
                crate::patch::FilterDefinition::FixtureProfileEncoding {
                    profile,
                    slot_count,
                    ..
                },
            ) => {
                let definition = project
                    .definitions
                    .fixture_profiles
                    .definitions
                    .get(profile)
                    .ok_or(ProjectValidationError::MissingFixtureProfile)?;
                if *slot_count != definition.slot_count() {
                    return Err(ProjectValidationError::InvalidRelationship(
                        "fixture-profile filter slot width differs from the profile".to_string(),
                    ));
                }
            }
            crate::patch::PatchNode::Filter(
                crate::patch::FilterDefinition::IndexedValueMapping { entries, .. },
            ) => {
                let source = patch
                    .edges
                    .iter()
                    .find(|edge| edge.to == *node_id)
                    .and_then(|edge| patch.nodes.get(&edge.from));
                let Some(crate::patch::PatchNode::Source(source)) = source else {
                    return Err(ProjectValidationError::InvalidRelationship(format!(
                        "Indexed mapping node {} must receive an indexed element source.",
                        node_id.0
                    )));
                };
                if !entries.contains_key(&0) {
                    return Err(ProjectValidationError::InvalidRelationship(format!(
                        "Indexed mapping node {} needs a value for ID 0, used when no control clip is active. Add the inactive value in the patch editor.",
                        node_id.0
                    )));
                }
                let tree = project
                    .element_trees
                    .get(&source.selection.tree)
                    .ok_or(ProjectValidationError::MissingElementTree)?;
                let selected = tree.flatten_selection(&source.selection).map_err(|error| {
                    ProjectValidationError::InvalidRelationship(format!(
                        "Invalid indexed source selection: {error:?}"
                    ))
                })?;
                let mut checked = HashSet::new();
                for cell in selected {
                    if !checked.insert(cell.node) {
                        continue;
                    }
                    if let Some(crate::element::ElementNode {
                        name,
                        kind: crate::element::ElementNodeKind::Indexed { options, .. },
                    }) = tree.nodes.get(&cell.node)
                    {
                        for option in options {
                            if !entries.contains_key(&option.id.0) {
                                return Err(ProjectValidationError::InvalidRelationship(format!(
                                    "Indexed mapping node {} has no value for option {} ({}) of {}. Add its value in the patch editor, or remove the assignment before changing options.",
                                    node_id.0, option.id.0, option.name, name
                                )));
                            }
                        }
                    }
                }
            }
            crate::patch::PatchNode::Filter(crate::patch::FilterDefinition::ColorBreakdown {
                capability: crate::element::ColorCapability::Discrete { mappings, .. },
                ..
            }) => {
                if !mappings
                    .iter()
                    .any(|mapping| mapping.color == dawn_runtime::element::black())
                {
                    return Err(ProjectValidationError::InvalidRelationship(format!(
                        "Discrete color mapping node {} needs a black mapping for inactive pixels. Add it to the light's color capability or the patch filter.",
                        node_id.0
                    )));
                }
            }
            crate::patch::PatchNode::Filter(_) => {}
        }
    }
    Ok(())
}

fn validate_setup(
    project: &DawnProject,
    setup: &crate::setup::Setup,
) -> Result<(), ProjectValidationError> {
    if !project.element_trees.contains_key(&setup.elements) {
        return Err(ProjectValidationError::MissingElementTree);
    }
    let layout = project
        .preview_layouts
        .get(&setup.preview)
        .ok_or(ProjectValidationError::MissingPreviewLayout)?;
    if layout.element_tree != setup.elements {
        return Err(ProjectValidationError::InvalidRelationship(
            "setup layout targets a different element tree".into(),
        ));
    }
    let patch = project
        .patches
        .get(&setup.patch)
        .ok_or(ProjectValidationError::MissingPatch)?;
    for controller in &setup.controllers {
        if !project.controllers.contains_key(controller) {
            return Err(ProjectValidationError::MissingController);
        }
    }
    for node in patch.nodes.values() {
        match node {
            crate::patch::PatchNode::Source(source) if source.selection.tree != setup.elements => {
                return Err(ProjectValidationError::InvalidRelationship(
                    "patch source targets a different element tree".into(),
                ));
            }
            crate::patch::PatchNode::Sink(sink)
                if !setup.controllers.contains(&sink.controller) =>
            {
                return Err(ProjectValidationError::InvalidRelationship(
                    "patch sink controller is not active in the setup".into(),
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn validate_sequence(
    project: &DawnProject,
    sequence: &Sequence,
) -> Result<(), SequenceValidationError> {
    let setup = project
        .setups
        .get(&project.root.setup)
        .ok_or_else(|| sequence_error("active setup is missing"))?;
    let tree = project
        .element_trees
        .get(&setup.elements)
        .ok_or_else(|| sequence_error("active element tree is missing"))?;

    if sequence.frame_rate == 0 {
        return Err(sequence_error("frame rate must be greater than zero"));
    }
    if sequence.frame_rate > MAX_SEQUENCE_FRAME_RATE {
        return Err(sequence_error(format!(
            "sequence frame rate exceeds the limit of {MAX_SEQUENCE_FRAME_RATE} frames per second"
        )));
    }
    if sequence.duration.0.is_zero() {
        return Err(sequence_error("sequence duration must be positive"));
    }
    let frame_count = sequence.frame_count();
    if frame_count > u128::from(MAX_SEQUENCE_FRAME_COUNT) {
        return Err(sequence_error(format!(
            "sequence exceeds the frame budget of {MAX_SEQUENCE_FRAME_COUNT} frames"
        )));
    }
    ensure_unique(sequence.layers.iter().map(|layer| layer.id.0), "layer ids")?;
    ensure_unique(
        sequence.effects.iter().map(|effect| effect.id.0),
        "effect ids",
    )?;
    ensure_unique(
        sequence
            .mark_collections
            .iter()
            .map(|collection| collection.key.name.as_str()),
        "mark collection keys",
    )?;
    ensure_unique(
        sequence.automation_clips.iter().map(|clip| clip.id.0),
        "automation clip ids",
    )?;
    ensure_unique(
        sequence.control_clips.iter().map(|clip| clip.id.0),
        "control clip ids",
    )?;

    let layer_ids = sequence
        .layers
        .iter()
        .map(|layer| &layer.id)
        .collect::<HashSet<_>>();
    let mark_keys = sequence
        .mark_collections
        .iter()
        .map(|collection| &collection.key)
        .collect::<HashSet<_>>();
    for collection in &sequence.mark_collections {
        if collection
            .marks
            .iter()
            .any(|mark| mark.0 > sequence.duration.0)
        {
            return Err(sequence_error("a mark lies outside the sequence duration"));
        }
    }

    for effect in &sequence.effects {
        if !layer_ids.contains(&effect.layer_id) {
            return Err(sequence_error(format!(
                "effect {} references a missing layer",
                effect.id.0
            )));
        }
        validate_timed_region(
            effect.start.0,
            effect.duration.0,
            sequence.duration.0,
            "effect",
        )?;
        validate_color_effect_target(project, tree, effect)?;
        let definition = project
            .definitions
            .effects
            .resolve(&effect.definition)
            .ok_or_else(|| {
                sequence_error(format!("effect {} definition is missing", effect.id.0))
            })?;
        for declaration in &definition.params {
            match effect.param_overrides.get(&declaration.name) {
                Some(value) if effect_param_matches_type(value, &declaration.ty) => {
                    validate_param_references(project, value, &mark_keys)?;
                }
                Some(_) => {
                    return Err(sequence_error(format!(
                        "effect {} parameter `{}` has the wrong type",
                        effect.id.0,
                        declaration.name.as_str()
                    )));
                }
                None if declaration.default.is_none() => {
                    return Err(sequence_error(format!(
                        "effect {} is missing required parameter `{}`",
                        effect.id.0,
                        declaration.name.as_str()
                    )));
                }
                None => {}
            }
        }
        if effect
            .param_overrides
            .keys()
            .any(|name| !definition.params.iter().any(|param| &param.name == name))
        {
            return Err(sequence_error(format!(
                "effect {} contains an undeclared parameter",
                effect.id.0
            )));
        }
    }

    validate_composition_graph(&sequence.composition_graph, &project.definitions.operators)
        .map_err(|error| sequence_error(error.message))?;
    let mut graph_layers = HashSet::new();
    for node in &sequence.composition_graph.nodes {
        if let CompositionGraphNodeKind::Layer { layer_id } = &node.kind {
            if !layer_ids.contains(layer_id) {
                return Err(sequence_error(format!(
                    "composition graph references missing layer {}",
                    layer_id.0
                )));
            }
            if !graph_layers.insert(layer_id) {
                return Err(sequence_error(format!(
                    "composition graph contains layer {} more than once",
                    layer_id.0
                )));
            }
        }
    }

    let mut automation_targets = HashSet::new();
    for clip in &sequence.automation_clips {
        validate_timed_region(
            clip.start.0,
            clip.duration.0,
            sequence.duration.0,
            "automation clip",
        )?;
        clip.curve
            .validate()
            .map_err(|error| sequence_error(format!("automation curve is invalid: {error:?}")))?;
        for target in clip
            .bindings
            .iter()
            .map(|binding| &binding.target)
            .chain(clip.detached_bindings.iter().map(|binding| &binding.target))
        {
            if !automation_targets.insert(target) {
                return Err(sequence_error(
                    "automation targets must be unique across active and detached bindings",
                ));
            }
        }
        for binding in &clip.bindings {
            let ty = automation_target_type(project, sequence, &binding.target)?;
            if !automation_mapping_matches_type(&binding.mapping, ty) {
                return Err(sequence_error(
                    "automation mapping does not match its target parameter",
                ));
            }
        }
    }

    for clip in &sequence.control_clips {
        validate_timed_region(
            clip.start.0,
            clip.duration.0,
            sequence.duration.0,
            "control clip",
        )?;
    }
    resolve_controls(
        tree,
        &project.definitions.fixture_profiles,
        &sequence.control_clips,
    )
    .map_err(|error| sequence_error(error.to_string()))?;
    Ok(())
}

fn ensure_unique<T>(
    values: impl Iterator<Item = T>,
    label: &str,
) -> Result<(), SequenceValidationError>
where
    T: Eq + std::hash::Hash,
{
    let mut seen = HashSet::new();
    for value in values {
        if !seen.insert(value) {
            return Err(sequence_error(format!("sequence {label} must be unique")));
        }
    }
    Ok(())
}

fn validate_timed_region(
    start: std::time::Duration,
    duration: std::time::Duration,
    sequence_duration: std::time::Duration,
    label: &str,
) -> Result<(), SequenceValidationError> {
    if duration.is_zero() {
        return Err(sequence_error(format!("{label} duration must be positive")));
    }
    let end = start
        .checked_add(duration)
        .ok_or_else(|| sequence_error(format!("{label} timing overflows")))?;
    if end > sequence_duration {
        return Err(sequence_error(format!(
            "{label} extends beyond the sequence duration"
        )));
    }
    Ok(())
}

fn validate_color_effect_target(
    project: &DawnProject,
    tree: &crate::element::ElementTree,
    effect: &crate::effect::EffectInst,
) -> Result<(), SequenceValidationError> {
    if effect.target.tree != tree.id {
        return Err(sequence_error("effect targets a different element tree"));
    }
    let addresses = tree
        .flatten_selection(&effect.target)
        .map_err(|error| sequence_error(format!("invalid effect selection: {error:?}")))?;
    for address in addresses {
        let node = tree
            .nodes
            .get(&address.node)
            .ok_or_else(|| sequence_error("effect selection references a missing element"))?;
        match &node.kind {
            crate::element::ElementNodeKind::Color { .. } => {}
            crate::element::ElementNodeKind::Fixture { profile } => {
                let accepts_color = project
                    .definitions
                    .fixture_profiles
                    .definitions
                    .get(profile)
                    .is_some_and(|profile| {
                        profile.functions.values().any(|function| {
                            matches!(
                                function.kind,
                                crate::fixture_profile::FixtureFunctionKind::ColorMixing { .. }
                            )
                        })
                    });
                if !accepts_color {
                    return Err(sequence_error(
                        "effect targets a fixture without configured color support",
                    ));
                }
            }
            _ => return Err(sequence_error("color effect targets a non-color element")),
        }
    }
    Ok(())
}

fn validate_param_references(
    project: &DawnProject,
    value: &EffectParamValue,
    mark_keys: &HashSet<&MarkCollectionKey>,
) -> Result<(), SequenceValidationError> {
    match value {
        EffectParamValue::Marks(key) if !mark_keys.contains(key) => Err(sequence_error(
            "effect parameter references a missing mark collection",
        )),
        EffectParamValue::Curve(CurveSource::Inline(curve)) => curve
            .validate()
            .map_err(|error| sequence_error(format!("inline curve is invalid: {error:?}"))),
        EffectParamValue::Curve(CurveSource::Reference(id))
            if !project.definitions.curves.definitions.contains_key(id) =>
        {
            Err(sequence_error(
                "effect parameter references a missing curve",
            ))
        }
        EffectParamValue::Gradient(GradientSource::Reference(id))
            if !project.definitions.gradients.definitions.contains_key(id) =>
        {
            Err(sequence_error(
                "effect parameter references a missing gradient",
            ))
        }
        EffectParamValue::Array(values) => {
            for value in values {
                validate_param_references(project, value, mark_keys)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

pub fn automation_target_type<'a>(
    project: &'a DawnProject,
    sequence: &'a Sequence,
    target: &AutomationTarget,
) -> Result<&'a Type, SequenceValidationError> {
    match target {
        AutomationTarget::EffectParam { effect_id, param } => {
            let effect = sequence
                .effects
                .iter()
                .find(|effect| &effect.id == effect_id)
                .ok_or_else(|| sequence_error("automation effect is missing"))?;
            project
                .definitions
                .effects
                .resolve(&effect.definition)
                .and_then(|definition| {
                    definition
                        .params
                        .iter()
                        .find(|declaration| &declaration.name == param)
                })
                .ok_or_else(|| sequence_error("automation parameter is missing"))
                .and_then(|declaration| {
                    if declaration.fixed {
                        Err(sequence_error(format!("fixed parameter `{}` requires preparation and cannot receive automation", declaration.name.as_str())))
                    } else {
                        Ok(&declaration.ty)
                    }
                })
        }
        AutomationTarget::CompositionNodeParam { node_id, param } => {
            let operator = sequence
                .composition_graph
                .nodes
                .iter()
                .find(|node| &node.id == node_id)
                .and_then(|node| match &node.kind {
                    CompositionGraphNodeKind::Operator(operator) => Some(operator),
                    _ => None,
                })
                .ok_or_else(|| sequence_error("automation graph node is missing"))?;
            project
                .definitions
                .operators
                .resolve(&operator.operator)
                .and_then(|definition| {
                    definition
                        .params
                        .iter()
                        .find(|declaration| &declaration.name == param)
                })
                .ok_or_else(|| sequence_error("automation parameter is missing"))
                .and_then(|declaration| {
                    if declaration.fixed {
                        Err(sequence_error(format!("fixed parameter `{}` requires preparation and cannot receive automation", declaration.name.as_str())))
                    } else {
                        Ok(&declaration.ty)
                    }
                })
        }
    }
}

fn automation_mapping_matches_type(mapping: &AutomationMapping, ty: &Type) -> bool {
    match (mapping, ty) {
        (AutomationMapping::Float { min, max }, Type::Float)
        | (AutomationMapping::Curve { min, max }, Type::Curve) => {
            min.is_finite() && max.is_finite() && min <= max
        }
        (AutomationMapping::Int { min, max }, Type::Int) => min <= max,
        (AutomationMapping::Bool, Type::Bool) => true,
        (AutomationMapping::Enum { values }, Type::Enum(options)) => {
            !values.is_empty() && values.iter().all(|value| options.contains(value))
        }
        _ => false,
    }
}

fn sequence_error(message: impl Into<String>) -> SequenceValidationError {
    SequenceValidationError {
        message: message.into(),
    }
}
