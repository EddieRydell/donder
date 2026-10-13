use std::collections::{HashMap, HashSet};

use crate::effect::{CurveSource, EffectParamValue, GradientSource};
use crate::fixture::{FixtureDefinitionError, FixtureDefinitionId};
use crate::layout::LayoutError;
use crate::operator::{effect_param_matches_type, validate_composition_graph};
use crate::project::DonderProject;
use crate::rig::{MAX_STACKED_VOICES, MappingAction, Rig, TriggerLength, VoicePolicy, When};
use crate::sequence::{
    AutomationTarget, CompositionGraphNodeKind, MarkCollectionKey, Sequence,
    SequenceCompositionGraph, SequenceLayer, SequenceLayerId,
};
use donder_language::compiler::ParamDecl;
use donder_language::{
    DonderDuration, DonderTime, sample_duration_from_donder_duration, sample_time_from_donder_time,
};
use donder_runtime_types::SampleDuration;
use indexmap::IndexMap;

pub(crate) const MAX_SEQUENCE_FRAME_COUNT: u32 = 250_000;
pub(crate) const MAX_SEQUENCE_FRAME_RATE: u32 = 1_000;

#[derive(Clone, Debug, PartialEq)]
pub enum ProjectValidationError {
    MissingSetup,
    MissingLayout,
    MissingPatch,
    MissingController,
    MissingInputDevice,
    InvalidRelationship(String),
    Fixture(FixtureDefinitionError),
    Layout(LayoutError),
    Controller(crate::controller::ControllerValidationError),
    InputDevice(crate::input_device::InputDeviceValidationError),
    Sequence(SequenceValidationError),
    Rig(SequenceValidationError),
}

impl std::fmt::Display for ProjectValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSetup => write!(f, "The project setup is missing."),
            Self::MissingLayout => write!(f, "The referenced layout is missing."),
            Self::MissingPatch => write!(f, "The setup patch is missing."),
            Self::MissingController => write!(f, "A referenced controller is missing."),
            Self::MissingInputDevice => write!(f, "A referenced input device is missing."),
            Self::InvalidRelationship(message) => f.write_str(message),
            Self::Sequence(error) | Self::Rig(error) => f.write_str(&error.message),
            Self::Fixture(error) => write!(f, "Invalid fixture definition: {error:?}"),
            Self::Layout(error) => write!(f, "Invalid layout: {error:?}"),
            Self::Controller(error) => write!(f, "Invalid controller: {error:?}"),
            Self::InputDevice(error) => write!(f, "Invalid input device: {error}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SequenceValidationError {
    pub message: String,
}

/// Validate `project` as an edit of `previous`, a project that already passed
/// validation. When only sequences changed, only sequences are revalidated, and
/// a clip that is still the allocation `previous` held skips the checks that
/// depend on nothing else that could have changed.
pub(crate) fn validate_project_edit(
    project: &DonderProject,
    previous: &DonderProject,
) -> Result<(), ProjectValidationError> {
    if !project.only_sequences_differ_from(previous) {
        return validate_project(project);
    }
    crate::ownership::validate_ownership(project)?;
    for sequence in project.sequences() {
        validate_sequence_since(project, sequence, previous.sequence(&sequence.id))
            .map_err(ProjectValidationError::Sequence)?;
    }
    Ok(())
}

