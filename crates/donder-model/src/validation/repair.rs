//! What gives way when an edit leaves other members of the project breaking a
//! rule. A member the edit added or changed never gives way: when it breaks a
//! rule, the edit is refused. A member the edit left alone is removed, shrunk,
//! reset or cut loose from what it lost: each rule names how its member gives
//! way, and the repair happens at the rule's site.
use std::time::Duration;

use donder_language::{DonderDuration, DonderTime};
use donder_runtime_types::Identifier;

use super::{
    BindingProblem, ClipProblem, CompositionProblem, ConsoleValidationError, EnvelopeProblem,
    EventProblem, InputProblem, ParamProblem, PlayedProblem, ProjectValidationError, RegionProblem,
    RouteProblem, ValidationProblem, project_problems, target_param,
};
use crate::composition::CompositionGraphNodeKind;
use crate::console::{Action, Console};
use crate::effect::Clip;
use crate::effect::Placement;
use crate::identity::ObjectId;
use crate::layout::LayoutError;
use crate::patch::PixelSpan;
use crate::project::{DonderProject, MemberMut, MemberRef};
use crate::sequence::Sequence;
use crate::site::{Field, Member, Site};

/// A change to a member an edit left alone, so the edited project keeps every
/// rule.
#[derive(Clone, Debug, PartialEq)]
pub struct Repair {
    pub site: Site,
    pub action: GiveWay,
}

/// How a member gives way.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GiveWay {
    /// The member, or the part of it its field names, goes.
    Remove,
    /// A clip, envelope or mark collection is cut at the sequence's end; an
    /// output, to the pixels its fixture and controller still have.
    Shrink,
    /// A parameter value goes back to the parameter's default.
    Reset,
    /// What pointed at something the edit took away is forgotten.
    Forget,
}

impl Repair {
    /// Whether the person editing is asked first: every repair that removes
    /// or changes something they made, but not one that only forgets what
    /// pointed at something gone with the edit.
    pub fn needs_confirmation(&self) -> bool {
        self.action != GiveWay::Forget
    }
}

impl ValidationProblem {
    /// How the member that breaks the rule gives way; none when it cannot.
    pub(crate) fn give_way(&self) -> Option<GiveWay> {
        match self {
            Self::Route { problem, .. } => match problem {
                RouteProblem::DuplicateId
                | RouteProblem::InvalidColorOrder
                | RouteProblem::InvalidGamma
                | RouteProblem::InvalidBrightness
                | RouteProblem::EmptySpan
                | RouteProblem::ChannelOverflow => None,
                RouteProblem::TargetNotFixture
                | RouteProblem::Target(_)
                | RouteProblem::MissingController
                | RouteProblem::MissingPort
                | RouteProblem::OutputsOverlap { .. } => Some(GiveWay::Remove),
                RouteProblem::SpanExceedsTarget { .. } | RouteProblem::PastLastChannel { .. } => {
                    Some(GiveWay::Shrink)
                }
            },
            Self::Timing(_) => None,
            Self::Composition(problem) => problem.give_way(),
            Self::Console(problem) => match problem {
                ConsoleValidationError::DuplicateEventId(_)
                | ConsoleValidationError::PlayedAndHeld { .. }
                | ConsoleValidationError::RepeatedAlwaysClip { .. }
                | ConsoleValidationError::AlwaysLength { .. } => None,
                ConsoleValidationError::MissingAlwaysClip => Some(GiveWay::Forget),
                ConsoleValidationError::Event { problem, .. } => match problem {
                    EventProblem::Input(
                        InputProblem::MissingDevice
                        | InputProblem::NotAControl
                        | InputProblem::NotAButton
                        | InputProblem::NotAnAxis,
                    )
                    | EventProblem::SetByOtherControl(_) => Some(GiveWay::Remove),
                    EventProblem::AxisPlays
                    | EventProblem::Copies(_)
                    | EventProblem::DurationBeyondClock
                    | EventProblem::NonPositiveDuration
                    | EventProblem::ShortestAboveLongest
                    | EventProblem::WindowOutOfRange(_) => None,
                    EventProblem::MissingClip
                    | EventProblem::MissingNode
                    | EventProblem::MissingLiveMarks
                    | EventProblem::UnknownParam(_)
                    | EventProblem::NotAutomatable(_) => Some(GiveWay::Forget),
                },
            },
            Self::Layout(LayoutError::MissingDefinition(_)) => Some(GiveWay::Remove),
            Self::Curve(_)
            | Self::CurveNotNormalized
            | Self::Gradient(_)
            | Self::Fixture(_)
            | Self::Layout(_)
            | Self::Controller(_)
            | Self::InputDevice(_)
            | Self::Project(_)
            | Self::Admission(_) => None,
        }
    }
}

