mod problem;
mod repair;

use std::collections::{HashMap, HashSet};

use crate::composition::{
    Composition, CompositionGraph, CompositionGraphNodeKind, MarkCollectionId,
};
use crate::console::{Action, CONSOLE_SESSION, Console, LengthSource, MAX_COPIES, On, PlayLength};
use crate::controller::ControllerOutput;
use crate::effect::{Clip, ClipContent, ClipId, CurveValue, GradientValue, ParamValue, Placement};
use crate::fixture::FixtureDefinitionId;
use crate::identity::ObjectId;
use crate::input_device::{ControlRef, InputControlKind};
use crate::operator::{param_matches, validate_composition_graph};
use crate::project::DonderProject;
use crate::sequence::{AutomationTarget, Sequence, curve_is_normalized};
use crate::site::Member;
use donder_language::compiler::{DeclarationKind, ParamDecl};
use donder_language::{
    DonderDuration, DonderTime, sample_duration_from_donder_duration, sample_time_from_donder_time,
};
use donder_runtime_types::{Identifier, InputHappening, SampleDuration};
use indexmap::IndexMap;

pub use problem::{
    AdmissionProblem, BindingProblem, ClipProblem, CompositionProblem, ConsoleValidationError,
    EnvelopeProblem, EventProblem, InputProblem, NameProblem, ParamProblem, PlayedProblem,
    ProjectProblem, ProjectValidationError, RegionProblem, RouteProblem, TimingProblem,
    ValidationProblem,
};
pub(crate) use repair::repair;
pub use repair::{GiveWay, Repair};

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
    if let Err(problem) = root_layout_exists(project) {
        problems.push(problem);
        return problems;
    }
    if let Some(previous) = previous
        && project.only_sequences_differ_from(previous)
    {
        for (sequence_id, sequence) in project.objects::<crate::Sequence>() {
            sequence_problems(
                project,
                sequence_id,
                sequence,
                previous.get::<crate::Sequence>(sequence_id),
                &mut problems,
            );
        }
        return problems;
    }
    for (id, definition) in project.reusable::<crate::CurveDefinition>() {
        if let Err(error) = definition.curve.validate() {
            problems.push(at(*id)(ValidationProblem::Curve(error)));
        } else if !curve_is_normalized(&definition.curve) {
            problems.push(at(*id)(ValidationProblem::CurveNotNormalized));
        }
    }
    for (id, definition) in project.reusable::<crate::GradientDefinition>() {
        if let Err(error) = definition.gradient.validate() {
            problems.push(at(*id)(ValidationProblem::Gradient(error)));
        }
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
    for (controller_id, controller) in project.objects::<crate::Controller>() {
        if let Err(error) = controller.validate() {
            problems.push(at(controller_id)(ValidationProblem::Controller(error)));
        }
    }
    for (device_id, device) in project.objects::<crate::InputDevice>() {
        if let Err(error) = device.validate() {
            problems.push(at(device_id)(ValidationProblem::InputDevice(error)));
        }
    }
    for (layout_id, layout) in project.objects::<crate::Layout>() {
        // Every fixture whose definition is gone, so each gives way at once.
        let missing = layout
            .fixtures
            .iter()
            .filter(|fixture| matches!(&fixture.kind, crate::LayoutFixtureKind::Fixture { definition, .. } if !project.reusable::<crate::FixtureDefinition>().contains_key(definition)))
            .map(|fixture| at(layout_id)(ValidationProblem::Layout(crate::LayoutError::MissingDefinition(fixture.id))))
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            problems.extend(missing);
            continue;
        }
        if let Err(error) = layout
            .validate(project.reusable::<crate::FixtureDefinition>())
            .and_then(|()| layout.validate_slices(&counts))
        {
            problems.push(at(layout_id)(ValidationProblem::Layout(error)));
        }
    }
    if !problems.is_empty() {
        return problems;
    }
    let mut found = Vec::new();
    validate_routes(project, &counts, &mut found);
    validate_root(project, &mut found);
    problems.extend(found.into_iter().map(at(project.root().id)));
    for (sequence_id, sequence) in project.objects::<crate::Sequence>() {
        sequence_problems(project, sequence_id, sequence, None, &mut problems);
    }
    for (console_id, console) in project.objects::<crate::Console>() {
        let mut found = Vec::new();
        if let Err(problem) = console_problems(project, console, &mut found) {
            found.push(problem);
        }
        problems.extend(found.into_iter().map(at(console_id)));
    }
    problems
}

