mod problem;
mod repair;

use std::collections::{HashMap, HashSet};

use crate::console::{
    Action, Console, ConsoleClip, ConsoleClipId, InputEvent, LengthSource, MAX_COPIES, On,
    PlayLength,
};
use crate::controller::ControllerOutput;
use crate::effect::{OwnedCurve, OwnedGradient, ParamValue};
use crate::fixture::FixtureDefinitionId;
use crate::identity::ObjectId;
use crate::input_device::{ControlRef, InputControlKind};
use crate::operator::{param_matches, validate_composition_graph};
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
    PlayedProblem, ProblemSite, ProjectValidationError, RegionProblem, RouteProblem,
    SequenceValidationError, SetupProblem, TimingProblem, ValidationProblem,
};
pub(crate) use repair::repair;
pub use repair::{GraphOwner, ParamOwner, Repair};

pub(crate) const MAX_SEQUENCE_FRAME_COUNT: u32 = 250_000;
pub(crate) const MAX_SEQUENCE_FRAME_RATE: u32 = 1_000;

pub fn validate_project(project: &DonderProject) -> Result<(), ProjectValidationError> {
    first(project_problems(project, None))
}

/// Every problem of `project`, in the order [`validate_project`] meets them:
/// one for each member that breaks a rule, or the one that stops an object's
/// checks. Objects that depend on definitions, layouts and controllers are
/// checked only when those pass. With `previous`, a project that already
/// passed validation, `project` is checked as an edit of it: when only
/// sequences changed, only sequences are checked, and a clip that is still the
/// allocation `previous` held skips the checks that depend on nothing else that
/// could have changed.
pub(crate) fn project_problems(
    project: &DonderProject,
    previous: Option<&DonderProject>,
) -> Vec<ProjectValidationError> {
    let mut problems = Vec::new();
    if let Err(problem) = crate::ownership::validate_ownership(project) {
        problems.push(problem);
        return problems;
    }
    if let Some(previous) = previous
        && project.only_sequences_differ_from(previous)
    {
        for sequence in project.objects::<crate::Sequence>() {
            sequence_problems(
                project,
                sequence,
                previous.get::<crate::Sequence>(sequence.id),
                &mut problems,
            );
        }
        return problems;
    }
    for (id, definition) in project.reusable::<crate::EffectDefinition>() {
        if definition.id != *id {
            problems.push(at(*id)(ValidationProblem::DefinitionMismatch));
        }
    }
    for (id, definition) in project.reusable::<crate::OperatorDefinition>() {
        if definition.id != *id {
            problems.push(at(*id)(ValidationProblem::DefinitionMismatch));
        }
    }
    for (id, definition) in project.reusable::<crate::CurveDefinition>() {
        if let Err(error) = definition.curve.validate() {
            problems.push(at(*id)(ValidationProblem::Curve(error)));
        }
    }
    for (id, definition) in project.reusable::<crate::GradientDefinition>() {
        if let Err(error) = definition.gradient.validate() {
            problems.push(at(*id)(ValidationProblem::Gradient(error)));
        }
    }
    if project
        .get::<crate::Setup>(*project.root.setup.id())
        .is_none()
    {
        problems.push(at(project.root.id)(ValidationProblem::MissingSetup));
    }
    let mut counts = IndexMap::new();
    for (id, definition) in project.reusable::<crate::FixtureDefinition>() {
        match definition.validate(id) {
            Ok(count) => {
                counts.insert(*id, count);
            }
            Err(error) => problems.push(at(*id)(ValidationProblem::Fixture(error))),
        }
    }
    for controller in project.objects::<crate::Controller>() {
        if let Err(error) = controller.validate() {
            problems.push(at(controller.id)(ValidationProblem::Controller(error)));
        }
    }
    for device in project.objects::<crate::InputDevice>() {
        if let Err(error) = device.validate() {
            problems.push(at(device.id)(ValidationProblem::InputDevice(error)));
        }
    }
    for layout in project.objects::<crate::Layout>() {
        if let Err(error) = layout.validate(project.reusable::<crate::FixtureDefinition>()) {
            problems.push(at(layout.id)(ValidationProblem::Layout(error)));
        }
    }
    if !problems.is_empty() {
        return problems;
    }
    for patch in project.objects::<crate::Patch>() {
        let mut found = Vec::new();
        validate_patch(project, patch, &counts, &mut found);
        problems.extend(found.into_iter().map(at(patch.id)));
    }
    for setup in project.objects::<crate::Setup>() {
        let mut found = Vec::new();
        if let Err(problem) = validate_setup(project, setup, &mut found) {
            found.push(problem);
        }
        problems.extend(found.into_iter().map(at(setup.id)));
    }
    for sequence in project.objects::<crate::Sequence>() {
        sequence_problems(project, sequence, None, &mut problems);
    }
    for console in project.objects::<crate::Console>() {
        let mut found = Vec::new();
        if let Err(problem) = console_problems(project, console, &mut found) {
            found.push(problem);
        }
        problems.extend(
            found
                .into_iter()
                .map(|problem| at(console.id)(ValidationProblem::Console(problem))),
        );
    }
    problems
}