pub fn validate_project(project: &DonderProject) -> Result<(), ProjectValidationError> {
    crate::ownership::validate_ownership(project)?;
    validate_definition_schemas(project)?;
    for (id, definition) in &project.definitions.curves.definitions {
        definition.curve.validate().map_err(|error| {
            ProjectValidationError::InvalidRelationship(format!(
                "Curve `{}` is invalid: {error:?}.",
                id.0.object()
            ))
        })?;
    }
    for (id, definition) in &project.definitions.gradients.definitions {
        definition.gradient.validate().map_err(|error| {
            ProjectValidationError::InvalidRelationship(format!(
                "Gradient `{}` is invalid: {error:?}.",
                id.0.object()
            ))
        })?;
    }
    if project.setup(project.root.setup.id()).is_none() {
        return Err(ProjectValidationError::MissingSetup);
    }
    let counts = project
        .definitions
        .fixtures
        .pixel_counts()
        .map_err(ProjectValidationError::Fixture)?;
    for controller in project.controllers() {
        controller
            .validate()
            .map_err(ProjectValidationError::Controller)?;
    }
    for device in project.input_devices() {
        device
            .validate()
            .map_err(ProjectValidationError::InputDevice)?;
    }
    for layout in project.layouts() {
        layout
            .validate(&project.definitions.fixtures.definitions)
            .map_err(ProjectValidationError::Layout)?;
    }
    for patch in project.patches() {
        validate_patch(project, patch, &counts)?;
    }
    for setup in project.setups() {
        validate_setup(project, setup)?;
    }
    for sequence in project.sequences() {
        validate_sequence(project, sequence).map_err(ProjectValidationError::Sequence)?;
    }
    for rig in project.rigs() {
        validate_rig(project, rig).map_err(ProjectValidationError::Rig)?;
    }
    Ok(())
}

fn validate_definition_schemas(project: &DonderProject) -> Result<(), ProjectValidationError> {
    for (id, definition) in &project.definitions.effects.definitions {
        let crate::effect::EffectImplementation::Dsl(compiled) = &definition.implementation;
        if definition.id != crate::effect::EffectRef::Custom(id.clone())
            || definition.params != compiled.params()
        {
            return Err(ProjectValidationError::InvalidRelationship(format!(
                "Effect `{}` does not match its compiled declaration.",
                id.0.object()
            )));
        }
    }
    for (id, definition) in &project.definitions.operators.definitions {
        let crate::operator::OperatorImplementation::Dsl(compiled) = &definition.implementation;
        if definition.id != crate::operator::OperatorRef::Custom(id.clone())
            || definition.params != compiled.params()
            || definition.inputs.len() != compiled.inputs().len()
            || definition
                .inputs
                .iter()
                .zip(compiled.inputs())
                .any(|(port, input)| {
                    port.source_name != input.name.as_str()
                        || port.cardinality != crate::operator::OperatorPortCardinality::One
                })
            || definition.output.source_name != "output"
            || definition.output.cardinality != crate::operator::OperatorPortCardinality::Many
        {
            return Err(ProjectValidationError::InvalidRelationship(format!(
                "Operator `{}` does not match its compiled declaration.",
                id.0.object()
            )));
        }
    }
    Ok(())
}

