mod problem;

use std::collections::{HashMap, HashSet};

use crate::effect::{CurveSource, EffectParamValue, GradientSource};
use crate::fixture::FixtureDefinitionId;
use crate::identity::ObjectId;
use crate::input_device::{ControlRef, InputControlKind};
use crate::operator::{effect_param_matches_type, validate_composition_graph};
use crate::project::DonderProject;
use crate::rig::{
    MAX_STACKED_VOICES, MappingAction, Rig, RigEffect, TriggerLength, VoicePolicy, When,
};
use crate::sequence::{
    AutomationTarget, CompositionGraphNodeKind, MarkCollectionKey, Sequence,
    SequenceCompositionGraph, SequenceLayer, SequenceLayerId, automation_curve_is_normalized,
};
use donder_language::compiler::ParamDecl;
use donder_language::{
    DonderDuration, DonderTime, sample_duration_from_donder_duration, sample_time_from_donder_time,
};
use donder_runtime_types::{Identifier, SampleDuration};
use indexmap::IndexMap;

pub use problem::{
    AdmissionProblem, AutomationProblem, CompositionProblem, EffectProblem, InputProblem,
    MappingProblem, Member, MemberPart, NameProblem, OwnershipProblem, ParamProblem, ProblemSite,
    ProjectValidationError, RegionProblem, RigValidationError, RouteProblem,
    SequenceValidationError, SetupProblem, TimingProblem, ValidationProblem,
};

pub(crate) const MAX_SEQUENCE_FRAME_COUNT: u32 = 250_000;
pub(crate) const MAX_SEQUENCE_FRAME_RATE: u32 = 1_000;

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
    for sequence in project.objects::<crate::Sequence>() {
        validate_sequence_since(
            project,
            sequence,
            previous.get::<crate::Sequence>(sequence.id),
        )
        .map_err(|error| at(sequence.id)(ValidationProblem::Sequence(error)))?;
    }
    Ok(())
}

pub fn validate_project(project: &DonderProject) -> Result<(), ProjectValidationError> {
    crate::ownership::validate_ownership(project)?;
    for (id, definition) in project.reusable::<crate::EffectDefinition>() {
        if definition.id != *id {
            return Err(at(*id)(ValidationProblem::DefinitionMismatch));
        }
    }
    for (id, definition) in project.reusable::<crate::OperatorDefinition>() {
        if definition.id != *id {
            return Err(at(*id)(ValidationProblem::DefinitionMismatch));
        }
    }
    for (id, definition) in project.reusable::<crate::CurveDefinition>() {
        definition
            .curve
            .validate()
            .map_err(|error| at(*id)(ValidationProblem::Curve(error)))?;
    }
    for (id, definition) in project.reusable::<crate::GradientDefinition>() {
        definition
            .gradient
            .validate()
            .map_err(|error| at(*id)(ValidationProblem::Gradient(error)))?;
    }
    if project
        .get::<crate::Setup>(*project.root.setup.id())
        .is_none()
    {
        return Err(at(project.root.id)(ValidationProblem::MissingSetup));
    }
    let mut counts = IndexMap::new();
    for (id, definition) in project.reusable::<crate::FixtureDefinition>() {
        let count = definition
            .validate(id)
            .map_err(|error| at(*id)(ValidationProblem::Fixture(error)))?;
        counts.insert(*id, count);
    }
    for controller in project.objects::<crate::Controller>() {
        controller
            .validate()
            .map_err(|error| at(controller.id)(ValidationProblem::Controller(error)))?;
    }
    for device in project.objects::<crate::InputDevice>() {
        device
            .validate()
            .map_err(|error| at(device.id)(ValidationProblem::InputDevice(error)))?;
    }
    for layout in project.objects::<crate::Layout>() {
        layout
            .validate(project.reusable::<crate::FixtureDefinition>())
            .map_err(|error| at(layout.id)(ValidationProblem::Layout(error)))?;
    }
    for patch in project.objects::<crate::Patch>() {
        validate_patch(project, patch, &counts).map_err(at(patch.id))?;
    }
    for setup in project.objects::<crate::Setup>() {
        validate_setup(project, setup).map_err(at(setup.id))?;
    }
    for sequence in project.objects::<crate::Sequence>() {
        validate_sequence(project, sequence)
            .map_err(|error| at(sequence.id)(ValidationProblem::Sequence(error)))?;
    }
    for rig in project.objects::<crate::Rig>() {
        validate_rig(project, rig).map_err(|error| at(rig.id)(ValidationProblem::Rig(error)))?;
    }
    Ok(())
}