/// The problems of one sequence, attached to it.
fn sequence_problems(
    project: &DonderProject,
    sequence: &Sequence,
    previous: Option<&Sequence>,
    problems: &mut Vec<ProjectValidationError>,
) {
    let mut found = Vec::new();
    if let Err(problem) = validate_sequence_since(project, sequence, previous, &mut found) {
        found.push(problem);
    }
    problems.extend(
        found
            .into_iter()
            .map(|problem| at(sequence.id)(ValidationProblem::Sequence(problem))),
    );
}

/// The first of `problems`, the one a single-problem check reports.
fn first<E>(problems: Vec<E>) -> Result<(), E> {
    problems.into_iter().next().map_or(Ok(()), Err)
}

/// Attach the object a problem belongs to.
pub(crate) fn at(
    object: impl Into<ObjectId>,
) -> impl Fn(ValidationProblem) -> ProjectValidationError {
    let object = object.into();
    move |problem| ProjectValidationError { object, problem }
}

/// The channels a route of a patch claims on a controller output.
struct Claim<'a> {
    channels: std::ops::Range<u32>,
    fixture: &'a Identifier,
}

/// A route that passed its own checks: the fixture it drives, its pixels and
/// its channel count.
struct CheckedRoute<'a> {
    fixture: Option<&'a Identifier>,
    pixels: std::ops::Range<u32>,
    width: u32,
}

