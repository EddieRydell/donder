use std::collections::HashSet;

use crate::dsl::Type;
use crate::effect::{CurveSource, EffectParamValue, GradientSource};
use crate::fixture::{FixtureDefinitionError, FixtureDefinitionId};
use crate::layout::LayoutError;
use crate::model::DawnProject;
use crate::operator::{effect_param_matches_type, validate_composition_graph};
use crate::sequence::{
    AutomationMapping, AutomationTarget, CompositionGraphNodeKind, MarkCollectionKey, Sequence,
};
use indexmap::IndexMap;

pub const MAX_SEQUENCE_FRAME_COUNT: u32 = 250_000;
pub const MAX_SEQUENCE_FRAME_RATE: u32 = 1_000;

#[derive(Clone, Debug, PartialEq)]
pub enum ProjectValidationError {
    MissingSetup,
    MissingLayout,
    MissingPatch,
    MissingController,
    InvalidRelationship(String),
    Fixture(FixtureDefinitionError),
    Layout(LayoutError),
    Controller(crate::controller::ControllerValidationError),
    Sequence(SequenceValidationError),
}

impl std::fmt::Display for ProjectValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSetup => write!(f, "The project setup is missing."),
            Self::MissingLayout => write!(f, "The referenced layout is missing."),
            Self::MissingPatch => write!(f, "The setup patch is missing."),
            Self::MissingController => write!(f, "A referenced controller is missing."),
            Self::InvalidRelationship(message) => f.write_str(message),
            Self::Sequence(error) => f.write_str(&error.message),
            Self::Fixture(error) => write!(f, "Invalid fixture definition: {error:?}"),
            Self::Layout(error) => write!(f, "Invalid layout: {error:?}"),
            Self::Controller(error) => write!(f, "Invalid controller: {error:?}"),
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
    let counts = project
        .definitions
        .fixtures
        .pixel_counts()
        .map_err(ProjectValidationError::Fixture)?;
    for controller in project.controllers.values() {
        controller
            .validate()
            .map_err(ProjectValidationError::Controller)?;
    }
    for layout in project.layouts.values() {
        layout
            .validate(&project.definitions.fixtures.definitions)
            .map_err(ProjectValidationError::Layout)?;
    }
    for patch in project.patches.values() {
        validate_patch(project, patch, &counts)?;
    }
    for setup in project.setups.values() {
        validate_setup(project, setup)?;
    }
    for sequence in project.sequences.values() {
        validate_sequence(project, sequence).map_err(ProjectValidationError::Sequence)?;
    }
    Ok(())
}

fn validate_patch(
    project: &DawnProject,
    patch: &crate::patch::Patch,
    counts: &IndexMap<FixtureDefinitionId, u32>,
) -> Result<(), ProjectValidationError> {
    let mut ids = HashSet::new();
    let mut occupied = std::collections::HashMap::<_, Vec<std::ops::Range<u32>>>::new();
    for route in &patch.routes {
        if !ids.insert(route.id) {
            return Err(ProjectValidationError::InvalidRelationship(
                "Duplicate output assignment ID.".into(),
            ));
        }
        if !route.encoding.is_valid()
            || !route.gamma.is_finite()
            || route.gamma <= 0.0
            || !route.brightness.is_finite()
            || !(0.0..=1.0).contains(&route.brightness)
        {
            return Err(ProjectValidationError::InvalidRelationship(
                "Invalid LED encoding, gamma, or brightness.".into(),
            ));
        }
        let layout = project
            .layouts
            .get(&route.target.layout)
            .ok_or(ProjectValidationError::MissingLayout)?;
        let target_count = layout
            .target_pixel_count(&route.target, counts)
            .map_err(ProjectValidationError::Layout)?;
        let count = if let Some(span) = route.pixels {
            if span.count == 0
                || span
                    .start
                    .checked_add(span.count)
                    .is_none_or(|end| end > target_count)
            {
                return Err(ProjectValidationError::InvalidRelationship(
                    "Output pixel span exceeds its target.".into(),
                ));
            }
            span.count
        } else {
            target_count
        };
        let controller = project
            .controllers
            .get(&route.controller)
            .ok_or(ProjectValidationError::MissingController)?;
        let port = controller
            .ports
            .iter()
            .find(|port| port.id == route.port)
            .ok_or_else(|| {
                ProjectValidationError::InvalidRelationship(
                    "Output controller port is missing.".into(),
                )
            })?;
        let width = count
            .checked_mul(route.encoding.channel_order().len() as u32)
            .ok_or_else(|| {
                ProjectValidationError::InvalidRelationship(
                    "Output channel count overflowed.".into(),
                )
            })?;
        let start = u32::from(route.start_slot);
        let end = start
            .checked_add(width)
            .filter(|&end| end <= u32::from(port.slot_count))
            .ok_or_else(|| {
                ProjectValidationError::InvalidRelationship(
                    "Output exceeds its controller port.".into(),
                )
            })?;
        let ranges = occupied.entry((&route.controller, route.port)).or_default();
        if width != 0
            && ranges
                .iter()
                .any(|range| start < range.end && range.start < end)
        {
            return Err(ProjectValidationError::InvalidRelationship(
                "Output assignments overlap on a controller port.".into(),
            ));
        }
        if width != 0 {
            ranges.push(start..end);
        }
    }
    Ok(())
}

fn validate_setup(
    project: &DawnProject,
    setup: &crate::setup::Setup,
) -> Result<(), ProjectValidationError> {
    if !project.layouts.contains_key(&setup.layout) {
        return Err(ProjectValidationError::MissingLayout);
    }
    let patch = project
        .patches
        .get(&setup.patch)
        .ok_or(ProjectValidationError::MissingPatch)?;
    let mut controllers = HashSet::new();
    for controller in &setup.controllers {
        if !project.controllers.contains_key(controller) {
            return Err(ProjectValidationError::MissingController);
        }
        if !controllers.insert(controller) {
            return Err(ProjectValidationError::InvalidRelationship(
                "Controller appears more than once in the setup.".into(),
            ));
        }
    }
    for route in &patch.routes {
        if route.target.layout != setup.layout {
            return Err(ProjectValidationError::InvalidRelationship(
                "Output targets a different layout.".into(),
            ));
        }
        if !controllers.contains(&route.controller) {
            return Err(ProjectValidationError::InvalidRelationship(
                "Output controller is not active in the setup.".into(),
            ));
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
    let layout = project
        .layouts
        .get(&setup.layout)
        .ok_or_else(|| sequence_error("active layout is missing"))?;

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
        if effect.target.layout != layout.id || layout.fixture(effect.target.fixture).is_none() {
            return Err(sequence_error(
                "Effect target is not present in the active layout.",
            ));
        }
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