/// Attach the object a problem belongs to.
pub(crate) fn at(
    object: impl Into<ObjectId>,
) -> impl FnOnce(ValidationProblem) -> ProjectValidationError {
    let object = object.into();
    move |problem| ProjectValidationError { object, problem }
}

fn validate_patch(
    project: &DonderProject,
    patch: &crate::patch::Patch,
    counts: &IndexMap<FixtureDefinitionId, u32>,
) -> Result<(), ValidationProblem> {
    let mut ids = HashSet::new();
    let mut occupied = std::collections::HashMap::<_, Vec<std::ops::Range<u32>>>::new();
    for route in &patch.routes {
        let fail = |problem| ValidationProblem::Route {
            route: route.id,
            problem,
        };
        if !ids.insert(route.id) {
            return Err(fail(RouteProblem::DuplicateId));
        }
        if !route.encoding.is_valid()
            || !route.gamma.is_finite()
            || route.gamma <= 0.0
            || !route.brightness.is_finite()
            || !(0.0..=1.0).contains(&route.brightness)
        {
            return Err(fail(RouteProblem::InvalidEncoding));
        }
        let layout = project
            .get::<crate::Layout>(route.target.layout)
            .ok_or_else(|| fail(RouteProblem::MissingLayout))?;
        if !matches!(
            layout
                .fixture(route.target.fixture)
                .map(|fixture| &fixture.kind),
            Some(crate::layout::LayoutFixtureKind::Fixture { .. })
        ) {
            return Err(fail(RouteProblem::TargetNotFixture));
        }
        let target_count = layout
            .target_pixel_count(&route.target, counts)
            .map_err(|error| fail(RouteProblem::Target(error)))?;
        let count = if let Some(span) = route.pixels {
            if span.count == 0
                || span
                    .start
                    .checked_add(span.count)
                    .is_none_or(|end| end > target_count)
            {
                return Err(fail(RouteProblem::SpanExceedsTarget));
            }
            span.count
        } else {
            target_count
        };
        let controller = project
            .get::<crate::Controller>(route.controller)
            .ok_or_else(|| fail(RouteProblem::MissingController))?;
        let port = controller
            .ports
            .iter()
            .find(|port| port.id == route.port)
            .ok_or_else(|| fail(RouteProblem::MissingPort))?;
        let width = count
            .checked_mul(route.encoding.channel_order().len() as u32)
            .ok_or_else(|| fail(RouteProblem::ChannelOverflow))?;
        let start = u32::from(route.start_slot);
        let end = start
            .checked_add(width)
            .filter(|&end| end <= u32::from(port.slot_count))
            .ok_or_else(|| fail(RouteProblem::OutputExceedsPort))?;
        let ranges = occupied.entry((&route.controller, route.port)).or_default();
        if width != 0
            && ranges
                .iter()
                .any(|range| start < range.end && range.start < end)
        {
            return Err(fail(RouteProblem::OutputsOverlap));
        }
        if width != 0 {
            ranges.push(start..end);
        }
    }
    Ok(())
}