impl CompositionProblem {
    fn give_way(&self) -> Option<GiveWay> {
        match self {
            Self::Param { problem, .. } => problem.give_way(),
            Self::Clip { problem, .. } => problem.give_way(),
            Self::MarkOutsideDuration(..) => Some(GiveWay::Shrink),
            Self::Name(_) | Self::DuplicateClipId(_) | Self::InvalidMarkLabel(..) => None,
            Self::NoLayers
            | Self::DuplicateLayerId(_)
            | Self::Graph(_)
            | Self::MissingLayer(_)
            | Self::RepeatedLayer(_)
            | Self::LiveMarksHold(_) => None,
        }
    }
}

impl ClipProblem {
    fn give_way(&self) -> Option<GiveWay> {
        match self {
            Self::MissingLayer
            | Self::MissingTarget
            | Self::NoTargets
            | Self::RepeatedTarget
            | Self::Definition(_) => Some(GiveWay::Remove),
            Self::Region(region) => region.give_way(),
            Self::Param(problem) => problem.give_way(),
            Self::NeverPlays | Self::EnvelopePlayed | Self::PlayedTargets => None,
            Self::Envelope(problem) => match problem {
                EnvelopeProblem::InvalidCurve(_)
                | EnvelopeProblem::CurveOutOfRange
                | EnvelopeProblem::BindsTwice
                | EnvelopeProblem::Binding(_, BindingProblem::WindowOutOfRange) => None,
                EnvelopeProblem::Overlap
                | EnvelopeProblem::AlsoSet(_)
                | EnvelopeProblem::Binding(
                    _,
                    BindingProblem::NotAutomatable | BindingProblem::MissingParam,
                ) => Some(GiveWay::Remove),
                EnvelopeProblem::Binding(
                    _,
                    BindingProblem::MissingClip | BindingProblem::MissingNode,
                ) => Some(GiveWay::Forget),
            },
            Self::Played(_, problem) => match problem {
                PlayedProblem::UnknownParam | PlayedProblem::NotAutomatable => {
                    Some(GiveWay::Forget)
                }
                PlayedProblem::WithoutPress
                | PlayedProblem::WithoutNotes
                | PlayedProblem::AlsoSet => Some(GiveWay::Remove),
                PlayedProblem::Twice | PlayedProblem::WindowOutOfRange => None,
            },
        }
    }
}

impl RegionProblem {
    /// A region that runs past the sequence's end is cut there; one invalid
    /// by itself cannot give way.
    fn give_way(&self) -> Option<GiveWay> {
        match self {
            Self::BeyondSequence | Self::EndBeyondAfterRounding => Some(GiveWay::Shrink),
            Self::ZeroDuration
            | Self::Overflow
            | Self::StartBeyondClock
            | Self::DurationBeyondClock
            | Self::RoundsToZero => None,
        }
    }
}

impl ParamProblem {
    fn give_way(&self) -> Option<GiveWay> {
        match self {
            Self::WrongType(_)
            | Self::Undeclared(_)
            | Self::MissingMarks(_)
            | Self::MissingCurve(_)
            | Self::MissingGradient(_) => Some(GiveWay::Reset),
            Self::MissingRequired(_)
            | Self::InvalidCurve(..)
            | Self::InvalidGradient(..)
            | Self::InvalidText(_)
            | Self::InvalidFontHeight(_)
            | Self::InvalidImageSize(_) => None,
        }
    }
}

/// Repair `project`, an edit of the valid `previous`, until it keeps every
/// rule, and return the repairs. A problem no repair answers, or repairs that
/// change nothing, refuse the edit with that problem. Each repair removes or
/// shrinks a member, so the repairs end.
pub(crate) fn repair(
    project: &mut DonderProject,
    previous: &DonderProject,
) -> Result<Vec<Repair>, ProjectValidationError> {
    let mut repairs = Vec::new();
    loop {
        let problems = project_problems(project, Some(previous));
        let Some(first) = problems.first() else {
            return Ok(repairs);
        };
        let mut planned: Vec<(Repair, &ValidationProblem)> = Vec::new();
        for problem in &problems {
            let repair = plan(project, previous, problem).ok_or_else(|| problem.clone())?;
            if !planned.iter().any(|(other, _)| other == &repair) {
                planned.push((repair, &problem.problem));
            }
        }
        let count = repairs.len();
        for (repair, problem) in planned {
            if apply(project, &repair, problem) {
                repairs.push(repair);
            }
        }
        if repairs.len() == count {
            return Err(first.clone());
        }
    }
}