/// Each route that breaks a rule, once.
fn validate_patch(
    project: &DonderProject,
    patch: &crate::patch::Patch,
    counts: &IndexMap<FixtureDefinitionId, u32>,
    problems: &mut Vec<ValidationProblem>,
) {
    let mut ids = HashSet::new();
    let mut checked = HashMap::new();
    let mut failed = HashMap::new();
    for route in &patch.routes {
        match check_route(project, route, counts, &mut ids) {
            Ok(route_check) => {
                checked.insert(route.id, route_check);
            }
            Err(problem) => {
                failed.insert(route.id, problem);
            }
        }
    }
    let placements = crate::patch::place_routes(
        &patch.routes,
        |id| project.get::<crate::Controller>(id),
        |route| {
            checked
                .get(&route.id)
                .map(|route: &CheckedRoute<'_>| route.width)
        },
    );
    let mut occupied = HashMap::new();
    for route in &patch.routes {
        let result = match (failed.remove(&route.id), checked.get(&route.id)) {
            (Some(problem), _) => Err(problem),
            (None, Some(route_check)) => {
                place_route(project, route, route_check, &placements, &mut occupied)
            }
            // A second route with a repeated id.
            (None, None) => Ok(()),
        };
        if let Err(problem) = result {
            problems.push(problem);
        }
    }
}

fn route_problem(
    route: &crate::patch::PixelRoute,
    fixture: Option<&Identifier>,
    problem: RouteProblem,
) -> ValidationProblem {
    ValidationProblem::Route {
        route: route.id,
        fixture: fixture.cloned(),
        problem,
    }
}

/// A route's own rules, apart from where its channels land.
fn check_route<'a>(
    project: &'a DonderProject,
    route: &crate::patch::PixelRoute,
    counts: &IndexMap<FixtureDefinitionId, u32>,
    ids: &mut HashSet<crate::patch::PixelRouteId>,
) -> Result<CheckedRoute<'a>, ValidationProblem> {
    let mut fixture = None;
    let fail = |fixture: Option<&Identifier>, problem| route_problem(route, fixture, problem);
    if !ids.insert(route.id) {
        return Err(fail(None, RouteProblem::DuplicateId));
    }
    let layout = project
        .get::<crate::Layout>(route.target.layout)
        .ok_or_else(|| fail(None, RouteProblem::MissingLayout))?;
    let target = layout.fixture(route.target.fixture);
    if let Some(target) = target {
        fixture = Some(&target.name);
    }
    if !matches!(
        target.map(|target| &target.kind),
        Some(crate::layout::LayoutFixtureKind::Fixture { .. })
    ) {
        return Err(fail(fixture, RouteProblem::TargetNotFixture));
    }
    if !route.encoding.is_valid() {
        return Err(fail(fixture, RouteProblem::InvalidColorOrder));
    }
    if !route.gamma.is_finite() || route.gamma <= 0.0 {
        return Err(fail(fixture, RouteProblem::InvalidGamma));
    }
    if !route.brightness.is_finite() || !(0.0..=1.0).contains(&route.brightness) {
        return Err(fail(fixture, RouteProblem::InvalidBrightness));
    }
    let target_count = layout
        .target_pixel_count(&route.target, counts)
        .map_err(|error| fail(fixture, RouteProblem::Target(error)))?;
    let pixels = match route.pixels {
        Some(span) if span.count == 0 => return Err(fail(fixture, RouteProblem::EmptySpan)),
        Some(span) => {
            if span
                .start
                .checked_add(span.count)
                .is_none_or(|end| end > target_count)
            {
                return Err(fail(
                    fixture,
                    RouteProblem::SpanExceedsTarget {
                        span,
                        pixels: target_count,
                    },
                ));
            }
            span.start..span.start + span.count
        }
        None => 0..target_count,
    };
    let controller = project
        .get::<crate::Controller>(route.controller)
        .ok_or_else(|| fail(fixture, RouteProblem::MissingController))?;
    if controller.port(route.port).is_none() {
        return Err(fail(fixture, RouteProblem::MissingPort));
    }
    let width = route
        .channel_count(target_count)
        .ok_or_else(|| fail(fixture, RouteProblem::ChannelOverflow))?;
    Ok(CheckedRoute {
        fixture,
        pixels,
        width,
    })
}

