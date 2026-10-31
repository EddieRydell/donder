mod problem;

use std::collections::{HashMap, HashSet};

use crate::console::{
    Action, Console, ConsoleClip, ConsoleClipId, InputEvent, LengthSource, MAX_COPIES, On,
    PlayLength,
};
use crate::effect::{OwnedCurve, OwnedGradient, ParamValue};
use crate::fixture::FixtureDefinitionId;
use crate::identity::ObjectId;
use crate::input_device::{ControlRef, InputControlKind};
use crate::operator::{param_matches_type, validate_composition_graph};
use crate::project::DonderProject;
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
    AdmissionProblem, AutomationProblem, ClipProblem, CompositionProblem, ConsoleValidationError,
    EventProblem, InputProblem, Member, MemberPart, NameProblem, OwnershipProblem, ParamProblem,
    ProblemSite, ProjectValidationError, RegionProblem, RouteProblem, SequenceValidationError,
    SetupProblem, TimingProblem, ValidationProblem,
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
    for console in project.objects::<crate::Console>() {
        validate_console(project, console)
            .map_err(|error| at(console.id)(ValidationProblem::Console(error)))?;
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
                .clips
                .iter()
                .map(|clip| (&clip.id, clip))
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
    if let Some(clip) = first_repeat(&sequence.clips, |clip| clip.id.clone()) {
        return Err(Error::DuplicateClipId(clip.id.clone()));
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

    let is_unchanged = |clip: &std::sync::Arc<crate::effect::Clip>| {
        unchanged
            .get(&clip.id)
            .is_some_and(|previous| std::sync::Arc::ptr_eq(previous, clip))
    };
    // An unchanged clip compared against an unchanged first clip in an unchanged
    // context only needs the checks that involve the sequence's own duration.
    let first_unchanged = sequence.clips.first().is_none_or(is_unchanged);
    for clip in &sequence.clips {
        let fail = |problem| Error::Clip {
            clip: clip.id.clone(),
            name: clip.name.clone(),
            problem,
        };
        let region = |region| fail(ClipProblem::Region(region));
        if first_unchanged && is_unchanged(clip) {
            validate_timed_region(
                clip.start.0,
                clip.duration.0,
                sequence.duration.0,
                sampled_sequence_duration,
            )
            .map_err(region)?;
            continue;
        }
        if !layer_ids.contains(&clip.layer_id) {
            return Err(fail(ClipProblem::MissingLayer));
        }
        validate_timed_region(
            clip.start.0,
            clip.duration.0,
            sequence.duration.0,
            sampled_sequence_duration,
        )
        .map_err(region)?;
        if active && &clip.target.layout != active_layout {
            return Err(fail(ClipProblem::NotInActiveLayout));
        }
        if sequence
            .clips
            .first()
            .is_some_and(|first| first.target.layout != clip.target.layout)
        {
            return Err(fail(ClipProblem::MixedLayouts));
        }
        if is_unchanged(clip) {
            continue;
        }
        validate_target(project, &clip.target).map_err(fail)?;
        validate_params(project, &clip.definition, &clip.param_overrides, &mark_keys)
            .map_err(fail)?;
    }
    let mut automation_targets = HashMap::<_, Vec<_>>::new();
    let sequence_layout = sequence
        .clips
        .first()
        .map(|clip| &clip.target.layout)
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
    unchanged: &HashMap<&crate::effect::ClipId, &std::sync::Arc<crate::effect::Clip>>,
) -> Result<(), NameProblem> {
    let clips = sequence
        .clips
        .iter()
        .map(|clip| (Member::Clip(clip.id.clone()), &clip.name));
    let marks = sequence.mark_collections.iter().map(|collection| {
        (
            Member::Marks(collection.key.name.clone()),
            &collection.key.name,
        )
    });
    // An unchanged clip's name was accepted when it was admitted.
    snake_case(
        sequence
            .clips
            .iter()
            .filter(|clip| {
                !unchanged
                    .get(&clip.id)
                    .is_some_and(|previous| std::sync::Arc::ptr_eq(previous, clip))
            })
            .map(|clip| (Member::Clip(clip.id.clone()), &clip.name))
            .chain(marks.clone()),
    )?;
    distinct(clips)?;
    distinct(marks)
}

/// The layers and composition graph of a sequence or console: layers and graph
/// nodes are referred to by name, so each name is valid and unique. A layer
/// node is named by its layer and the root node is `root`, so operator
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
    let root = donder_language::object_name("root");
    let root_node = graph
        .nodes
        .iter()
        .find(|node| matches!(node.kind, CompositionGraphNodeKind::Root))
        .map(|node| (Member::Node(node.id.clone()), &root));
    distinct(layer_names.chain(root_node).chain(node_names()))?;
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

/// A sequence or console clip's target names an existing fixture or group.
fn validate_target(
    project: &DonderProject,
    target: &crate::layout::FixtureTarget,
) -> Result<(), ClipProblem> {
    let layout = project
        .get::<crate::Layout>(target.layout)
        .ok_or(ClipProblem::MissingTargetLayout)?;
    if layout.fixture(target.fixture).is_none() {
        return Err(ClipProblem::MissingTarget);
    }
    Ok(())
}

/// A clip's parameter overrides: each declared, typed by its declaration,
/// and every parameter without a default supplied.
fn validate_params(
    project: &DonderProject,
    definition: &crate::effect::EffectDefinitionId,
    overrides: &IndexMap<Identifier, ParamValue>,
    mark_keys: &HashSet<&MarkCollectionKey>,
) -> Result<(), ClipProblem> {
    let definition = project
        .get::<crate::EffectDefinition>(*definition)
        .ok_or(ClipProblem::MissingDefinition)?;
    let param = |problem| Err(ClipProblem::Param(problem));
    if let Some(name) = overrides
        .keys()
        .find(|name| !definition.params().iter().any(|param| &param.name == *name))
    {
        return param(ParamProblem::Undeclared(name.clone()));
    }
    for declaration in definition.params() {
        match overrides.get(&declaration.name) {
            Some(value) if param_matches_type(value, &declaration.ty) => {
                validate_param_references(project, &declaration.name, value, mark_keys)
                    .map_err(ClipProblem::Param)?;
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

/// A console clip.
fn validate_console_clip(
    project: &DonderProject,
    clip: &ConsoleClip,
    layer_ids: &HashSet<&SequenceLayerId>,
    mark_keys: &HashSet<&MarkCollectionKey>,
    active_layout: Option<&crate::layout::LayoutId>,
    console_layout: Option<&crate::layout::LayoutId>,
) -> Result<(), ClipProblem> {
    if !layer_ids.contains(&clip.layer_id) {
        return Err(ClipProblem::MissingLayer);
    }
    if active_layout.is_some_and(|layout| &clip.target.layout != layout) {
        return Err(ClipProblem::NotInActiveLayout);
    }
    if console_layout.is_some_and(|layout| layout != &clip.target.layout) {
        return Err(ClipProblem::MixedLayouts);
    }
    validate_target(project, &clip.target)?;
    validate_params(project, &clip.definition, &clip.param_overrides, mark_keys)
}

/// The automation mapping of parameter `name` of `definition`: `None` when
/// it has no such parameter, `Some(None)` when it cannot be automated.
fn param_mapping(
    project: &DonderProject,
    definition: &crate::effect::EffectDefinitionId,
    name: &Identifier,
) -> Option<Option<donder_runtime_types::AutomationMapping>> {
    project
        .get::<crate::EffectDefinition>(*definition)
        .and_then(|definition| {
            definition
                .params()
                .iter()
                .find(|declaration| &declaration.name == name)
        })
        .map(ParamDecl::automation_mapping)
}

pub fn validate_console(
    project: &DonderProject,
    console: &Console,
) -> Result<(), ConsoleValidationError> {
    use ConsoleValidationError as Error;
    let setup = project
        .get::<crate::Setup>(*project.root.setup.id())
        .ok_or(Error::MissingActiveSetup)?;
    let active = project
        .root
        .consoles
        .iter()
        .any(|source| source.id() == &console.id);
    if console.frame_rate == 0 {
        return Err(Error::Timing(TimingProblem::ZeroFrameRate));
    }
    if console.frame_rate > MAX_SEQUENCE_FRAME_RATE {
        return Err(Error::Timing(TimingProblem::FrameRateAboveLimit));
    }
    let live_marks = console
        .live_marks
        .iter()
        .map(|marks| (Member::LiveMarks(marks.key.name.clone()), &marks.key.name));
    let clips = console
        .clips
        .iter()
        .map(|clip| (Member::ConsoleClip(clip.id), &clip.name));
    snake_case(live_marks.clone().chain(clips.clone()))?;
    distinct(live_marks)?;
    distinct(clips)?;
    if let Some(clip) = first_repeat(&console.clips, |clip| clip.id) {
        return Err(Error::DuplicateClipId(clip.id));
    }
    if let Some(on) = first_repeat(&console.events, |on| on.id) {
        return Err(Error::DuplicateEventId(on.id));
    }
    let mark_keys = console
        .live_marks
        .iter()
        .map(|marks| &marks.key)
        .collect::<HashSet<_>>();
    let layer_ids = validate_layers_and_graph::<Error>(
        project,
        &console.layers,
        &console.composition_graph,
        &mark_keys,
    )?;
    let console_layout = console.clips.first().map(|clip| &clip.target.layout);
    let active_layout = active.then(|| setup.layout.id());
    for clip in &console.clips {
        validate_console_clip(
            project,
            clip,
            &layer_ids,
            &mark_keys,
            active_layout,
            console_layout,
        )
        .map_err(|problem| Error::Clip {
            clip: clip.id,
            name: clip.name.clone(),
            problem,
        })?;
    }
    let mut always = HashSet::new();
    for id in &console.always {
        let clip = console.clip(*id).ok_or(Error::MissingAlwaysClip)?;
        if !always.insert(id) {
            return Err(Error::RepeatedAlwaysClip {
                clip: clip.id,
                name: clip.name.clone(),
            });
        }
    }
    for clip in &console.clips {
        let played = console
            .actions()
            .any(|action| matches!(action, Action::Play(play) if play.clip == clip.id));
        let held = always.contains(&clip.id)
            || console.actions().any(|action| {
                matches!(action, Action::Start { clip: id, .. } | Action::Toggle { clip: id, .. } if *id == clip.id)
            });
        if played && held {
            return Err(Error::PlayedAndHeld {
                clip: clip.id,
                name: clip.name.clone(),
            });
        }
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
    for on in &console.events {
        validate_event(project, console, on, &mark_keys, &control).map_err(|problem| {
            Error::Event {
                event: on.id,
                problem,
            }
        })?;
    }
    Ok(())
}

fn validate_event<'a>(
    project: &'a DonderProject,
    console: &Console,
    on: &On,
    mark_keys: &HashSet<&MarkCollectionKey>,
    control: &impl Fn(&ControlRef) -> Result<&'a InputControlKind, InputProblem>,
) -> Result<(), EventProblem> {
    let input = control(on.event.control()).map_err(EventProblem::Input)?;
    match on.event {
        InputEvent::Press(_) | InputEvent::Release(_) if !input.is_button() => {
            return Err(EventProblem::Input(InputProblem::NotAButton));
        }
        InputEvent::Move(_) if input.is_button() => {
            return Err(EventProblem::Input(InputProblem::NotAnAxis));
        }
        InputEvent::Move(_)
            if on
                .actions
                .iter()
                .any(|action| !matches!(action, Action::Set(_))) =>
        {
            return Err(EventProblem::AxisPlays);
        }
        _ => {}
    }
    let duration = |duration: &DonderDuration| {
        let sampled = sample_duration_from_donder_duration(duration)
            .map_err(|_| EventProblem::DurationBeyondClock)?;
        if duration.0.is_zero() || sampled.as_ticks() == 0 {
            return Err(EventProblem::NonPositiveDuration);
        }
        Ok(())
    };
    for action in &on.actions {
        let clip = match action.clip() {
            Some(id) => Some(console.clip(id).ok_or(EventProblem::MissingClip)?),
            None => None,
        };
        match action {
            Action::Play(play) => {
                if !(1..=MAX_COPIES).contains(&play.copies) {
                    return Err(EventProblem::Copies(play.copies));
                }
                match &play.length {
                    PlayLength::Fixed(length) => duration(length)?,
                    PlayLength::Scaled {
                        by,
                        shortest,
                        longest,
                    } => {
                        duration(shortest)?;
                        duration(longest)?;
                        if shortest.0 > longest.0 {
                            return Err(EventProblem::ShortestAboveLongest);
                        }
                        if let LengthSource::Axis(axis) = by
                            && control(axis).map_err(EventProblem::Input)?.is_button()
                        {
                            return Err(EventProblem::Input(InputProblem::NotAnAxis));
                        }
                    }
                }
            }
            Action::Mark(key) => {
                if !mark_keys.contains(key) {
                    return Err(EventProblem::MissingLiveMarks(key.name.clone()));
                }
            }
            Action::Set(target) => {
                let declaration = target_param(
                    project,
                    &console.composition_graph,
                    target,
                    |clip: &ConsoleClipId| console.clip(*clip).map(|clip| clip.definition),
                )
                .map_err(|problem| match problem {
                    AutomationProblem::MissingClip => EventProblem::MissingClip,
                    AutomationProblem::MissingNode => EventProblem::MissingNode,
                    AutomationProblem::MissingParam(param) => EventProblem::UnknownParam(param),
                    _ => unreachable!("a target's parameter is missing or found"),
                })?;
                let param = target.param();
                if declaration.automation_mapping().is_none() {
                    return Err(EventProblem::NotAutomatable(param.clone()));
                }
                if let AutomationTarget::ClipParam { clip_id, .. } = target
                    && console.actions().any(|action| {
                        action.clip() == Some(*clip_id)
                            && action.values().iter().any(|value| &value.param == param)
                    })
                {
                    return Err(EventProblem::SetAlsoPlayed(param.clone()));
                }
                // One control sets a parameter, by every event it answers.
                if console.events.iter().any(|other| {
                    other.event.control() != on.event.control()
                        && other
                            .actions
                            .iter()
                            .any(|action| matches!(action, Action::Set(set) if set == target))
                }) {
                    return Err(EventProblem::SetByOtherControl(param.clone()));
                }
            }
            Action::Start { .. } | Action::Stop { .. } | Action::Toggle { .. } => {}
        }
        if !action.values().is_empty() && !matches!(on.event, InputEvent::Press(_)) {
            return Err(EventProblem::ValuesWithoutPress);
        }
        let mut played = HashSet::new();
        for value in action.values() {
            let name = || value.param.clone();
            let Some(clip) = clip else {
                unreachable!("only actions that start a clip play values")
            };
            match param_mapping(project, &clip.definition, &value.param) {
                None => return Err(EventProblem::UnknownValue(name())),
                Some(None) => return Err(EventProblem::ValueNotAutomatable(name())),
                Some(Some(_)) => {}
            }
            if !played.insert(&value.param) {
                return Err(EventProblem::ValueTwice(name()));
            }
            if value.from.needs_note() && input.notes().is_none() {
                return Err(EventProblem::PitchWithoutNotes(name()));
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
    value: &ParamValue,
    mark_keys: &HashSet<&MarkCollectionKey>,
) -> Result<(), ParamProblem> {
    let name = || param.clone();
    match value {
        ParamValue::Marks(Some(key)) if !mark_keys.contains(key) => {
            Err(ParamProblem::MissingMarks(name()))
        }
        ParamValue::Curve(OwnedCurve::Inline(curve)) => curve
            .validate()
            .map_err(|error| ParamProblem::InvalidCurve(name(), error)),
        ParamValue::Curve(OwnedCurve::Reference(id))
            if !project
                .reusable::<crate::CurveDefinition>()
                .contains_key(id) =>
        {
            Err(ParamProblem::MissingCurve(name()))
        }
        ParamValue::Gradient(OwnedGradient::Reference(id))
            if !project
                .reusable::<crate::GradientDefinition>()
                .contains_key(id) =>
        {
            Err(ParamProblem::MissingGradient(name()))
        }
        ParamValue::Gradient(OwnedGradient::Inline(gradient)) => gradient
            .validate()
            .map_err(|error| ParamProblem::InvalidGradient(name(), error)),
        ParamValue::Array(values) => {
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
    target_param(project, &sequence.composition_graph, target, |clip_id| {
        sequence
            .clips
            .iter()
            .find(|clip| &clip.id == clip_id)
            .map(|clip| clip.definition)
    })
}

/// The declaration a sequence's or console's automation target addresses;
/// `clip_definition` finds the effect of a clip it names.
fn target_param<'a, Clip>(
    project: &'a DonderProject,
    graph: &SequenceCompositionGraph,
    target: &AutomationTarget<Clip>,
    clip_definition: impl FnOnce(&Clip) -> Option<crate::effect::EffectDefinitionId>,
) -> Result<&'a ParamDecl, AutomationProblem> {
    let find = |params: &'a [ParamDecl], param: &Identifier| {
        params
            .iter()
            .find(|declaration| &declaration.name == param)
            .ok_or_else(|| AutomationProblem::MissingParam(param.clone()))
    };
    match target {
        AutomationTarget::ClipParam { clip_id, param } => {
            let definition = clip_definition(clip_id).ok_or(AutomationProblem::MissingClip)?;
            let definition = project
                .get::<crate::EffectDefinition>(definition)
                .ok_or_else(|| AutomationProblem::MissingParam(param.clone()))?;
            find(definition.params(), param)
        }
        AutomationTarget::CompositionNodeParam { node_id, param } => {
            let operator = graph
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

impl From<NameProblem> for ConsoleValidationError {
    fn from(problem: NameProblem) -> Self {
        Self::Name(problem)
    }
}

impl From<CompositionProblem> for ConsoleValidationError {
    fn from(problem: CompositionProblem) -> Self {
        Self::Composition(problem)
    }
}