/// The repair for one problem, if a member the edit left alone can give way:
/// the one that breaks the rule, or the one it overlaps when the edit placed
/// that one.
fn plan(
    project: &DonderProject,
    previous: &DonderProject,
    error: &ProjectValidationError,
) -> Option<Repair> {
    let action = error.problem.give_way()?;
    let site = error.site();
    // Only a field that names a part of the member narrows the repair.
    let site = Site {
        field: site.field.filter(|field| {
            matches!(
                field,
                Field::Param(_)
                    | Field::Played(_)
                    | Field::Set(_)
                    | Field::Binding(_)
                    | Field::Always
            )
        }),
        ..site
    };
    let same = |site: &Site| unchanged(project, previous, site);
    if let ValidationProblem::Composition(CompositionProblem::Clip {
        problem: ClipProblem::Envelope(EnvelopeProblem::Overlap),
        ..
    }) = &error.problem
    {
        return unbind_overlap(project, &site, same);
    }
    if same(&site) {
        let action = match action {
            GiveWay::Shrink => match fit(project, &site, &error.problem)? {
                Fit::Nothing => GiveWay::Remove,
                _ => GiveWay::Shrink,
            },
            action => action,
        };
        return Some(Repair { site, action });
    }
    // The member the edit placed wins over the one already there.
    let other = |member| Site {
        member: Some(member),
        ..site.clone()
    };
    let other = match &error.problem {
        ValidationProblem::Route {
            route,
            problem: RouteProblem::OutputsOverlap { other: fixture, .. },
            ..
        } => {
            let routes = &project.root().routes;
            let placed = routes.iter().find(|candidate| candidate.id == *route)?;
            let controller = project.get::<crate::Controller>(placed.controller)?;
            let layout = project.layout();
            routes.iter().find_map(|candidate| {
                let site = other(Member::Route(candidate.id));
                (candidate.id != *route
                    && candidate.controller == placed.controller
                    && controller.port_output(candidate.port)
                        == controller.port_output(placed.port)
                    && layout
                        .fixture(candidate.target)
                        .is_some_and(|target| &target.name == fixture)
                    && same(&site))
                .then_some(site)
            })?
        }
        // Of an envelope and a `Set` that write one parameter, the one the
        // edit left alone lets go of it.
        ValidationProblem::Composition(CompositionProblem::Clip {
            problem: ClipProblem::Envelope(EnvelopeProblem::AlsoSet(target)),
            ..
        }) => {
            let ObjectId::Console(console) = error.object else {
                return None;
            };
            let console = project.get::<Console>(console)?;
            console.events.iter().find_map(|on| {
                let site = Site::member(error.object, Member::Event(on.id))
                    .at(Field::Set(target.param().clone()));
                (on.actions.iter().any(
                    |action| matches!(action, Action::Set { target: set, .. } if set == target),
                ) && same(&site))
                .then_some(site)
            })?
        }
        ValidationProblem::Console(ConsoleValidationError::Event {
            event,
            problem: EventProblem::SetByOtherControl(param),
        }) => {
            let ObjectId::Console(console) = error.object else {
                return None;
            };
            let console = project.get::<Console>(console)?;
            let placed = console.events.iter().find(|on| on.id == *event)?;
            console.events.iter().find_map(|candidate| {
                let site = other(Member::Event(candidate.id));
                (candidate.control != placed.control && sets(candidate, param) && same(&site))
                    .then_some(site)
            })?
        }
        _ => return None,
    };
    Some(Repair {
        site: other,
        action,
    })
}

/// Whether the member `site` names is in the edited project as it was before
/// the edit; at an object, whether the field it names is.
fn unchanged(project: &DonderProject, previous: &DonderProject, site: &Site) -> bool {
    if site.member.is_some() {
        let member = project.member(site);
        return member.is_some() && member == previous.member(site);
    }
    match (site.object, &site.field) {
        (ObjectId::Console(id), Some(Field::Always)) => {
            let always = |project: &DonderProject| {
                project
                    .get::<Console>(id)
                    .map(|console| console.always.clone())
            };
            let now = always(project);
            now.is_some() && now == always(previous)
        }
        _ => false,
    }
}