/// A checked route's channels, after the routes before it in its patch
/// claimed theirs in `occupied`. A route whose start follows a route that
/// broke a rule is not placed, and that route reports the problem.
fn place_route<'a>(
    project: &DonderProject,
    route: &crate::patch::PixelRoute,
    checked: &CheckedRoute<'a>,
    placements: &HashMap<crate::patch::PixelRouteId, crate::patch::RoutePlacement>,
    occupied: &mut HashMap<(crate::controller::ControllerId, ControllerOutput), Vec<Claim<'a>>>,
) -> Result<(), ValidationProblem> {
    let CheckedRoute {
        fixture,
        pixels,
        width,
    } = checked;
    let (fixture, width) = (*fixture, *width);
    let fail = |problem| route_problem(route, fixture, problem);
    let Some(placement) = placements.get(&route.id) else {
        return match route.start {
            crate::patch::RouteStart::Channel(start) if start.checked_add(width).is_none() => {
                Err(fail(RouteProblem::ChannelOverflow))
            }
            _ => Ok(()),
        };
    };
    let Some(controller) = project.get::<crate::Controller>(route.controller) else {
        return Err(fail(RouteProblem::MissingController));
    };
    let output = placement.output;
    let (start, end) = (placement.channels.start, placement.channels.end);
    let available = controller
        .output_width(output)
        .ok_or_else(|| fail(RouteProblem::MissingPort))?;
    let pixels = pixels.clone();
    if end > available {
        return Err(fail(RouteProblem::PastLastChannel {
            channels: start..end,
            available,
            output,
        }));
    }
    if width == 0 {
        return Ok(());
    }
    let claims = occupied.entry((route.controller, output)).or_default();
    if let Some(other) = claims
        .iter()
        .find(|claim| start < claim.channels.end && claim.channels.start < end)
    {
        let shared = start.max(other.channels.start)..end.min(other.channels.end);
        return Err(fail(RouteProblem::OutputsOverlap {
            pixels,
            other: other.fixture.clone(),
            place: controller
                .place(output, shared)
                .ok_or_else(|| fail(RouteProblem::MissingPort))?,
        }));
    }
    if let Some(fixture) = fixture {
        claims.push(Claim {
            channels: start..end,
            fixture,
        });
    }
    Ok(())
}

/// The name of the fixture a route drives, when its layout has it.
fn route_fixture(project: &DonderProject, route: &crate::patch::PixelRoute) -> Option<Identifier> {
    project
        .get::<crate::Layout>(route.target.layout)?
        .fixture(route.target.fixture)
        .map(|fixture| fixture.name.clone())
}

/// The setup's members exist and are distinct by ownership validation. A
/// missing member stops the checks; each other problem is pushed to
/// `problems`.
fn validate_setup(
    project: &DonderProject,
    setup: &crate::setup::Setup,
    problems: &mut Vec<ValidationProblem>,
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
            problems.push(ValidationProblem::Setup(SetupProblem::SharedOscPort(port)));
        }
    }
    for route in &patch.routes {
        if &route.target.layout != setup.layout.id() {
            problems.push(ValidationProblem::Setup(SetupProblem::RouteOnOtherLayout {
                route: route.id,
                fixture: route_fixture(project, route),
            }));
        } else if !controllers.contains(&route.controller) {
            problems.push(ValidationProblem::Setup(
                SetupProblem::RouteControllerInactive {
                    route: route.id,
                    fixture: route_fixture(project, route),
                },
            ));
        }
    }
    Ok(())
}

pub fn validate_sequence(
    project: &DonderProject,
    sequence: &Sequence,
) -> Result<(), SequenceValidationError> {
    let mut problems = Vec::new();
    validate_sequence_since(project, sequence, None, &mut problems)?;
    first(problems)
}