/// The setup's members exist and are distinct by ownership validation.
fn validate_setup(
    project: &DonderProject,
    setup: &crate::setup::Setup,
) -> Result<(), ValidationProblem> {
    if setup.controllers.len() as u128 > u128::from(u32::MAX) + 1 {
        return Err(ValidationProblem::Setup(SetupProblem::TooManyControllers));
    }
    if project.get::<crate::Layout>(*setup.layout.id()).is_none() {
        return Err(ValidationProblem::MissingLayout);
    }
    let patch = project
        .get::<crate::Patch>(*setup.patch.id())
        .ok_or(ValidationProblem::MissingPatch)?;
    let mut controllers = HashSet::new();
    for controller in &setup.controllers {
        if project.get::<crate::Controller>(*controller.id()).is_none() {
            return Err(ValidationProblem::MissingController);
        }
        controllers.insert(controller.id());
    }
    let mut osc_ports = HashSet::new();
    for device in &setup.input_devices {
        let Some(input_device) = project.get::<crate::InputDevice>(*device.id()) else {
            return Err(ValidationProblem::MissingInputDevice);
        };
        if let crate::InputDriver::Osc { port } = input_device.driver
            && !osc_ports.insert(port)
        {
            return Err(ValidationProblem::Setup(SetupProblem::SharedOscPort(port)));
        }
    }
    for route in &patch.routes {
        if &route.target.layout != setup.layout.id() {
            return Err(ValidationProblem::Setup(SetupProblem::RouteOnOtherLayout(
                route.id,
            )));
        }
        if !controllers.contains(&route.controller) {
            return Err(ValidationProblem::Setup(
                SetupProblem::RouteControllerInactive(route.id),
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
    use SequenceValidationError as Error;
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
        .get::<crate::Setup>(*project.root.setup.id())
        .ok_or(Error::MissingActiveSetup)?;
    let active_layout = setup.layout.id();
    let active = project
        .root
        .sequences
        .iter()
        .any(|source| source.id() == &sequence.id);

    let timing = |problem| Err(Error::Timing(problem));
    if sequence.frame_rate == 0 {
        return timing(TimingProblem::ZeroFrameRate);
    }
    if sequence.frame_rate > MAX_SEQUENCE_FRAME_RATE {
        return timing(TimingProblem::FrameRateAboveLimit);
    }
    if sequence.duration.0.is_zero() {
        return timing(TimingProblem::ZeroDuration);
    }
    let Ok(sampled_sequence_duration) = sample_duration_from_donder_duration(&sequence.duration)
    else {
        return timing(TimingProblem::DurationBeyondClock);
    };
    if sampled_sequence_duration.as_ticks() == 0 {
        return timing(TimingProblem::DurationRoundsToZero);
    }
    if sequence.frame_count() > u128::from(MAX_SEQUENCE_FRAME_COUNT) {
        return timing(TimingProblem::TooManyFrames);
    }
    if let Some(effect) = first_repeat(&sequence.effects, |effect| effect.id.clone()) {
        return Err(Error::DuplicateClipId(effect.id.clone()));
    }
    if let Some(clip) = first_repeat(&sequence.automation_clips, |clip| clip.id.0) {
        return Err(Error::DuplicateAutomationClipId(clip.id.clone()));
    }
    validate_sequence_names(sequence, &unchanged).map_err(Error::Name)?;

    let mark_keys = sequence
        .mark_collections
        .iter()
        .map(|collection| &collection.key)
        .collect::<HashSet<_>>();
    let layer_ids = validate_layers_and_graph::<Error>(
        project,
        &sequence.layers,
        &sequence.composition_graph,
        &mark_keys,
    )?;
    for collection in &sequence.mark_collections {
        let name = || collection.key.name.clone();
        if collection
            .marks
            .iter()
            .any(|mark| mark.time.0 > sequence.duration.0)
        {
            return Err(Error::MarkOutsideDuration(name()));
        }
        if collection.marks.iter().any(|mark| {
            mark.label.as_deref().is_some_and(|label| {
                label.is_empty() || label.trim() != label || label.contains(['\n', '\r'])
            })
        }) {
            return Err(Error::InvalidMarkLabel(name()));
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
        let fail = |problem| Error::Clip {
            clip: effect.id.clone(),
            name: effect.name.clone(),
            problem,
        };
        let region = |region| fail(EffectProblem::Region(region));
        if first_unchanged && is_unchanged(effect) {
            validate_timed_region(
                effect.start.0,
                effect.duration.0,
                sequence.duration.0,
                sampled_sequence_duration,
            )
            .map_err(region)?;
            continue;
        }
        if !layer_ids.contains(&effect.layer_id) {
            return Err(fail(EffectProblem::MissingLayer));
        }
        validate_timed_region(
            effect.start.0,
            effect.duration.0,
            sequence.duration.0,
            sampled_sequence_duration,
        )
        .map_err(region)?;
        if active && &effect.target.layout != active_layout {
            return Err(fail(EffectProblem::NotInActiveLayout));
        }
        if sequence
            .effects
            .first()
            .is_some_and(|first| first.target.layout != effect.target.layout)
        {
            return Err(fail(EffectProblem::MixedLayouts));
        }
        if is_unchanged(effect) {
            continue;
        }
        validate_target(project, &effect.target).map_err(fail)?;
        validate_effect_params(
            project,
            &effect.definition,
            &effect.param_overrides,
            &mark_keys,
        )
        .map_err(fail)?;
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
        let fail = |problem| Error::Automation {
            clip: clip.id.clone(),
            problem,
        };
        if sequence_layout.is_some_and(|layout| layout != &clip.row_target.layout) {
            return Err(fail(AutomationProblem::MixedLayouts));
        }
        if active && &clip.row_target.layout != active_layout {
            return Err(fail(AutomationProblem::NotInActiveLayout));
        }
        if project
            .get::<crate::Layout>(clip.row_target.layout)
            .and_then(|layout| layout.fixture(clip.row_target.fixture))
            .is_none()
        {
            return Err(fail(AutomationProblem::MissingRow));
        }
        validate_timed_region(
            clip.start.0,
            clip.duration.0,
            sequence.duration.0,
            sampled_sequence_duration,
        )
        .map_err(|problem| fail(AutomationProblem::Region(problem)))?;
        clip.curve
            .validate()
            .map_err(|error| fail(AutomationProblem::InvalidCurve(error)))?;
        if !automation_curve_is_normalized(&clip.curve) {
            return Err(fail(AutomationProblem::CurveOutOfRange));
        }
        let mut clip_targets = HashSet::new();
        for target in clip.targets() {
            if !clip_targets.insert(target) {
                return Err(fail(AutomationProblem::BindsTwice));
            }
            automation_targets
                .entry(target)
                .or_insert_with(Vec::new)
                .push(clip);
        }
        for binding in &clip.bindings {
            let declaration =
                automation_target_param(project, sequence, &binding.target).map_err(fail)?;
            if declaration.automation_mapping().is_none() {
                return Err(fail(AutomationProblem::NotAutomatable(
                    declaration.name.clone(),
                )));
            }
        }
    }
    for clips in automation_targets.values() {
        for (index, clip) in clips.iter().enumerate() {
            if let Some(other) = clips[index + 1..].iter().find(|other| clip.overlaps(other)) {
                return Err(Error::Automation {
                    clip: other.id.clone(),
                    problem: AutomationProblem::Overlap,
                });
            }
        }
    }

    Ok(())
}

/// Clips and mark collections are referred to by name, so each name is valid
/// and unique among its kind.
fn validate_sequence_names(
    sequence: &Sequence,
    unchanged: &HashMap<&crate::effect::EffectInstId, &std::sync::Arc<crate::effect::EffectInst>>,
) -> Result<(), NameProblem> {
    let clips = sequence
        .effects
        .iter()
        .map(|effect| (Member::Clip(effect.id.clone()), &effect.name));
    let marks = sequence.mark_collections.iter().map(|collection| {
        (
            Member::Marks(collection.key.name.clone()),
            &collection.key.name,
        )
    });
    // An unchanged clip's name was accepted when it was admitted.
    snake_case(
        sequence
            .effects
            .iter()
            .filter(|effect| {
                !unchanged
                    .get(&effect.id)
                    .is_some_and(|previous| std::sync::Arc::ptr_eq(previous, effect))
            })
            .map(|effect| (Member::Clip(effect.id.clone()), &effect.name))
            .chain(marks.clone()),
    )?;
    distinct(clips)?;
    distinct(marks)
}

/// The layers and composition graph of a sequence or rig: layers and graph
/// nodes are referred to by name, so each name is valid and unique. A layer
/// node is named by its layer and the output node is `output`, so operator
/// nodes share that namespace with layers. Returns the layer ids.
fn validate_layers_and_graph<'a, E: From<NameProblem> + From<CompositionProblem>>(
    project: &DonderProject,
    layers: &'a [SequenceLayer],
    graph: &SequenceCompositionGraph,
    mark_keys: &HashSet<&MarkCollectionKey>,
) -> Result<HashSet<&'a SequenceLayerId>, E> {
    let operators = || {
        graph.nodes.iter().filter_map(|node| match &node.kind {
            CompositionGraphNodeKind::Operator(operator) => Some((&node.id, operator)),
            _ => None,
        })
    };
    if let Some(layer) = first_repeat(layers, |layer| layer.id.clone()) {
        return Err(CompositionProblem::DuplicateLayerId(layer.id.clone()).into());
    }
    let layer_names = layers
        .iter()
        .map(|layer| (Member::Layer(layer.id.clone()), &layer.name));
    let node_names =
        || operators().map(|(id, operator)| (Member::Node(id.clone()), &operator.name));
    snake_case(layer_names.clone().chain(node_names()))?;
    distinct(layer_names.clone())?;
    let output = donder_language::object_name("output");
    let output_node = graph
        .nodes
        .iter()
        .find(|node| matches!(node.kind, CompositionGraphNodeKind::Output))
        .map(|node| (Member::Node(node.id.clone()), &output));
    distinct(layer_names.chain(output_node).chain(node_names()))?;
    validate_composition_graph(graph, project.reusable::<crate::OperatorDefinition>())
        .map_err(CompositionProblem::Graph)?;
    for (node, operator) in operators() {
        for (name, value) in &operator.params {
            validate_param_references(project, name, value, mark_keys).map_err(|problem| {
                CompositionProblem::Param {
                    node: node.clone(),
                    problem,
                }
            })?;
        }
    }
    let layer_ids = layers.iter().map(|layer| &layer.id).collect::<HashSet<_>>();
    let mut graph_layers = HashSet::new();
    for node in &graph.nodes {
        if let CompositionGraphNodeKind::Layer { layer_id } = &node.kind {
            if !layer_ids.contains(layer_id) {
                return Err(CompositionProblem::MissingLayer(node.id.clone()).into());
            }
            if !graph_layers.insert(layer_id) {
                return Err(CompositionProblem::RepeatedLayer(node.id.clone()).into());
            }
        }
    }
    Ok(layer_ids)
}

/// A clip's or rig effect's target names an existing fixture or group.
fn validate_target(
    project: &DonderProject,
    target: &crate::layout::FixtureTarget,
) -> Result<(), EffectProblem> {
    let layout = project
        .get::<crate::Layout>(target.layout)
        .ok_or(EffectProblem::MissingTargetLayout)?;
    if layout.fixture(target.fixture).is_none() {
        return Err(EffectProblem::MissingTarget);
    }
    Ok(())
}

/// An effect's parameter overrides: each declared, typed by its declaration,
/// and every parameter without a default supplied.
fn validate_effect_params(
    project: &DonderProject,
    definition: &crate::effect::EffectDefinitionId,
    overrides: &IndexMap<Identifier, EffectParamValue>,
    mark_keys: &HashSet<&MarkCollectionKey>,
) -> Result<(), EffectProblem> {
    let definition = project
        .get::<crate::EffectDefinition>(*definition)
        .ok_or(EffectProblem::MissingDefinition)?;
    let param = |problem| Err(EffectProblem::Param(problem));
    if let Some(name) = overrides
        .keys()
        .find(|name| !definition.params().iter().any(|param| &param.name == *name))
    {
        return param(ParamProblem::Undeclared(name.clone()));
    }
    for declaration in definition.params() {
        match overrides.get(&declaration.name) {
            Some(value) if effect_param_matches_type(value, &declaration.ty) => {
                validate_param_references(project, &declaration.name, value, mark_keys)
                    .map_err(EffectProblem::Param)?;
            }
            Some(_) => return param(ParamProblem::WrongType(declaration.name.clone())),
            None if declaration.default.is_none() => {
                return param(ParamProblem::MissingRequired(declaration.name.clone()));
            }
            None => {}
        }
    }
    Ok(())
}

/// A rig voice's or trigger's effect.
fn validate_rig_effect(
    project: &DonderProject,
    effect: &RigEffect,
    layer_ids: &HashSet<&SequenceLayerId>,
    mark_keys: &HashSet<&MarkCollectionKey>,
    active_layout: Option<&crate::layout::LayoutId>,
    rig_layout: Option<&crate::layout::LayoutId>,
) -> Result<(), EffectProblem> {
    if !layer_ids.contains(&effect.layer_id) {
        return Err(EffectProblem::MissingLayer);
    }
    if active_layout.is_some_and(|layout| &effect.target.layout != layout) {
        return Err(EffectProblem::NotInActiveLayout);
    }
    if rig_layout.is_some_and(|layout| layout != &effect.target.layout) {
        return Err(EffectProblem::MixedLayouts);
    }
    validate_target(project, &effect.target)?;
    validate_effect_params(
        project,
        &effect.definition,
        &effect.param_overrides,
        mark_keys,
    )
}

pub fn validate_rig(project: &DonderProject, rig: &Rig) -> Result<(), RigValidationError> {
    use RigValidationError as Error;
    let setup = project
        .get::<crate::Setup>(*project.root.setup.id())
        .ok_or(Error::MissingActiveSetup)?;
    let active = project
        .root
        .rigs
        .iter()
        .any(|source| source.id() == &rig.id);
    if rig.frame_rate == 0 {
        return Err(Error::Timing(TimingProblem::ZeroFrameRate));
    }
    if rig.frame_rate > MAX_SEQUENCE_FRAME_RATE {
        return Err(Error::Timing(TimingProblem::FrameRateAboveLimit));
    }
    let live_marks = rig
        .live_marks
        .iter()
        .map(|marks| (Member::LiveMarks(marks.key.name.clone()), &marks.key.name));
    let voices = rig
        .voices
        .iter()
        .map(|voice| (Member::Voice(voice.id), &voice.name));
    let switches = rig
        .switches
        .iter()
        .map(|switch| (Member::Switch(switch.id), &switch.name));
    let mappings = rig
        .mappings
        .iter()
        .map(|mapping| (Member::Mapping(mapping.id), &mapping.name));
    snake_case(
        live_marks
            .clone()
            .chain(voices.clone())
            .chain(switches.clone())
            .chain(mappings.clone()),
    )?;
    distinct(live_marks)?;
    distinct(voices.chain(mappings))?;
    distinct(switches)?;
    if let Some(voice) = first_repeat(&rig.voices, |voice| voice.id) {
        return Err(Error::DuplicateVoiceId(voice.id));
    }
    if let Some(switch) = first_repeat(&rig.switches, |switch| switch.id) {
        return Err(Error::DuplicateSwitchId(switch.id));
    }
    if let Some(mapping) = first_repeat(&rig.mappings, |mapping| mapping.id) {
        return Err(Error::DuplicateMappingId(mapping.id));
    }
    let mark_keys = rig
        .live_marks
        .iter()
        .map(|marks| &marks.key)
        .collect::<HashSet<_>>();
    let layer_ids = validate_layers_and_graph::<Error>(
        project,
        &rig.layers,
        &rig.composition_graph,
        &mark_keys,
    )?;
    let rig_layout = rig.effects().next().map(|effect| &effect.target.layout);
    let active_layout = active.then(|| setup.layout.id());
    let effect = |effect: &RigEffect| {
        validate_rig_effect(
            project,
            effect,
            &layer_ids,
            &mark_keys,
            active_layout,
            rig_layout,
        )
    };
    for voice in &rig.voices {
        effect(&voice.effect).map_err(|problem| Error::Voice {
            voice: voice.id,
            name: voice.name.clone(),
            problem,
        })?;
    }
    let setup_devices = setup
        .input_devices
        .iter()
        .map(|source| source.id())
        .collect::<HashSet<_>>();
    let control = |input: &ControlRef| -> Result<&InputControlKind, InputProblem> {
        let device = project
            .get::<crate::InputDevice>(input.device)
            .ok_or(InputProblem::MissingDevice)?;
        let control = device
            .control(input.control)
            .ok_or(InputProblem::NotAControl)?;
        if active && !setup_devices.contains(&input.device) {
            return Err(InputProblem::NotInSetup);
        }
        Ok(&control.kind)
    };
    for switch in &rig.switches {
        let fail = |problem| Error::Switch {
            switch: switch.id,
            name: switch.name.clone(),
            problem,
        };
        if !control(&switch.input).map_err(fail)?.is_button() {
            return Err(fail(InputProblem::NotAButton));
        }
    }
    for mapping in &rig.mappings {
        validate_mapping(project, rig, mapping, &mark_keys, &control, &effect).map_err(
            |problem| Error::Mapping {
                mapping: mapping.id,
                name: mapping.name.clone(),
                problem,
            },
        )?;
    }
    Ok(())
}

fn validate_mapping<'a>(
    project: &'a DonderProject,
    rig: &Rig,
    mapping: &crate::rig::Mapping,
    mark_keys: &HashSet<&MarkCollectionKey>,
    control: &impl Fn(&ControlRef) -> Result<&'a InputControlKind, InputProblem>,
    effect: &impl Fn(&RigEffect) -> Result<(), EffectProblem>,
) -> Result<(), MappingProblem> {
    let input = control(&mapping.input).map_err(MappingProblem::Input)?;
    match &mapping.action {
        MappingAction::Control { .. } if input.is_pressed() && !input.is_button() => {
            return Err(MappingProblem::KeysDriveControl);
        }
        MappingAction::Trigger(_) | MappingAction::Mark(_) if !input.is_pressed() => {
            return Err(MappingProblem::DialDrivesTrigger);
        }
        _ => {}
    }
    if let Some(When::On(switch) | When::Off(switch)) = mapping.when
        && !rig.switches.iter().any(|candidate| candidate.id == switch)
    {
        return Err(MappingProblem::MissingSwitch);
    }
    let param = |definition: &crate::effect::EffectDefinitionId, name: &Identifier| {
        project
            .get::<crate::EffectDefinition>(*definition)
            .and_then(|definition| {
                definition
                    .params()
                    .iter()
                    .find(|declaration| &declaration.name == name)
            })
            .map(ParamDecl::automation_mapping)
    };
    match &mapping.action {
        MappingAction::Trigger(trigger) => {
            effect(&trigger.effect).map_err(MappingProblem::Effect)?;
            if let TriggerLength::Fixed(duration) = &trigger.length {
                let sampled = sample_duration_from_donder_duration(duration)
                    .map_err(|_| MappingProblem::DurationBeyondClock)?;
                if duration.0.is_zero() || sampled.as_ticks() == 0 {
                    return Err(MappingProblem::NonPositiveFixedDuration);
                }
            }
            if let VoicePolicy::Stack { count } = trigger.voices
                && !(1..=MAX_STACKED_VOICES).contains(&count)
            {
                return Err(MappingProblem::StackSize(count));
            }
            let mut played = HashSet::new();
            for value in &trigger.values {
                let name = || value.param.clone();
                match param(&trigger.effect.definition, &value.param) {
                    None => return Err(MappingProblem::UnknownValue(name())),
                    Some(None) => return Err(MappingProblem::ValueNotAutomatable(name())),
                    Some(Some(_)) => {}
                }
                if !played.insert(&value.param) {
                    return Err(MappingProblem::ValueTwice(name()));
                }
                if value.from.needs_note() && input.notes().is_none() {
                    return Err(MappingProblem::PitchWithoutNotes(name()));
                }
            }
        }
        MappingAction::Mark(key) => {
            if !mark_keys.contains(key) {
                return Err(MappingProblem::MissingLiveMarks(key.name.clone()));
            }
        }
        MappingAction::Control {
            target,
            param: name,
        } => {
            let effect = rig
                .control_effect(*target)
                .ok_or(MappingProblem::MissingControlTarget)?;
            match param(&effect.definition, name) {
                None => return Err(MappingProblem::UnknownControlParam(name.clone())),
                Some(None) => return Err(MappingProblem::ControlNotAutomatable(name.clone())),
                Some(Some(_)) => {}
            }
        }
    }
    Ok(())
}

/// Names others refer to are `snake_case`.
fn snake_case<'a>(
    names: impl Iterator<Item = (Member, &'a Identifier)>,
) -> Result<(), NameProblem> {
    for (member, name) in names {
        if !donder_language::NameKind::Object.accepts(name.as_str()) {
            return Err(NameProblem::NotSnakeCase(member, name.clone()));
        }
    }
    Ok(())
}

/// Names others refer to are unique; the second of two is the duplicate.
fn distinct<'a>(names: impl Iterator<Item = (Member, &'a Identifier)>) -> Result<(), NameProblem> {
    let mut seen = HashSet::new();
    for (member, name) in names {
        if !seen.insert(name) {
            return Err(NameProblem::Duplicate(member, name.clone()));
        }
    }
    Ok(())
}