fn validate_patch(
    project: &DonderProject,
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
            .layout(&route.target.layout)
            .ok_or(ProjectValidationError::MissingLayout)?;
        if !matches!(
            layout
                .fixture(route.target.fixture)
                .map(|fixture| &fixture.kind),
            Some(crate::layout::LayoutFixtureKind::Fixture { .. })
        ) {
            return Err(ProjectValidationError::InvalidRelationship(
                "Output routes must target a fixture.".into(),
            ));
        }
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
            .controller(&route.controller)
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
    project: &DonderProject,
    setup: &crate::setup::Setup,
) -> Result<(), ProjectValidationError> {
    if setup.controllers.len() as u128 > u128::from(u32::MAX) + 1 {
        return Err(ProjectValidationError::InvalidRelationship(
            "Setup controller indices exceed the portable output address range.".into(),
        ));
    }
    if project.layout(setup.layout.id()).is_none() {
        return Err(ProjectValidationError::MissingLayout);
    }
    let patch = project
        .patch(setup.patch.id())
        .ok_or(ProjectValidationError::MissingPatch)?;
    let mut controllers = HashSet::new();
    for controller in &setup.controllers {
        if project.controller(controller.id()).is_none() {
            return Err(ProjectValidationError::MissingController);
        }
        if !controllers.insert(controller.id()) {
            return Err(ProjectValidationError::InvalidRelationship(
                "Controller appears more than once in the setup.".into(),
            ));
        }
    }
    let mut devices = HashSet::new();
    for device in &setup.input_devices {
        if project.input_device(device.id()).is_none() {
            return Err(ProjectValidationError::MissingInputDevice);
        }
        if !devices.insert(device.id()) {
            return Err(ProjectValidationError::InvalidRelationship(
                "Input device appears more than once in the setup.".into(),
            ));
        }
    }
    for route in &patch.routes {
        if &route.target.layout != setup.layout.id() {
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
    project: &DonderProject,
    sequence: &Sequence,
) -> Result<(), SequenceValidationError> {
    validate_sequence_since(project, sequence, None)
}

/// [`validate_sequence`], where `previous` is this sequence in a validated
/// project whose layouts and definitions are the same allocations as
/// `project`'s. A clip still shared with `previous` keeps its target and
/// parameter checks when the layers and mark collections are unchanged too.
fn validate_sequence_since(
    project: &DonderProject,
    sequence: &Sequence,
    previous: Option<&Sequence>,
) -> Result<(), SequenceValidationError> {
    let unchanged = previous
        .filter(|previous| {
            previous.layers == sequence.layers
                && previous.mark_collections == sequence.mark_collections
        })
        .map(|previous| {
            previous
                .effects
                .iter()
                .map(|effect| (&effect.id, effect))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    let setup = project
        .setup(project.root.setup.id())
        .ok_or_else(|| sequence_error("active setup is missing"))?;
    let active_layout = setup.layout.id();
    let active = project
        .root
        .sequences
        .iter()
        .any(|source| source.id() == &sequence.id);

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
    let sampled_sequence_duration = sample_duration_from_donder_duration(&sequence.duration)
        .map_err(|_| sequence_error("sequence duration exceeds the runtime clock range"))?;
    if sampled_sequence_duration.as_ticks() == 0 {
        return Err(sequence_error(
            "sequence duration rounds to zero on the runtime clock",
        ));
    }
    let frame_count = sequence.frame_count();
    if frame_count > u128::from(MAX_SEQUENCE_FRAME_COUNT) {
        return Err(sequence_error(format!(
            "sequence exceeds the frame budget of {MAX_SEQUENCE_FRAME_COUNT} frames"
        )));
    }
    ensure_unique(
        sequence.effects.iter().map(|effect| effect.id.0),
        "sequence effect ids",
    )?;
    ensure_unique(
        sequence
            .mark_collections
            .iter()
            .map(|collection| collection.key.name.as_str()),
        "sequence mark collection keys",
    )?;
    ensure_unique(
        sequence.automation_clips.iter().map(|clip| clip.id.0),
        "sequence automation clip ids",
    )?;
    validate_sequence_names(sequence, &unchanged)?;

    let mark_keys = sequence
        .mark_collections
        .iter()
        .map(|collection| &collection.key)
        .collect::<HashSet<_>>();
    let layer_ids = validate_layers_and_graph(
        project,
        "sequence",
        &sequence.layers,
        &sequence.composition_graph,
        &mark_keys,
    )?;
    for collection in &sequence.mark_collections {
        if collection
            .marks
            .iter()
            .any(|mark| mark.time.0 > sequence.duration.0)
        {
            return Err(sequence_error("a mark lies outside the sequence duration"));
        }
        if collection.marks.iter().any(|mark| {
            mark.label.as_deref().is_some_and(|label| {
                label.is_empty() || label.trim() != label || label.contains(['\n', '\r'])
            })
        }) {
            return Err(sequence_error(
                "a mark label must be one non-empty line without surrounding whitespace",
            ));
        }
    }

    let is_unchanged = |effect: &std::sync::Arc<crate::effect::EffectInst>| {
        unchanged
            .get(&effect.id)
            .is_some_and(|previous| std::sync::Arc::ptr_eq(previous, effect))
    };
    // An unchanged clip compared against an unchanged first clip in an unchanged
    // context only needs the checks that involve the sequence's own duration.
    let first_unchanged = sequence.effects.first().is_none_or(is_unchanged);
    for effect in &sequence.effects {
        if first_unchanged && is_unchanged(effect) {
            validate_timed_region(
                effect.start.0,
                effect.duration.0,
                sequence.duration.0,
                sampled_sequence_duration,
                "effect",
            )?;
            continue;
        }
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
            sampled_sequence_duration,
            "effect",
        )?;
        if active && &effect.target.layout != active_layout {
            return Err(sequence_error(
                "Effect target is not present in the active layout.",
            ));
        }
        if sequence
            .effects
            .first()
            .is_some_and(|first| first.target.layout != effect.target.layout)
        {
            return Err(sequence_error(
                "All effect targets in a sequence must use the same layout.",
            ));
        }
        if is_unchanged(effect) {
            continue;
        }
        validate_target(project, &effect.target)?;
        validate_effect_params(
            project,
            &effect.definition,
            &effect.param_overrides,
            &mark_keys,
            &format!("effect {}", effect.id.0),
        )?;
    }
    let mut automation_targets = HashMap::<_, Vec<_>>::new();
    let sequence_layout = sequence
        .effects
        .first()
        .map(|effect| &effect.target.layout)
        .or_else(|| {
            sequence
                .automation_clips
                .first()
                .map(|clip| &clip.row_target.layout)
        });
    for clip in &sequence.automation_clips {
        if sequence_layout.is_some_and(|layout| layout != &clip.row_target.layout) {
            return Err(sequence_error(
                "All timeline row targets in a sequence must use the same layout.",
            ));
        }
        if active && &clip.row_target.layout != active_layout {
            return Err(sequence_error(
                "Automation row target is not in the active layout.",
            ));
        }
        if project
            .layout(&clip.row_target.layout)
            .and_then(|layout| layout.fixture(clip.row_target.fixture))
            .is_none()
        {
            return Err(sequence_error("Automation row target is missing."));
        }
        validate_timed_region(
            clip.start.0,
            clip.duration.0,
            sequence.duration.0,
            sampled_sequence_duration,
            "automation clip",
        )?;
        clip.curve
            .validate()
            .map_err(|error| sequence_error(format!("automation curve is invalid: {error:?}")))?;
        let mut clip_targets = HashSet::new();
        for target in clip.targets() {
            if !clip_targets.insert(target) {
                return Err(sequence_error(
                    "an automation clip binds the same target more than once",
                ));
            }
            automation_targets
                .entry(target)
                .or_insert_with(Vec::new)
                .push(clip);
        }
        for binding in &clip.bindings {
            if automation_target_param(project, sequence, &binding.target)?
                .automation_mapping()
                .is_none()
            {
                return Err(sequence_error(
                    "automation target parameter does not support automation",
                ));
            }
        }
    }
    for clips in automation_targets.values() {
        if clips
            .iter()
            .enumerate()
            .any(|(index, clip)| clips[index + 1..].iter().any(|other| clip.overlaps(other)))
        {
            return Err(sequence_error(
                "automation clips bound to one target must not overlap",
            ));
        }
    }

    Ok(())
}

/// Clips and mark collections are referred to by name, so each name is valid
/// and unique among its kind.
fn validate_sequence_names(
    sequence: &Sequence,
    unchanged: &HashMap<&crate::effect::EffectInstId, &std::sync::Arc<crate::effect::EffectInst>>,
) -> Result<(), SequenceValidationError> {
    // An unchanged clip's name was accepted when it was admitted.
    let changed_effects = sequence.effects.iter().filter(|effect| {
        !unchanged
            .get(&effect.id)
            .is_some_and(|previous| std::sync::Arc::ptr_eq(previous, effect))
    });
    ensure_names(
        changed_effects.map(|effect| &effect.name).chain(
            sequence
                .mark_collections
                .iter()
                .map(|collection| &collection.key.name),
        ),
    )?;
    ensure_unique(
        sequence.effects.iter().map(|effect| &effect.name),
        "sequence clip names",
    )
}

/// The layers and composition graph of a sequence or rig, `owner`: layers
/// and graph nodes are referred to by name, so each name is valid and unique.
/// A layer node is named by its layer and the output node is `output`, so
/// operator nodes share that namespace with layers. Returns the layer ids.
fn validate_layers_and_graph<'a>(
    project: &DonderProject,
    owner: &str,
    layers: &'a [SequenceLayer],
    graph: &SequenceCompositionGraph,
    mark_keys: &HashSet<&MarkCollectionKey>,
) -> Result<HashSet<&'a SequenceLayerId>, SequenceValidationError> {
    let operators = || {
        graph.nodes.iter().filter_map(|node| match &node.kind {
            CompositionGraphNodeKind::Operator(operator) => Some(operator),
            _ => None,
        })
    };
    ensure_unique(
        layers.iter().map(|layer| layer.id.0),
        &format!("{owner} layer ids"),
    )?;
    ensure_names(
        layers
            .iter()
            .map(|layer| &layer.name)
            .chain(operators().map(|operator| &operator.name)),
    )?;
    ensure_unique(
        layers.iter().map(|layer| &layer.name),
        &format!("{owner} layer names"),
    )?;
    ensure_unique(
        layers
            .iter()
            .map(|layer| layer.name.as_str())
            .chain(core::iter::once("output"))
            .chain(operators().map(|operator| operator.name.as_str())),
        &format!("{owner} graph node names"),
    )?;
    validate_composition_graph(graph, &project.definitions.operators)
        .map_err(|error| sequence_error(error.message))?;
    for operator in operators() {
        for value in operator.params.values() {
            validate_param_references(project, value, mark_keys)?;
        }
    }
    let layer_ids = layers.iter().map(|layer| &layer.id).collect::<HashSet<_>>();
    let mut graph_layers = HashSet::new();
    for node in &graph.nodes {
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
    Ok(layer_ids)
}

/// A clip's or rig effect's target names an existing fixture or group.
fn validate_target(
    project: &DonderProject,
    target: &crate::layout::FixtureTarget,
) -> Result<(), SequenceValidationError> {
    let layout = project
        .layout(&target.layout)
        .ok_or_else(|| sequence_error("Effect target layout is missing."))?;
    if layout.fixture(target.fixture).is_none() {
        return Err(sequence_error(
            "Effect target is not present in the active layout.",
        ));
    }
    Ok(())
}

/// An effect's parameter overrides, of the effect `label` names: each
/// declared, typed by its declaration, and every parameter without a default
/// supplied.
fn validate_effect_params(
    project: &DonderProject,
    definition: &crate::effect::EffectRef,
    overrides: &IndexMap<donder_runtime_types::Identifier, EffectParamValue>,
    mark_keys: &HashSet<&MarkCollectionKey>,
    label: &str,
) -> Result<(), SequenceValidationError> {
    let definition = project
        .definitions
        .effects
        .resolve(definition)
        .ok_or_else(|| sequence_error(format!("{label} definition is missing")))?;
    for declaration in &definition.params {
        match overrides.get(&declaration.name) {
            Some(value) if effect_param_matches_type(value, &declaration.ty) => {
                validate_param_references(project, value, mark_keys)?;
            }
            Some(_) => {
                return Err(sequence_error(format!(
                    "{label} parameter `{}` has the wrong type",
                    declaration.name.as_str()
                )));
            }
            None if declaration.default.is_none() => {
                return Err(sequence_error(format!(
                    "{label} is missing required parameter `{}`",
                    declaration.name.as_str()
                )));
            }
            None => {}
        }
    }
    if overrides
        .keys()
        .any(|name| !definition.params.iter().any(|param| &param.name == name))
    {
        return Err(sequence_error(format!(
            "{label} contains an undeclared parameter"
        )));
    }
    Ok(())
}

pub fn validate_rig(project: &DonderProject, rig: &Rig) -> Result<(), SequenceValidationError> {
    let rig_name = rig
        .id
        .0
        .owned_path()
        .last()
        .and_then(|slot| match slot {
            crate::identity::OwnedObjectSlot::Rig(name) => Some(name.as_str()),
            _ => None,
        })
        .unwrap_or_else(|| rig.id.0.root_source().object());
    let error = |message: String| sequence_error(format!("rig `{rig_name}`: {message}"));
    let setup = project
        .setup(project.root.setup.id())
        .ok_or_else(|| error("active setup is missing".into()))?;
    let active = project
        .root
        .rigs
        .iter()
        .any(|source| source.id() == &rig.id);
    if rig.frame_rate == 0 {
        return Err(error("frame rate must be greater than zero".into()));
    }
    if rig.frame_rate > MAX_SEQUENCE_FRAME_RATE {
        return Err(error(format!(
            "frame rate exceeds the limit of {MAX_SEQUENCE_FRAME_RATE} frames per second"
        )));
    }
    let named =
        |result: Result<(), SequenceValidationError>| result.map_err(|inner| error(inner.message));
    named(ensure_names(
        rig.live_marks
            .iter()
            .map(|marks| &marks.key.name)
            .chain(rig.voices.iter().map(|voice| &voice.name))
            .chain(rig.switches.iter().map(|switch| &switch.name))
            .chain(rig.mappings.iter().map(|mapping| &mapping.name)),
    ))?;
    named(ensure_unique(
        rig.live_marks.iter().map(|marks| &marks.key.name),
        "live mark collection names",
    ))?;
    named(ensure_unique(
        rig.voices
            .iter()
            .map(|voice| &voice.name)
            .chain(rig.mappings.iter().map(|mapping| &mapping.name)),
        "voice and mapping names",
    ))?;
    named(ensure_unique(
        rig.switches.iter().map(|switch| &switch.name),
        "switch names",
    ))?;
    named(ensure_unique(
        rig.voices.iter().map(|voice| voice.id),
        "voice ids",
    ))?;
    named(ensure_unique(
        rig.switches.iter().map(|switch| switch.id),
        "switch ids",
    ))?;
    named(ensure_unique(
        rig.mappings.iter().map(|mapping| mapping.id),
        "mapping ids",
    ))?;
    let mark_keys = rig
        .live_marks
        .iter()
        .map(|marks| &marks.key)
        .collect::<HashSet<_>>();
    let layer_ids = validate_layers_and_graph(
        project,
        "rig",
        &rig.layers,
        &rig.composition_graph,
        &mark_keys,
    )
    .map_err(|inner| error(inner.message))?;
    let rig_layout = rig.effects().next().map(|effect| &effect.target.layout);
    for (index, effect) in rig.effects().enumerate() {
        if !layer_ids.contains(&effect.layer_id) {
            return Err(error(format!(
                "effect {} references a missing layer",
                index + 1
            )));
        }
        if active && &effect.target.layout != setup.layout.id() {
            return Err(error(
                "an effect target is not present in the active layout".into(),
            ));
        }
        if rig_layout.is_some_and(|layout| layout != &effect.target.layout) {
            return Err(error("all effect targets must use the same layout".into()));
        }
        named(validate_target(project, &effect.target))?;
        named(validate_effect_params(
            project,
            &effect.definition,
            &effect.param_overrides,
            &mark_keys,
            &format!("effect {}", index + 1),
        ))?;
    }
    let setup_devices = setup
        .input_devices
        .iter()
        .map(|source| source.id())
        .collect::<HashSet<_>>();
    for input in rig.inputs() {
        let device = project
            .input_device(&input.device)
            .ok_or_else(|| error("an input device is missing".into()))?;
        if !matches!(
            device.control(input.control).map(|control| &control.kind),
            Some(crate::input_device::InputControlKind::Button(_))
        ) {
            return Err(error("an input is not a button of its device".into()));
        }
        if active && !setup_devices.contains(&input.device) {
            return Err(error(
                "an input device is not in the project's setup".into(),
            ));
        }
    }
    for mapping in &rig.mappings {
        let mapping_error =
            |message: &str| error(format!("mapping `{}` {message}", mapping.name.as_str()));
        if let Some(When::On(switch) | When::Off(switch)) = mapping.when
            && !rig.switches.iter().any(|candidate| candidate.id == switch)
        {
            return Err(mapping_error("depends on a missing switch"));
        }
        match &mapping.action {
            MappingAction::Trigger(trigger) => {
                if let TriggerLength::Fixed(duration) = &trigger.length {
                    let sampled = sample_duration_from_donder_duration(duration).map_err(|_| {
                        mapping_error("has a duration beyond the runtime clock range")
                    })?;
                    if duration.0.is_zero() || sampled.as_ticks() == 0 {
                        return Err(mapping_error("has a `Fixed` duration that is not positive"));
                    }
                }
                if let VoicePolicy::Stack { count } = trigger.voices
                    && !(1..=MAX_STACKED_VOICES).contains(&count)
                {
                    return Err(mapping_error(&format!(
                        "stacks {count} voices; a stack has 1 to {MAX_STACKED_VOICES}"
                    )));
                }
            }
            MappingAction::Mark(key) => {
                if !mark_keys.contains(key) {
                    return Err(mapping_error(
                        "adds marks to a missing live mark collection",
                    ));
                }
            }
            MappingAction::Control { target, param } => {
                let effect = rig
                    .control_effect(*target)
                    .ok_or_else(|| mapping_error("controls a missing voice or trigger"))?;
                let declaration = project
                    .definitions
                    .effects
                    .resolve(&effect.definition)
                    .and_then(|definition| {
                        definition
                            .params
                            .iter()
                            .find(|declaration| &declaration.name == param)
                    })
                    .ok_or_else(|| mapping_error("controls a missing parameter"))?;
                if declaration.automation_mapping().is_none() {
                    return Err(mapping_error(&format!(
                        "controls `{}`, which cannot be automated",
                        param.as_str()
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Names others refer to are `snake_case`.
fn ensure_names<'a>(
    names: impl Iterator<Item = &'a donder_runtime_types::Identifier>,
) -> Result<(), SequenceValidationError> {
    for name in names {
        if !donder_language::NameKind::Object.accepts(name.as_str()) {
            return Err(sequence_error(format!(
                "`{}` is not a snake_case name",
                name.as_str()
            )));
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
            return Err(sequence_error(format!("{label} must be unique")));
        }
    }
    Ok(())
}

fn validate_timed_region(
    start: std::time::Duration,
    duration: std::time::Duration,
    sequence_duration: std::time::Duration,
    sampled_sequence_duration: SampleDuration,
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
    let sampled_start = sample_time_from_donder_time(&DonderTime(start))
        .map_err(|_| sequence_error(format!("{label} start exceeds the runtime clock range")))?;
    let sampled_duration = sample_duration_from_donder_duration(&DonderDuration(duration))
        .map_err(|_| sequence_error(format!("{label} duration exceeds the runtime clock range")))?;
    if sampled_duration.as_ticks() == 0 {
        return Err(sequence_error(format!(
            "{label} duration rounds to zero on the runtime clock"
        )));
    }
    if sampled_start
        .checked_add_duration(sampled_duration)
        .is_none_or(|sampled_end| sampled_end.as_ticks() > sampled_sequence_duration.as_ticks())
    {
        return Err(sequence_error(format!(
            "{label} end exceeds the runtime clock range or sequence duration after rounding"
        )));
    }
    Ok(())
}

fn validate_param_references(
    project: &DonderProject,
    value: &EffectParamValue,
    mark_keys: &HashSet<&MarkCollectionKey>,
) -> Result<(), SequenceValidationError> {
    match value {
        EffectParamValue::Marks(Some(key)) if !mark_keys.contains(key) => Err(sequence_error(
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
        EffectParamValue::Gradient(GradientSource::Inline(gradient)) => gradient
            .validate()
            .map_err(|error| sequence_error(format!("inline gradient is invalid: {error:?}"))),
        EffectParamValue::Array(values) => {
            for value in values {
                validate_param_references(project, value, mark_keys)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// The declaration an automation target addresses.
pub fn automation_target_param<'a>(
    project: &'a DonderProject,
    sequence: &'a Sequence,
    target: &AutomationTarget,
) -> Result<&'a ParamDecl, SequenceValidationError> {
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
        }
    }
}

fn sequence_error(message: impl Into<String>) -> SequenceValidationError {
    SequenceValidationError {
        message: message.into(),
    }
}