/// [`validate_sequence`], where `previous` is this sequence in a validated
/// project whose layouts and definitions are the same allocations as
/// `project`'s. A clip still shared with `previous` keeps its target and
/// parameter checks when the layers and mark collections are unchanged too.
/// A problem of the whole sequence stops the checks; each mark collection,
/// clip and automation clip that breaks a rule is pushed to `problems`.
fn validate_sequence_since(
    project: &DonderProject,
    sequence: &Sequence,
    previous: Option<&Sequence>,
    problems: &mut Vec<SequenceValidationError>,
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
            problems.push(Error::MarkOutsideDuration(name()));
        } else if collection.marks.iter().any(|mark| {
            mark.label.as_deref().is_some_and(|label| {
                label.is_empty() || label.trim() != label || label.contains(['\n', '\r'])
            })
        }) {
            problems.push(Error::InvalidMarkLabel(name()));
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
    let region = |clip: &crate::effect::Clip| {
        validate_timed_region(
            clip.start.0,
            clip.duration.0,
            sequence.duration.0,
            sampled_sequence_duration,
        )
        .map_err(ClipProblem::Region)
    };
    for clip in &sequence.clips {
        let check = || {
            if first_unchanged && is_unchanged(clip) {
                return region(clip);
            }
            if !layer_ids.contains(&clip.layer_id) {
                return Err(ClipProblem::MissingLayer);
            }
            region(clip)?;
            if active && &clip.target.layout != active_layout {
                return Err(ClipProblem::NotInActiveLayout);
            }
            if sequence
                .clips
                .first()
                .is_some_and(|first| first.target.layout != clip.target.layout)
            {
                return Err(ClipProblem::MixedLayouts);
            }
            if is_unchanged(clip) {
                return Ok(());
            }
            validate_target(project, &clip.target)?;
            validate_params(project, &clip.definition, &clip.param_overrides, &mark_keys)
        };
        if let Err(problem) = check() {
            problems.push(Error::Clip {
                clip: clip.id.clone(),
                name: clip.name.clone(),
                problem,
            });
        }
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
        let mut check = || {
            if sequence_layout.is_some_and(|layout| layout != &clip.row_target.layout) {
                return Err(AutomationProblem::MixedLayouts);
            }
            if active && &clip.row_target.layout != active_layout {
                return Err(AutomationProblem::NotInActiveLayout);
            }
            if project
                .get::<crate::Layout>(clip.row_target.layout)
                .and_then(|layout| layout.fixture(clip.row_target.fixture))
                .is_none()
            {
                return Err(AutomationProblem::MissingRow);
            }
            validate_timed_region(
                clip.start.0,
                clip.duration.0,
                sequence.duration.0,
                sampled_sequence_duration,
            )
            .map_err(AutomationProblem::Region)?;
            clip.curve
                .validate()
                .map_err(AutomationProblem::InvalidCurve)?;
            if !automation_curve_is_normalized(&clip.curve) {
                return Err(AutomationProblem::CurveOutOfRange);
            }
            let mut clip_targets = HashSet::new();
            for target in clip.targets() {
                if !clip_targets.insert(target) {
                    return Err(AutomationProblem::BindsTwice);
                }
                automation_targets
                    .entry(target)
                    .or_insert_with(Vec::new)
                    .push(clip);
            }
            for binding in &clip.bindings {
                if !binding.window.is_normalized() {
                    return Err(AutomationProblem::WindowOutOfRange);
                }
                let declaration = automation_target_param(project, sequence, &binding.target)?;
                if declaration.automation_mapping().is_none() {
                    return Err(AutomationProblem::NotAutomatable(declaration.name.clone()));
                }
            }
            Ok(())
        };
        if let Err(problem) = check() {
            problems.push(Error::Automation {
                clip: clip.id.clone(),
                problem,
            });
        }
    }
    for clips in automation_targets.values() {
        for (index, clip) in clips.iter().enumerate() {
            if let Some(other) = clips[index + 1..].iter().find(|other| clip.overlaps(other)) {
                problems.push(Error::Automation {
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
/// nodes are referred to by name, so each name is valid and unique. Edges name
/// a layer node `layers.layer`, so layers have their own namespace; operator
/// nodes share theirs with the root node `root`. Returns the layer ids.
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
    if layers.is_empty() {
        return Err(CompositionProblem::NoLayers.into());
    }
    if let Some(layer) = first_repeat(layers, |layer| layer.id.clone()) {
        return Err(CompositionProblem::DuplicateLayerId(layer.id.clone()).into());
    }
    let layer_names = layers
        .iter()
        .map(|layer| (Member::Layer(layer.id.clone()), &layer.name));
    let node_names =
        || operators().map(|(id, operator)| (Member::Node(id.clone()), &operator.name));
    snake_case(layer_names.clone().chain(node_names()))?;
    distinct(layer_names)?;
    let root = donder_language::object_name(donder_language::data::ROOT_NODE);
    let root_node = graph
        .nodes
        .iter()
        .find(|node| matches!(node.kind, CompositionGraphNodeKind::Root))
        .map(|node| (Member::Node(node.id.clone()), &root));
    distinct(root_node.into_iter().chain(node_names()))?;
    validate_composition_graph(graph, project.reusable::<crate::OperatorDefinition>())
        .map_err(CompositionProblem::Graph)?;
    for (node, operator) in operators() {
        let definition = project
            .get::<crate::OperatorDefinition>(operator.operator)
            .unwrap_or_else(|| unreachable!("the graph's operators were found"));
        let problem = |problem| CompositionProblem::Param {
            node: node.clone(),
            problem,
        };
        validate_overrides(definition.params(), &operator.params).map_err(problem)?;
        for (name, value) in &operator.params {
            validate_param_references(project, name, value, mark_keys).map_err(problem)?;
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
    validate_overrides(definition.params(), overrides).map_err(ClipProblem::Param)?;
    for (name, value) in overrides {
        validate_param_references(project, name, value, mark_keys).map_err(ClipProblem::Param)?;
    }
    Ok(())
}

/// Parameter overrides of a clip or operator node: each declared and typed by
/// its declaration, and every parameter without a default supplied.
fn validate_overrides(
    declarations: &[donder_language::compiler::ParamDecl],
    overrides: &IndexMap<Identifier, ParamValue>,
) -> Result<(), ParamProblem> {
    if let Some(name) = overrides
        .keys()
        .find(|name| !declarations.iter().any(|param| &param.name == *name))
    {
        return Err(ParamProblem::Undeclared(name.clone()));
    }
    for declaration in declarations {
        match overrides.get(&declaration.name) {
            Some(value) if !param_matches(value, declaration) => {
                return Err(ParamProblem::WrongType(declaration.name.clone()));
            }
            None if declaration.default.is_none() => {
                return Err(ParamProblem::MissingRequired(declaration.name.clone()));
            }
            _ => {}
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
    let mut problems = Vec::new();
    console_problems(project, console, &mut problems)?;
    first(problems)
}

/// [`validate_console`]: a problem of the whole console stops the checks; each
/// clip, `always` entry, event and played value that breaks a rule is pushed
/// to `problems`.
fn console_problems(
    project: &DonderProject,
    console: &Console,
    problems: &mut Vec<ConsoleValidationError>,
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
        if let Err(problem) = validate_console_clip(
            project,
            clip,
            &layer_ids,
            &mark_keys,
            active_layout,
            console_layout,
        ) {
            problems.push(Error::Clip {
                clip: clip.id,
                name: clip.name.clone(),
                problem,
            });
        }
    }
    let mut always = HashSet::new();
    for entry in &console.always {
        let Some(clip) = console.clip(entry.clip) else {
            problems.push(Error::MissingAlwaysClip);
            continue;
        };
        if !always.insert(entry.clip) {
            problems.push(Error::RepeatedAlwaysClip {
                clip: clip.id,
                name: clip.name.clone(),
            });
        } else if let Err(problem) = length(&entry.length) {
            problems.push(Error::AlwaysLength {
                clip: clip.id,
                name: clip.name.clone(),
                problem,
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
            problems.push(Error::PlayedAndHeld {
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
        if let Err(problem) = validate_event(project, console, on, &mark_keys, &control) {
            problems.push(Error::Event {
                event: on.id,
                problem,
            });
        }
    }
    for clip in &console.clips {
        validate_played(project, console, clip, &control, &mut |param, problem| {
            problems.push(Error::Played {
                clip: clip.id,
                name: clip.name.clone(),
                param,
                problem,
            });
        });
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
                .any(|action| !matches!(action, Action::Set { .. })) =>
        {
            return Err(EventProblem::AxisPlays);
        }
        _ => {}
    }
    for action in &on.actions {
        if let Some(id) = action.clip()
            && console.clip(id).is_none()
        {
            return Err(EventProblem::MissingClip);
        }
        match action {
            Action::Play(play) => {
                if !(1..=MAX_COPIES).contains(&play.copies) {
                    return Err(EventProblem::Copies(play.copies));
                }
                match &play.length {
                    PlayLength::Fixed(fixed) => length(fixed)?,
                    PlayLength::Scaled {
                        by,
                        shortest,
                        longest,
                    } => {
                        length(shortest)?;
                        length(longest)?;
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
            Action::Set { target, window } => {
                if !window.is_normalized() {
                    return Err(EventProblem::WindowOutOfRange(target.param().clone()));
                }
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
                // One control sets a parameter, by every event it answers.
                if console.events.iter().any(|other| {
                    other.event.control() != on.event.control()
                        && other
                            .actions
                            .iter()
                            .any(|action| matches!(action, Action::Set { target: set, .. } if set == target))
                }) {
                    return Err(EventProblem::SetByOtherControl(param.clone()));
                }
            }
            Action::Start { length: held, .. } | Action::Toggle { length: held, .. } => {
                length(held)?;
            }
            Action::Stop { .. } => {}
        }
    }
    Ok(())
}

/// A length a clip plays or repeats every: positive on the playback clock.
fn length(length: &DonderDuration) -> Result<(), EventProblem> {
    let sampled = sample_duration_from_donder_duration(length)
        .map_err(|_| EventProblem::DurationBeyondClock)?;
    if length.0.is_zero() || sampled.as_ticks() == 0 {
        return Err(EventProblem::NonPositiveDuration);
    }
    Ok(())
}

/// A clip's played values: each an automatable parameter of its effect, once,
/// that no `Set` writes, on a clip only presses start, and from a note only
/// when every control that starts it reads notes. Each value that breaks a
/// rule is reported once, to `report`.
fn validate_played<'a>(
    project: &'a DonderProject,
    console: &Console,
    clip: &ConsoleClip,
    control: &impl Fn(&ControlRef) -> Result<&'a InputControlKind, InputProblem>,
    report: &mut dyn FnMut(Identifier, PlayedProblem),
) {
    let starts = console
        .events
        .iter()
        .filter(|on| {
            on.actions
                .iter()
                .any(|action| action.started() == Some(clip.id))
        })
        .collect::<Vec<_>>();
    let pressed = !console.is_always(clip.id)
        && starts
            .iter()
            .all(|on| matches!(on.event, InputEvent::Press(_)));
    let notes = starts
        .iter()
        .all(|on| control(on.event.control()).is_ok_and(|input| input.notes().is_some()));
    let set = console.set_targets();
    let mut played = HashSet::new();
    for value in &clip.played {
        let mut check = || {
            match param_mapping(project, &clip.definition, &value.param) {
                None => return Err(PlayedProblem::UnknownParam),
                Some(None) => return Err(PlayedProblem::NotAutomatable),
                Some(Some(_)) => {}
            }
            if !played.insert(&value.param) {
                return Err(PlayedProblem::Twice);
            }
            if !value.window.is_normalized() {
                return Err(PlayedProblem::WindowOutOfRange);
            }
            if !pressed {
                return Err(PlayedProblem::WithoutPress);
            }
            if value.from.needs_note() && !notes {
                return Err(PlayedProblem::WithoutNotes);
            }
            let target = AutomationTarget::ClipParam {
                clip_id: clip.id,
                param: value.param.clone(),
            };
            if set.contains(&&target) {
                return Err(PlayedProblem::AlsoSet);
            }
            Ok(())
        };
        if let Err(problem) = check() {
            report(value.param.clone(), problem);
        }
    }
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