/// The first item whose key an earlier item has.
fn first_repeat<T, K: Eq + std::hash::Hash>(items: &[T], key: impl Fn(&T) -> K) -> Option<&T> {
    let mut seen = HashSet::new();
    items.iter().find(|item| !seen.insert(key(item)))
}

fn validate_timed_region(
    start: std::time::Duration,
    duration: std::time::Duration,
    sequence_duration: std::time::Duration,
    sampled_sequence_duration: SampleDuration,
) -> Result<(), RegionProblem> {
    if duration.is_zero() {
        return Err(RegionProblem::ZeroDuration);
    }
    let end = start.checked_add(duration).ok_or(RegionProblem::Overflow)?;
    if end > sequence_duration {
        return Err(RegionProblem::BeyondSequence);
    }
    let sampled_start = sample_time_from_donder_time(&DonderTime(start))
        .map_err(|_| RegionProblem::StartBeyondClock)?;
    let sampled_duration = sample_duration_from_donder_duration(&DonderDuration(duration))
        .map_err(|_| RegionProblem::DurationBeyondClock)?;
    if sampled_duration.as_ticks() == 0 {
        return Err(RegionProblem::RoundsToZero);
    }
    if sampled_start
        .checked_add_duration(sampled_duration)
        .is_none_or(|sampled_end| sampled_end.as_ticks() > sampled_sequence_duration.as_ticks())
    {
        return Err(RegionProblem::EndBeyondAfterRounding);
    }
    Ok(())
}