/// Of two envelopes that drive one parameter at once, the one the edit left
/// alone lets go of it.
fn unbind_overlap(
    project: &DonderProject,
    site: &Site,
    same: impl Fn(&Site) -> bool,
) -> Option<Repair> {
    let composition = match site.object {
        ObjectId::Sequence(id) => &project.get::<Sequence>(id)?.composition,
        ObjectId::Console(id) => &project.get::<Console>(id)?.composition,
        _ => return None,
    };
    let Some(MemberRef::Clip(value)) = project.member(site) else {
        return None;
    };
    let targets = |clip: &Clip| {
        clip.envelope()
            .map(|envelope| {
                envelope
                    .bindings
                    .iter()
                    .map(|binding| binding.target.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let at = |clip: &Clip| Site::member(site.object, Member::Clip(clip.id));
    let unchanged = same(site);
    composition.clips.iter().find_map(|other| {
        if other.id == value.id || other.envelope().is_none() || !other.overlaps(value) {
            return None;
        }
        let (giver, keeper) = if unchanged {
            (value, other.as_ref())
        } else if same(&at(other)) {
            (other.as_ref(), value)
        } else {
            return None;
        };
        let kept = targets(keeper);
        let target = targets(giver)
            .into_iter()
            .find(|target| kept.contains(target))?;
        Some(Repair {
            site: at(giver).at(Field::Binding(target.clone())),
            action: GiveWay::Remove,
        })
    })
}

/// Whether event `on` sets a parameter named `param`.
fn sets(on: &crate::console::On, param: &Identifier) -> bool {
    on.actions
        .iter()
        .any(|action| matches!(action, Action::Set { target, .. } if target.param() == param))
}

/// What a member shrinks to.
enum Fit {
    /// Nothing of it is left.
    Nothing,
    Duration(DonderDuration),
    Pixels(PixelSpan),
    /// Marks keep what lies inside the sequence.
    Marks,
}

/// What the member at `site` shrinks to for `problem`.
fn fit(project: &DonderProject, site: &Site, problem: &ValidationProblem) -> Option<Fit> {
    let rest = |start: &DonderTime| {
        let ObjectId::Sequence(id) = site.object else {
            return None;
        };
        Some(trim(start, &project.get::<Sequence>(id)?.duration))
    };
    match (problem, project.member(site)?) {
        (
            ValidationProblem::Composition(CompositionProblem::Clip { .. }),
            MemberRef::Clip(clip),
        ) => rest(clip.timing()?.0),
        (ValidationProblem::Composition(CompositionProblem::MarkOutsideDuration(..)), _) => {
            Some(Fit::Marks)
        }
        (ValidationProblem::Route { problem, .. }, MemberRef::Route(route)) => {
            let (start, count) = match problem {
                RouteProblem::SpanExceedsTarget { span, pixels } => {
                    (span.start, pixels.saturating_sub(span.start))
                }
                RouteProblem::PastLastChannel {
                    channels,
                    available,
                    ..
                } => {
                    let per_pixel = route.encoding.channel_count() as u32;
                    let span = route.pixels.unwrap_or(PixelSpan {
                        start: 0,
                        count: channels.len() as u32 / per_pixel,
                    });
                    (
                        span.start,
                        available.saturating_sub(channels.start) / per_pixel,
                    )
                }
                _ => return None,
            };
            Some(if count == 0 {
                Fit::Nothing
            } else {
                Fit::Pixels(PixelSpan { start, count })
            })
        }
        _ => None,
    }
}

/// The part of a region from `start` that ends inside a sequence of length
/// `sequence`.
fn trim(start: &DonderTime, sequence: &DonderDuration) -> Fit {
    let (Some(exact), Some(micros)) = (
        sequence.0.checked_sub(start.0),
        sequence
            .as_micros_rounded()
            .checked_sub(start.as_micros_rounded()),
    ) else {
        return Fit::Nothing;
    };
    // The shorter of the exact rest and the rest on the playback clock ends
    // inside the sequence both ways.
    let duration = DonderDuration(exact.min(Duration::from_micros(micros as u64)));
    if duration.as_micros_rounded() > 0 {
        Fit::Duration(duration)
    } else {
        Fit::Nothing
    }
}

/// Apply `repair`, planned for `problem`, to `project`; whether it changed
/// anything.
fn apply(project: &mut DonderProject, repair: &Repair, problem: &ValidationProblem) -> bool {
    let site = &repair.site;
    match repair.action {
        GiveWay::Remove => project.remove_at(site),
        GiveWay::Shrink => {
            let Some(fit) = fit(project, site, problem) else {
                return false;
            };
            let end = match site.object {
                ObjectId::Sequence(id) => project
                    .get::<Sequence>(id)
                    .map(|sequence| sequence.duration.0),
                _ => None,
            };
            match (fit, project.member_mut(site)) {
                (Fit::Duration(fit), Some(MemberMut::Clip(clip))) => match clip.timing() {
                    Some((start, duration)) if *duration != fit => {
                        let start = start.clone();
                        std::sync::Arc::make_mut(clip).crop(start, fit);
                        true
                    }
                    _ => false,
                },
                (Fit::Pixels(pixels), Some(MemberMut::Route(route))) => {
                    route.pixels != Some(pixels) && {
                        route.pixels = Some(pixels);
                        true
                    }
                }
                (Fit::Marks, Some(MemberMut::Marks(collection))) => end.is_some_and(|end| {
                    let mut changed = retain(&mut collection.marks, |mark| mark.time.0 <= end);
                    for mark in &mut collection.marks {
                        if mark.end() > end {
                            mark.duration.0 = end - mark.time.0;
                            changed = true;
                        }
                    }
                    changed
                }),
                _ => false,
            }
        }
        GiveWay::Reset => {
            let Some(Field::Param(param)) = &site.field else {
                return false;
            };
            match project.member_mut(site) {
                Some(MemberMut::Clip(clip)) => {
                    clip.effect()
                        .is_some_and(|effect| effect.params.contains_key(param))
                        && std::sync::Arc::make_mut(clip)
                            .effect_mut()
                            .is_some_and(|effect| effect.params.shift_remove(param).is_some())
                }
                Some(MemberMut::Node(node)) => match &mut node.kind {
                    CompositionGraphNodeKind::Operator(operator) => {
                        operator.params.shift_remove(param).is_some()
                    }
                    _ => false,
                },
                _ => false,
            }
        }
        GiveWay::Forget => forget(project, site),
    }
}

/// Forget what the member or field at `site` named that is gone.
fn forget(project: &mut DonderProject, site: &Site) -> bool {
    match (site.object, &site.member, &site.field) {
        (ObjectId::Console(id), None, Some(Field::Always)) => {
            project.find_mut::<Console>(id).is_some_and(|console| {
                let clips = console
                    .composition
                    .clips
                    .iter()
                    .filter(|clip| matches!(clip.placement, Placement::Played(_)))
                    .map(|clip| clip.id)
                    .collect::<Vec<_>>();
                retain(&mut console.always, |entry| clips.contains(&entry.clip))
            })
        }
        (_, Some(Member::Clip(_)), Some(Field::Played(_) | Field::Binding(_))) => {
            project.remove_at(site)
        }
        (ObjectId::Console(id), Some(Member::Event(_)), _) => {
            let (Some(console), Some(MemberRef::Event(on))) =
                (project.get::<Console>(id), project.member(site))
            else {
                return false;
            };
            let kept = on
                .actions
                .iter()
                .map(|action| resolves(project, console, action))
                .collect::<Vec<_>>();
            kept.contains(&false)
                && match project.member_mut(site) {
                    Some(MemberMut::Event(on)) => {
                        let mut kept = kept.into_iter();
                        on.actions.retain(|_| kept.next().unwrap_or(true));
                        true
                    }
                    _ => false,
                }
        }
        _ => false,
    }
}

/// Whether a console action names only clips, nodes, parameters and marks
/// that are there.
fn resolves(project: &DonderProject, console: &Console, action: &Action) -> bool {
    if let Some(clip) = action.clip()
        && console.clip(clip).is_none()
    {
        return false;
    }
    match action {
        Action::Mark(id) => console
            .composition
            .marks
            .iter()
            .any(|marks| marks.id == *id),
        Action::Set { target, .. } => {
            target_param(project, &console.composition.graph, target, |clip| {
                console
                    .clip(clip)
                    .and_then(crate::effect::Clip::effect)
                    .map(|effect| effect.definition)
            })
            .is_ok_and(|param| param.automation_mapping().is_some())
        }
        Action::Play(_) | Action::Start { .. } | Action::Stop { .. } | Action::Toggle { .. } => {
            true
        }
    }
}

/// Keep the items `keep` accepts; whether any went.
fn retain<T>(items: &mut Vec<T>, keep: impl FnMut(&T) -> bool) -> bool {
    let count = items.len();
    items.retain(keep);
    items.len() != count
}