/// The problems of one sequence, attached to it.
fn sequence_problems(
    project: &DonderProject,
    sequence_id: crate::SequenceId,
    sequence: &Sequence,
    previous: Option<&Sequence>,
    problems: &mut Vec<ProjectValidationError>,
) {
    let mut found = Vec::new();
    if let Err(problem) = validate_sequence_since(project, sequence, previous, &mut found) {
        found.push(problem);
    }
    problems.extend(found.into_iter().map(at(sequence_id)));
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

/// The channels a route claims on a controller output.
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
fn validate_routes(
    project: &DonderProject,
    counts: &IndexMap<FixtureDefinitionId, u32>,
    problems: &mut Vec<ValidationProblem>,
) {
    let mut ids = HashSet::new();
    let mut checked = HashMap::new();
    let mut failed = HashMap::new();
    let layout = project.layout();
    let routes = &project.root().routes;
    for route in routes {
        match check_route(project, &layout, route, counts, &mut ids) {
            Ok(route_check) => {
                checked.insert(route.id, route_check);
            }
            Err(problem) => {
                failed.insert(route.id, problem);
            }
        }
    }
    let placements = crate::patch::place_routes(
        routes,
        |id| project.get::<crate::Controller>(id),
        |route| {
            checked
                .get(&route.id)
                .map(|route: &CheckedRoute<'_>| route.width)
        },
    );
    let mut occupied = HashMap::new();
    for route in routes {
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
    project: &DonderProject,
    layout: &'a crate::Layout,
    route: &crate::patch::PixelRoute,
    counts: &IndexMap<FixtureDefinitionId, u32>,
    ids: &mut HashSet<crate::patch::PixelRouteId>,
) -> Result<CheckedRoute<'a>, ValidationProblem> {
    let mut fixture = None;
    let fail = |fixture: Option<&Identifier>, problem| route_problem(route, fixture, problem);
    if !ids.insert(route.id) {
        return Err(fail(None, RouteProblem::DuplicateId));
    }
    let target = layout.fixture(route.target);
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
        .target_pixel_count(route.target, counts)
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

/// A checked route's channels, after the routes before it
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
    // check_route found the controller and its port, whose output the
    // placement is on.
    let controller = project.referenced::<crate::Controller>(route.controller);
    let output = placement.output;
    let (start, end) = (placement.channels.start, placement.channels.end);
    let Some(available) = controller.output_width(output) else {
        unreachable!("a route is placed on its port's output");
    };
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
                .unwrap_or_else(|| unreachable!("overlapping channels are on the route's output")),
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

/// The root's layout exists: checks of other objects read it.
fn root_layout_exists(project: &DonderProject) -> Result<(), ProjectValidationError> {
    let root = project.root().id;
    if !project.layouts.contains_key(&project.root().layout) {
        return Err(at(root)(ValidationProblem::Project(
            ProjectProblem::MissingLayout,
        )));
    }
    Ok(())
}

/// The rules of the project's own members. Each problem is pushed to
/// `problems`.
fn validate_root(project: &DonderProject, problems: &mut Vec<ValidationProblem>) {
    if project.controllers.len() as u128 > u128::from(u32::MAX) + 1 {
        problems.push(ValidationProblem::Project(
            ProjectProblem::TooManyControllers,
        ));
    }
    let mut osc_ports = HashSet::new();
    for input_device in project.input_devices() {
        if let crate::InputDriver::Osc { port } = input_device.driver
            && !osc_ports.insert(port)
        {
            problems.push(ValidationProblem::Project(ProjectProblem::SharedOscPort(
                port,
            )));
        }
    }
}

pub fn validate_sequence(
    project: &DonderProject,
    sequence: &Sequence,
) -> Result<(), ValidationProblem> {
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
    problems: &mut Vec<ValidationProblem>,
) -> Result<(), ValidationProblem> {
    let timing = |problem| Err(ValidationProblem::Timing(problem));
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
    let mut found = Vec::new();
    validate_composition(
        project,
        &sequence.composition,
        Clock::Sequence {
            end: sequence.duration.0,
            sampled: sampled_sequence_duration,
        },
        previous.map(|previous| &previous.composition),
        &mut found,
    )
    .map_err(ValidationProblem::Composition)?;
    problems.extend(found.into_iter().map(ValidationProblem::Composition));
    Ok(())
}

/// The clock a composition plays on: a sequence's, which its marks and clips
/// end within and which plays only timed clips, or a console's session.
#[derive(Clone, Copy)]
enum Clock {
    Sequence {
        end: std::time::Duration,
        sampled: SampleDuration,
    },
    Session,
}

impl Clock {
    /// Where timed clips end by, exactly and on the playback clock.
    fn end(self) -> (std::time::Duration, SampleDuration) {
        match self {
            Self::Sequence { end, sampled } => (end, sampled),
            Self::Session => (
                std::time::Duration::from_micros(u64::from(CONSOLE_SESSION.as_ticks())),
                CONSOLE_SESSION,
            ),
        }
    }
}

/// The ids of a composition's mark collections.
fn mark_keys(composition: &Composition) -> HashSet<&MarkCollectionId> {
    composition.marks.iter().map(|marks| &marks.id).collect()
}

/// A sequence's or console's composition. Mark collections, clips, layers and
/// graph nodes are referred to by name, so each name is valid and unique.
/// Edges name a layer node `layers.layer`, so layers have their own
/// namespace; operator nodes share theirs with the root node `root`. A
/// sequence's marks end by its end; a console's start empty. A sequence plays
/// timed clips; a console also played ones. With `previous`, the composition
/// in a validated project whose layouts and definitions are the same
/// allocations, a clip still shared with it keeps its target and parameter
/// checks when the layers and mark collections are unchanged too. A problem of
/// the whole composition stops the checks; each mark collection and clip that
/// breaks a rule is pushed to `problems`.
fn validate_composition(
    project: &DonderProject,
    composition: &Composition,
    clock: Clock,
    previous: Option<&Composition>,
    problems: &mut Vec<CompositionProblem>,
) -> Result<(), CompositionProblem> {
    let Composition {
        marks,
        layers,
        graph,
        clips,
    } = composition;
    let unchanged = previous
        .filter(|previous| previous.layers == *layers && previous.marks == *marks)
        .map(|previous| {
            previous
                .clips
                .iter()
                .map(|clip| (clip.id, clip))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    let is_unchanged = |clip: &std::sync::Arc<Clip>| {
        unchanged
            .get(&clip.id)
            .is_some_and(|previous| std::sync::Arc::ptr_eq(previous, clip))
    };
    if let Some(clip) = first_repeat(clips, |clip| clip.id) {
        return Err(CompositionProblem::DuplicateClipId(clip.id));
    }
    let name = CompositionProblem::Name;
    let mark_names = marks
        .iter()
        .map(|collection| (Member::Marks(collection.id), &collection.name));
    let clip_names = clips.iter().map(|clip| (Member::Clip(clip.id), &clip.name));
    // An unchanged clip's name was accepted when it was admitted.
    snake_case(
        clips
            .iter()
            .filter(|clip| !is_unchanged(clip))
            .map(|clip| (Member::Clip(clip.id), &clip.name))
            .chain(mark_names.clone()),
    )
    .map_err(name)?;
    distinct(clip_names).map_err(name)?;
    distinct(mark_names).map_err(name)?;
    let mark_keys = mark_keys(composition);
    let operators = || {
        graph.nodes.iter().filter_map(|node| match &node.kind {
            CompositionGraphNodeKind::Operator(operator) => Some((&node.id, operator)),
            _ => None,
        })
    };
    if layers.is_empty() {
        return Err(CompositionProblem::NoLayers);
    }
    if let Some(layer) = first_repeat(layers, |layer| layer.id.clone()) {
        return Err(CompositionProblem::DuplicateLayerId(layer.id.clone()));
    }
    let layer_names = layers
        .iter()
        .map(|layer| (Member::Layer(layer.id.clone()), &layer.name));
    let node_names =
        || operators().map(|(id, operator)| (Member::Node(id.clone()), &operator.name));
    snake_case(layer_names.clone().chain(node_names())).map_err(name)?;
    distinct(layer_names).map_err(name)?;
    let root = donder_language::object_name(donder_language::data::ROOT_NODE);
    let root_node = graph
        .nodes
        .iter()
        .find(|node| matches!(node.kind, CompositionGraphNodeKind::Root))
        .map(|node| (Member::Node(node.id.clone()), &root));
    distinct(root_node.into_iter().chain(node_names())).map_err(name)?;
    validate_composition_graph(graph, project.reusable::<crate::ScriptDefinition>())
        .map_err(CompositionProblem::Graph)?;
    for (node, operator) in operators() {
        let definition = project
            .get::<crate::ScriptDefinition>(operator.operator)
            .unwrap_or_else(|| unreachable!("the graph's operators were found"));
        let problem = |problem| CompositionProblem::Param {
            node: node.clone(),
            problem,
        };
        validate_overrides(definition.params(), &operator.params).map_err(problem)?;
        for (name, value) in &operator.params {
            validate_param_references(project, name, value, &mark_keys).map_err(problem)?;
        }
    }
    let layer_ids = layers.iter().map(|layer| &layer.id).collect::<HashSet<_>>();
    let mut graph_layers = HashSet::new();
    for node in &graph.nodes {
        if let CompositionGraphNodeKind::Layer { layer_id } = &node.kind {
            if !layer_ids.contains(layer_id) {
                return Err(CompositionProblem::MissingLayer(node.id.clone()));
            }
            if !graph_layers.insert(layer_id) {
                return Err(CompositionProblem::RepeatedLayer(node.id.clone()));
            }
        }
    }
    for collection in marks {
        let name = || collection.name.clone();
        match clock {
            Clock::Session if !collection.marks.is_empty() => {
                problems.push(CompositionProblem::LiveMarksHold(collection.id));
            }
            Clock::Sequence { end, .. } if collection.marks.iter().any(|mark| mark.end() > end) => {
                problems.push(CompositionProblem::MarkOutsideDuration(
                    collection.id,
                    name(),
                ));
            }
            _ if collection.marks.iter().any(|mark| {
                mark.label.as_deref().is_some_and(|label| {
                    label.is_empty() || label.trim() != label || label.contains(['\n', '\r'])
                })
            }) =>
            {
                problems.push(CompositionProblem::InvalidMarkLabel(collection.id, name()));
            }
            _ => {}
        }
    }
    // An unchanged clip compared against an unchanged first clip in an unchanged
    // context only needs the checks that involve the clock's own end.
    let first_unchanged = clips.first().is_none_or(is_unchanged);
    let (end, sampled_end) = clock.end();
    let placement = |clip: &Clip| match (&clip.placement, clock) {
        (Placement::Timed { start, duration }, _) => {
            validate_timed_region(start.0, duration.0, end, sampled_end)
                .map_err(ClipProblem::Region)
        }
        (Placement::Played(_), Clock::Sequence { .. }) => Err(ClipProblem::NeverPlays),
        (Placement::Played(_), Clock::Session) if clip.envelope().is_some() => {
            Err(ClipProblem::EnvelopePlayed)
        }
        (Placement::Played(_), Clock::Session) => Ok(()),
    };
    for clip in clips {
        let check = || match &clip.content {
            ClipContent::Effect(effect) => {
                if first_unchanged && is_unchanged(clip) {
                    return placement(clip);
                }
                if !layer_ids.contains(&effect.layer) {
                    return Err(ClipProblem::MissingLayer);
                }
                placement(clip)?;
                if is_unchanged(clip) {
                    return Ok(());
                }
                validate_target(project, clip.target)?;
                validate_params(project, &effect.definition, &effect.params, &mark_keys)
            }
            // An envelope's checks read the clips and nodes it binds.
            ClipContent::Envelope(envelope) => {
                placement(clip)?;
                validate_target(project, clip.target)?;
                validate_envelope(project, composition, envelope).map_err(ClipProblem::Envelope)
            }
        };
        if let Err(problem) = check() {
            problems.push(CompositionProblem::Clip {
                clip: clip.id,
                name: clip.name.clone(),
                problem,
            });
        }
    }
    // Of two envelopes that drive one parameter at once, the later one breaks
    // the rule.
    let mut driven = HashMap::<_, Vec<&Clip>>::new();
    for clip in clips {
        for binding in clip
            .envelope()
            .into_iter()
            .flat_map(|envelope| &envelope.bindings)
        {
            driven.entry(&binding.target).or_default().push(clip);
        }
    }
    for envelopes in driven.values() {
        for (index, clip) in envelopes.iter().enumerate() {
            if let Some(other) = envelopes[index + 1..]
                .iter()
                .find(|other| clip.overlaps(other))
            {
                problems.push(CompositionProblem::Clip {
                    clip: other.id,
                    name: other.name.clone(),
                    problem: ClipProblem::Envelope(EnvelopeProblem::Overlap),
                });
            }
        }
    }
    Ok(())
}

/// An envelope's curve is valid and normalized, and each binding drives an
/// automatable parameter of a timed effect clip or operator of `composition`,
/// once, through a normalized window.
fn validate_envelope(
    project: &DonderProject,
    composition: &Composition,
    envelope: &crate::Envelope,
) -> Result<(), EnvelopeProblem> {
    envelope
        .curve
        .validate()
        .map_err(EnvelopeProblem::InvalidCurve)?;
    if !curve_is_normalized(&envelope.curve) {
        return Err(EnvelopeProblem::CurveOutOfRange);
    }
    let mut targets = HashSet::new();
    for binding in &envelope.bindings {
        if !targets.insert(&binding.target) {
            return Err(EnvelopeProblem::BindsTwice);
        }
        let problem = |problem| EnvelopeProblem::Binding(binding.target.clone(), problem);
        if !binding.window.is_normalized() {
            return Err(problem(BindingProblem::WindowOutOfRange));
        }
        let declaration =
            automation_target_param(project, composition, &binding.target).map_err(problem)?;
        if declaration.automation_mapping().is_none() {
            return Err(problem(BindingProblem::NotAutomatable));
        }
    }
    Ok(())
}

/// A clip's target names a fixture or group of the project's layout.
fn validate_target(
    project: &DonderProject,
    target: crate::layout::FixtureId,
) -> Result<(), ClipProblem> {
    if project.layout().fixture(target).is_none() {
        return Err(ClipProblem::MissingTarget);
    }
    Ok(())
}

/// A clip's parameter overrides: each declared, typed by its declaration,
/// and every parameter without a default supplied.
fn validate_params(
    project: &DonderProject,
    definition: &crate::effect::ScriptId,
    overrides: &IndexMap<Identifier, ParamValue>,
    mark_keys: &HashSet<&MarkCollectionId>,
) -> Result<(), ClipProblem> {
    let definition = project
        .script(*definition, DeclarationKind::Effect)
        .map_err(ClipProblem::Definition)?;
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

/// The automation mapping of parameter `name` of `definition`: `None` when
/// it has no such parameter, `Some(None)` when it cannot be automated.
fn param_mapping(
    project: &DonderProject,
    definition: &crate::effect::ScriptId,
    name: &Identifier,
) -> Option<Option<donder_runtime_types::AutomationMapping>> {
    project
        .get::<crate::ScriptDefinition>(*definition)
        .and_then(|definition| {
            definition
                .params()
                .iter()
                .find(|declaration| &declaration.name == name)
        })
        .map(ParamDecl::automation_mapping)
}

/// A console's problems: a problem of the whole console stops the checks; each
/// clip, `always` entry, event and played value that breaks a rule is pushed
/// to `problems`.
fn console_problems(
    project: &DonderProject,
    console: &Console,
    problems: &mut Vec<ValidationProblem>,
) -> Result<(), ValidationProblem> {
    use ConsoleValidationError as Error;
    if console.frame_rate == 0 {
        return Err(ValidationProblem::Timing(TimingProblem::ZeroFrameRate));
    }
    if console.frame_rate > MAX_SEQUENCE_FRAME_RATE {
        return Err(ValidationProblem::Timing(
            TimingProblem::FrameRateAboveLimit,
        ));
    }
    if let Some(on) = first_repeat(&console.events, |on| on.id) {
        return Err(ValidationProblem::Console(Error::DuplicateEventId(on.id)));
    }
    let mut found = Vec::new();
    validate_composition(
        project,
        &console.composition,
        Clock::Session,
        None,
        &mut found,
    )
    .map_err(ValidationProblem::Composition)?;
    let mark_keys = mark_keys(&console.composition);
    let mut problems_of_console = Vec::new();
    let problems_of_composition = problems;
    let problems = &mut problems_of_console;
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
    for clip in &console.composition.clips {
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
    let control = |input: &ControlRef| -> Result<&InputControlKind, InputProblem> {
        let device = project
            .get::<crate::InputDevice>(input.device)
            .ok_or(InputProblem::MissingDevice)?;
        let control = device
            .control(input.control)
            .ok_or(InputProblem::NotAControl)?;
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
    // A parameter follows an envelope or a control's `Set`, not both.
    let set = console.set_targets();
    for clip in &console.composition.clips {
        for binding in clip
            .envelope()
            .into_iter()
            .flat_map(|envelope| &envelope.bindings)
            .filter(|binding| set.contains(&&binding.target))
        {
            found.push(CompositionProblem::Clip {
                clip: clip.id,
                name: clip.name.clone(),
                problem: ClipProblem::Envelope(EnvelopeProblem::AlsoSet(binding.target.clone())),
            });
        }
    }
    for clip in &console.composition.clips {
        validate_played(project, console, clip, &control, &mut |param, problem| {
            found.push(CompositionProblem::Clip {
                clip: clip.id,
                name: clip.name.clone(),
                problem: ClipProblem::Played(param, problem),
            });
        });
    }
    problems_of_composition.extend(found.into_iter().map(ValidationProblem::Composition));
    problems_of_composition.extend(
        problems_of_console
            .into_iter()
            .map(ValidationProblem::Console),
    );
    Ok(())
}

fn validate_event<'a>(
    project: &'a DonderProject,
    console: &Console,
    on: &On,
    mark_keys: &HashSet<&MarkCollectionId>,
    control: &impl Fn(&ControlRef) -> Result<&'a InputControlKind, InputProblem>,
) -> Result<(), EventProblem> {
    let input = control(&on.control).map_err(EventProblem::Input)?;
    match on.happening {
        InputHappening::Press | InputHappening::Release if !input.is_button() => {
            return Err(EventProblem::Input(InputProblem::NotAButton));
        }
        InputHappening::Move if input.is_button() => {
            return Err(EventProblem::Input(InputProblem::NotAnAxis));
        }
        InputHappening::Move
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
                    return Err(EventProblem::MissingLiveMarks);
                }
            }
            Action::Set { target, window } => {
                if !window.is_normalized() {
                    return Err(EventProblem::WindowOutOfRange(target.param().clone()));
                }
                let declaration =
                    target_param(project, &console.composition.graph, target, |clip| {
                        console
                            .clip(clip)
                            .and_then(Clip::effect)
                            .map(|effect| effect.definition)
                    })
                    .map_err(|problem| match problem {
                        BindingProblem::MissingClip => EventProblem::MissingClip,
                        BindingProblem::MissingNode => EventProblem::MissingNode,
                        BindingProblem::MissingParam => {
                            EventProblem::UnknownParam(target.param().clone())
                        }
                        BindingProblem::WindowOutOfRange | BindingProblem::NotAutomatable => {
                            unreachable!("a target's parameter is missing or found")
                        }
                    })?;
                let param = target.param();
                if declaration.automation_mapping().is_none() {
                    return Err(EventProblem::NotAutomatable(param.clone()));
                }
                // One control sets a parameter, by every event it answers.
                if console.events.iter().any(|other| {
                    other.control != on.control
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
    clip: &Clip,
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
            .all(|on| on.happening == InputHappening::Press);
    let notes = starts
        .iter()
        .all(|on| control(&on.control).is_ok_and(|input| input.notes().is_some()));
    let set = console.set_targets();
    let mut played = HashSet::new();
    let Some(effect) = clip.effect() else {
        return;
    };
    for value in clip.played() {
        let mut check = || {
            match param_mapping(project, &effect.definition, &value.param) {
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
            if crate::console::needs_note(value.from) && !notes {
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
    mark_keys: &HashSet<&MarkCollectionId>,
) -> Result<(), ParamProblem> {
    let name = || param.clone();
    match value {
        ParamValue::Marks(Some(key)) if !mark_keys.contains(key) => {
            Err(ParamProblem::MissingMarks(name()))
        }
        ParamValue::Curve(CurveValue::Literal(curve)) => curve
            .validate()
            .map_err(|error| ParamProblem::InvalidCurve(name(), error)),
        ParamValue::Curve(CurveValue::Linked(id))
            if !project
                .reusable::<crate::CurveDefinition>()
                .contains_key(id) =>
        {
            Err(ParamProblem::MissingCurve(name()))
        }
        ParamValue::Gradient(GradientValue::Linked(id))
            if !project
                .reusable::<crate::GradientDefinition>()
                .contains_key(id) =>
        {
            Err(ParamProblem::MissingGradient(name()))
        }
        ParamValue::Gradient(GradientValue::Literal(gradient)) => gradient
            .validate()
            .map_err(|error| ParamProblem::InvalidGradient(name(), error)),
        ParamValue::Array(values) => {
            for value in values {
                validate_param_references(project, param, value, mark_keys)?;
            }
            Ok(())
        }
        ParamValue::Text(text)
            if text.chars().count() > donder_runtime_types::TEXT_LENGTH || text.contains(' ') =>
        {
            Err(ParamProblem::InvalidText(name()))
        }
        ParamValue::Font(Some(face))
            if !(1..=crate::FontFace::MAX_HEIGHT).contains(&face.height) =>
        {
            Err(ParamProblem::InvalidFontHeight(name()))
        }
        ParamValue::Image(Some(image))
            if !(1..=crate::ImageSource::MAX_SIZE).contains(&image.size) =>
        {
            Err(ParamProblem::InvalidImageSize(name()))
        }
        _ => Ok(()),
    }
}

/// The declaration an envelope's or `Set`'s target addresses: a parameter of
/// a timed effect clip or of an operator node of `composition`.
pub fn automation_target_param<'a>(
    project: &'a DonderProject,
    composition: &Composition,
    target: &AutomationTarget,
) -> Result<&'a ParamDecl, BindingProblem> {
    target_param(project, &composition.graph, target, |clip| {
        composition
            .clip(clip)
            .and_then(Clip::bindable)
            .map(|effect| effect.definition)
    })
}

/// The declaration a sequence's or console's automation target addresses;
/// `clip_definition` finds the effect of a clip it names.
fn target_param<'a>(
    project: &'a DonderProject,
    graph: &CompositionGraph,
    target: &AutomationTarget,
    clip_definition: impl FnOnce(ClipId) -> Option<crate::effect::ScriptId>,
) -> Result<&'a ParamDecl, BindingProblem> {
    let find = |params: &'a [ParamDecl], param: &Identifier| {
        params
            .iter()
            .find(|declaration| &declaration.name == param)
            .ok_or(BindingProblem::MissingParam)
    };
    match target {
        AutomationTarget::ClipParam { clip_id, param } => {
            let definition = clip_definition(*clip_id).ok_or(BindingProblem::MissingClip)?;
            let definition = project
                .get::<crate::ScriptDefinition>(definition)
                .ok_or(BindingProblem::MissingParam)?;
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
                .ok_or(BindingProblem::MissingNode)?;
            let definition = project
                .get::<crate::ScriptDefinition>(operator.operator)
                .ok_or(BindingProblem::MissingParam)?;
            find(definition.params(), param)
        }
    }
}