/// The value of parameter `param` refers to existing marks, curves and
/// gradients, and any inline curve or gradient is valid.
fn validate_param_references(
    project: &DonderProject,
    param: &Identifier,
    value: &EffectParamValue,
    mark_keys: &HashSet<&MarkCollectionKey>,
) -> Result<(), ParamProblem> {
    let name = || param.clone();
    match value {
        EffectParamValue::Marks(Some(key)) if !mark_keys.contains(key) => {
            Err(ParamProblem::MissingMarks(name()))
        }
        EffectParamValue::Curve(CurveSource::Inline(curve)) => curve
            .validate()
            .map_err(|error| ParamProblem::InvalidCurve(name(), error)),
        EffectParamValue::Curve(CurveSource::Reference(id))
            if !project
                .reusable::<crate::CurveDefinition>()
                .contains_key(id) =>
        {
            Err(ParamProblem::MissingCurve(name()))
        }
        EffectParamValue::Gradient(GradientSource::Reference(id))
            if !project
                .reusable::<crate::GradientDefinition>()
                .contains_key(id) =>
        {
            Err(ParamProblem::MissingGradient(name()))
        }
        EffectParamValue::Gradient(GradientSource::Inline(gradient)) => gradient
            .validate()
            .map_err(|error| ParamProblem::InvalidGradient(name(), error)),
        EffectParamValue::Array(values) => {
            for value in values {
                validate_param_references(project, param, value, mark_keys)?;
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
) -> Result<&'a ParamDecl, AutomationProblem> {
    let find = |params: &'a [ParamDecl], param: &Identifier| {
        params
            .iter()
            .find(|declaration| &declaration.name == param)
            .ok_or_else(|| AutomationProblem::MissingParam(param.clone()))
    };
    match target {
        AutomationTarget::EffectParam { effect_id, param } => {
            let effect = sequence
                .effects
                .iter()
                .find(|effect| &effect.id == effect_id)
                .ok_or(AutomationProblem::MissingEffect)?;
            let definition = project
                .get::<crate::EffectDefinition>(effect.definition)
                .ok_or_else(|| AutomationProblem::MissingParam(param.clone()))?;
            find(definition.params(), param)
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
                .ok_or(AutomationProblem::MissingNode)?;
            let definition = project
                .get::<crate::OperatorDefinition>(operator.operator)
                .ok_or_else(|| AutomationProblem::MissingParam(param.clone()))?;
            find(definition.params(), param)
        }
    }
}

impl From<NameProblem> for SequenceValidationError {
    fn from(problem: NameProblem) -> Self {
        Self::Name(problem)
    }
}

impl From<CompositionProblem> for SequenceValidationError {
    fn from(problem: CompositionProblem) -> Self {
        Self::Composition(problem)
    }
}

impl From<NameProblem> for RigValidationError {
    fn from(problem: NameProblem) -> Self {
        Self::Name(problem)
    }
}

impl From<CompositionProblem> for RigValidationError {
    fn from(problem: CompositionProblem) -> Self {
        Self::Composition(problem)
    }
}
