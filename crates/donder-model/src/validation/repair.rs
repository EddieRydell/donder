//! What gives way when an edit leaves other members of the project breaking a
//! rule. A member the edit added or changed never gives way: when it breaks a
//! rule, the edit is refused. A member the edit left alone is removed, trimmed
//! or cut loose from what it lost, by one repair for each rule it can break.
use std::sync::Arc;
use std::time::Duration;

use donder_language::{DonderDuration, DonderTime};
use donder_runtime_types::Identifier;

use super::{
    AutomationProblem, ClipProblem, CompositionProblem, ConsoleValidationError, EventProblem,
    InputProblem, ParamProblem, PlayedProblem, ProjectValidationError, RegionProblem, RouteProblem,
    SequenceValidationError, SetupProblem, ValidationProblem, automation_target_param,
    project_problems, route_fixture, target_param,
};
use crate::console::{Action, Console, ConsoleClipId, OnId};
use crate::effect::ClipId;
use crate::identity::{ConsoleId, ObjectId, PatchId, SequenceId};
use crate::layout::{FixtureInstanceId, Layout, LayoutError, LayoutFixtureKind, LayoutId};
use crate::patch::{Patch, PixelRouteId, PixelSpan};
use crate::project::DonderProject;
use crate::sequence::{
    AutomationClipId, AutomationTarget, CompositionGraphNodeId, CompositionGraphNodeKind, Sequence,
    SequenceCompositionGraph,
};

/// A change to a member an edit left alone, so the edited project keeps every
/// rule.
#[derive(Clone, Debug, PartialEq)]
pub enum Repair {
    /// The fixtures of a layout whose definition is gone.
    RemoveLayoutItems {
        layout: LayoutId,
        items: Vec<FixtureInstanceId>,
    },
    /// A route whose target, controller or output is gone.
    RemoveRoute { patch: PatchId, route: PixelRouteId },
    /// A route cut to the pixels its target and output still have.
    ShortenRoute {
        patch: PatchId,
        route: PixelRouteId,
        pixels: PixelSpan,
    },
    /// A clip whose layer, target or effect is gone, or that starts after the
    /// sequence ends.
    RemoveClip { sequence: SequenceId, clip: ClipId },
    /// A clip cut at the sequence's end.
    TrimClip {
        sequence: SequenceId,
        clip: ClipId,
        duration: DonderDuration,
    },
    /// The marks of a collection past the sequence's end.
    TrimMarks {
        sequence: SequenceId,
        collection: Identifier,
    },
    /// An envelope whose row is gone, or that starts after the sequence ends.
    RemoveAutomationClip {
        sequence: SequenceId,
        clip: AutomationClipId,
    },
    /// An envelope cut at the sequence's end.
    TrimAutomationClip {
        sequence: SequenceId,
        clip: AutomationClipId,
        duration: DonderDuration,
    },
    /// An envelope's bindings to parameters that are gone: kept detached when
    /// their clip or node is still there to bind again, removed with it
    /// otherwise.
    DetachBindings {
        sequence: SequenceId,
        clip: AutomationClipId,
    },
    /// An envelope's binding to a parameter another envelope drives at once.
    Unbind {
        sequence: SequenceId,
        clip: AutomationClipId,
        target: AutomationTarget,
    },
    /// A parameter value that names something gone or no longer fits its
    /// parameter, back to the parameter's default.
    ResetParam {
        owner: ParamOwner,
        param: Identifier,
    },
    /// A console clip whose layer, target or effect is gone.
    RemoveConsoleClip {
        console: ConsoleId,
        clip: ConsoleClipId,
    },
    /// The `always` entries of clips that are gone.
    ForgetAlways { console: ConsoleId },
    /// An event whose control is gone or no longer fits it.
    RemoveEvent { console: ConsoleId, event: OnId },
    /// An event's actions on clips, nodes, parameters and marks that are gone.
    ForgetActions { console: ConsoleId, event: OnId },
    /// An event's `Set` of a parameter another control sets.
    RemoveSet {
        console: ConsoleId,
        event: OnId,
        param: Identifier,
    },
    /// A played value that its clip's events or a `Set` no longer allow.
    DropPlayed {
        console: ConsoleId,
        clip: ConsoleClipId,
        param: Identifier,
    },
    /// A played value of a parameter that is gone.
    ForgetPlayed {
        console: ConsoleId,
        clip: ConsoleClipId,
        param: Identifier,
    },
}

/// The holder of a parameter value.
#[derive(Clone, Debug, PartialEq)]
pub enum ParamOwner {
    Clip {
        sequence: SequenceId,
        clip: ClipId,
    },
    ConsoleClip {
        console: ConsoleId,
        clip: ConsoleClipId,
    },
    Node {
        graph: GraphOwner,
        node: CompositionGraphNodeId,
    },
}

/// The sequence or console a composition graph belongs to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GraphOwner {
    Sequence(SequenceId),
    Console(ConsoleId),
}

impl Repair {
    /// Whether the person editing is asked first: every repair that removes
    /// or changes something they made, but not one that only forgets what
    /// pointed at something gone with the edit.
    pub fn needs_confirmation(&self) -> bool {
        match self {
            Self::RemoveLayoutItems { .. }
            | Self::RemoveRoute { .. }
            | Self::ShortenRoute { .. }
            | Self::RemoveClip { .. }
            | Self::TrimClip { .. }
            | Self::TrimMarks { .. }
            | Self::RemoveAutomationClip { .. }
            | Self::TrimAutomationClip { .. }
            | Self::Unbind { .. }
            | Self::ResetParam { .. }
            | Self::RemoveConsoleClip { .. }
            | Self::RemoveEvent { .. }
            | Self::RemoveSet { .. }
            | Self::DropPlayed { .. } => true,
            Self::DetachBindings { .. }
            | Self::ForgetAlways { .. }
            | Self::ForgetActions { .. }
            | Self::ForgetPlayed { .. } => false,
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
        let mut planned = Vec::new();
        for problem in &problems {
            let repair = plan(project, previous, problem).ok_or_else(|| problem.clone())?;
            if !planned.contains(&repair) {
                planned.push(repair);
            }
        }
        let first = first.clone();
        let count = repairs.len();
        for repair in planned {
            if apply(project, &repair) {
                repairs.push(repair);
            }
        }
        if repairs.len() == count {
            return Err(first);
        }
    }
}

/// The repair for one problem, if the member that breaks the rule can give
/// way.
fn plan(
    project: &DonderProject,
    previous: &DonderProject,
    error: &ProjectValidationError,
) -> Option<Repair> {
    let state = State { project, previous };
    match &error.problem {
        ValidationProblem::Route { route, problem, .. } => {
            let ObjectId::Patch(patch) = error.object else {
                return None;
            };
            state.route(patch, *route, problem)
        }
        ValidationProblem::Setup(problem) => {
            let ObjectId::Setup(setup) = error.object else {
                return None;
            };
            match problem {
                SetupProblem::RouteControllerInactive { route, .. } => {
                    let patch = *project.get::<crate::Setup>(setup)?.patch.id();
                    state
                        .route_unchanged(patch, *route)
                        .then_some(Repair::RemoveRoute {
                            patch,
                            route: *route,
                        })
                }
                SetupProblem::TooManyControllers
                | SetupProblem::SharedOscPort(_)
                | SetupProblem::RouteOnOtherLayout { .. } => None,
            }
        }
        ValidationProblem::Sequence(problem) => {
            let ObjectId::Sequence(sequence) = error.object else {
                return None;
            };
            state.sequence(sequence, problem)
        }
        ValidationProblem::Console(problem) => {
            let ObjectId::Console(console) = error.object else {
                return None;
            };
            state.console(console, problem)
        }
        ValidationProblem::Layout(LayoutError::MissingDefinition(definition)) => {
            let ObjectId::Layout(id) = error.object else {
                return None;
            };
            let layout = project.get::<Layout>(id)?;
            if !same(Some(layout), previous.get::<Layout>(id)) {
                return None;
            }
            let items = layout
                .fixtures
                .iter()
                .filter(|fixture| {
                    matches!(&fixture.kind, LayoutFixtureKind::Fixture { definition: crate::ownership::Owned::Reference(owned), .. } if owned == definition)
                })
                .map(|fixture| fixture.id)
                .collect::<Vec<_>>();
            (!items.is_empty()).then_some(Repair::RemoveLayoutItems { layout: id, items })
        }
        ValidationProblem::MissingSetup
        | ValidationProblem::MissingLayout
        | ValidationProblem::MissingPatch
        | ValidationProblem::MissingController
        | ValidationProblem::MissingInputDevice
        | ValidationProblem::Ownership(_)
        | ValidationProblem::DefinitionMismatch
        | ValidationProblem::Curve(_)
        | ValidationProblem::CurveNotNormalized
        | ValidationProblem::Gradient(_)
        | ValidationProblem::Fixture(_)
        | ValidationProblem::Layout(_)
        | ValidationProblem::Controller(_)
        | ValidationProblem::InputDevice(_)
        | ValidationProblem::Admission(_) => None,
    }
}

/// The edited project and the valid one it was edited from.
struct State<'a> {
    project: &'a DonderProject,
    previous: &'a DonderProject,
}

/// A lookup of a member in a project or object, typed so it serves both the
/// edited project and the one before.
fn finder<C, T>(find: impl Fn(&C) -> Option<&T>) -> impl Fn(&C) -> Option<&T> {
    find
}

/// Whether a member is in the edited project as it was before the edit.
fn same<T: PartialEq>(member: Option<&T>, before: Option<&T>) -> bool {
    member.is_some() && member == before
}

impl State<'_> {
    fn route_unchanged(&self, patch: PatchId, route: PixelRouteId) -> bool {
        let find = finder(|project: &DonderProject| {
            project
                .get::<Patch>(patch)
                .and_then(|patch| patch.routes.iter().find(|candidate| candidate.id == route))
        });
        same(find(self.project), find(self.previous))
    }

    fn route(&self, patch: PatchId, id: PixelRouteId, problem: &RouteProblem) -> Option<Repair> {
        let unchanged = self.route_unchanged(patch, id);
        let route = self
            .project
            .get::<Patch>(patch)?
            .routes
            .iter()
            .find(|route| route.id == id)?;
        let remove = Repair::RemoveRoute { patch, route: id };
        let shorten = |start: u32, count: u32| {
            Some(if count == 0 {
                remove.clone()
            } else {
                Repair::ShortenRoute {
                    patch,
                    route: id,
                    pixels: PixelSpan { start, count },
                }
            })
        };
        match problem {
            RouteProblem::DuplicateId
            | RouteProblem::InvalidColorOrder
            | RouteProblem::InvalidGamma
            | RouteProblem::InvalidBrightness
            | RouteProblem::EmptySpan
            | RouteProblem::ChannelOverflow => None,
            RouteProblem::MissingLayout
            | RouteProblem::TargetNotFixture
            | RouteProblem::Target(_)
            | RouteProblem::MissingController
            | RouteProblem::MissingPort => unchanged.then_some(remove),
            RouteProblem::SpanExceedsTarget { span, pixels } => {
                if !unchanged {
                    return None;
                }
                shorten(span.start, pixels.saturating_sub(span.start))
            }
            RouteProblem::PastLastChannel {
                channels,
                available,
                ..
            } => {
                if !unchanged {
                    return None;
                }
                let per_pixel = route.encoding.channel_count() as u32;
                let span = route.pixels.unwrap_or(PixelSpan {
                    start: 0,
                    count: channels.len() as u32 / per_pixel,
                });
                shorten(
                    span.start,
                    available.saturating_sub(channels.start) / per_pixel,
                )
            }
            RouteProblem::OutputsOverlap { other, .. } => {
                if unchanged {
                    return Some(remove);
                }
                // The route the edit placed wins over the one already there.
                let patch_value = self.project.get::<Patch>(patch)?;
                let controller = self.project.get::<crate::Controller>(route.controller)?;
                let same_stream = |candidate: &crate::patch::PixelRoute| {
                    controller.port_output(candidate.port) == controller.port_output(route.port)
                };
                let earlier = patch_value.routes.iter().find(|candidate| {
                    candidate.id != id
                        && candidate.controller == route.controller
                        && same_stream(candidate)
                        && route_fixture(self.project, candidate).as_ref() == Some(other)
                        && self.route_unchanged(patch, candidate.id)
                })?;
                Some(Repair::RemoveRoute {
                    patch,
                    route: earlier.id,
                })
            }
        }
    }

    fn sequence(&self, id: SequenceId, problem: &SequenceValidationError) -> Option<Repair> {
        let sequence = self.project.get::<Sequence>(id)?;
        let before = self.previous.get::<Sequence>(id);
        match problem {
            SequenceValidationError::MissingActiveSetup
            | SequenceValidationError::Timing(_)
            | SequenceValidationError::Name(_)
            | SequenceValidationError::DuplicateClipId(_)
            | SequenceValidationError::DuplicateAutomationClipId(_)
            | SequenceValidationError::InvalidMarkLabel(_) => None,
            SequenceValidationError::Composition(problem) => self.composition(
                GraphOwner::Sequence(id),
                &sequence.composition_graph,
                before.map(|before| &before.composition_graph),
                problem,
            ),
            SequenceValidationError::MarkOutsideDuration(name) => {
                let find = finder(|sequence: &Sequence| {
                    sequence
                        .mark_collections
                        .iter()
                        .find(|collection| &collection.key.name == name)
                });
                same(find(sequence), before.and_then(find)).then(|| Repair::TrimMarks {
                    sequence: id,
                    collection: name.clone(),
                })
            }
            SequenceValidationError::Clip { clip, problem, .. } => {
                let find = finder(|sequence: &Sequence| {
                    sequence
                        .clips
                        .iter()
                        .find(|candidate| &candidate.id == clip)
                        .map(Arc::as_ref)
                });
                let value = find(sequence)?;
                if !same(Some(value), before.and_then(find)) {
                    return None;
                }
                let remove = Repair::RemoveClip {
                    sequence: id,
                    clip: clip.clone(),
                };
                match problem {
                    ClipProblem::MissingLayer
                    | ClipProblem::MissingTargetLayout
                    | ClipProblem::MissingTarget
                    | ClipProblem::MissingDefinition => Some(remove),
                    ClipProblem::NotInActiveLayout | ClipProblem::MixedLayouts => None,
                    ClipProblem::Region(region) => trim(region, &value.start, &sequence.duration)
                        .map(|duration| match duration {
                            Some(duration) => Repair::TrimClip {
                                sequence: id,
                                clip: clip.clone(),
                                duration,
                            },
                            None => remove,
                        }),
                    ClipProblem::Param(problem) => param(
                        ParamOwner::Clip {
                            sequence: id,
                            clip: clip.clone(),
                        },
                        problem,
                    ),
                }
            }
            SequenceValidationError::Automation { clip, problem } => {
                let find = finder(|sequence: &Sequence| {
                    sequence
                        .automation_clips
                        .iter()
                        .find(|candidate| &candidate.id == clip)
                });
                let value = find(sequence)?;
                let unchanged = same(Some(value), before.and_then(find));
                let remove = Repair::RemoveAutomationClip {
                    sequence: id,
                    clip: clip.clone(),
                };
                match problem {
                    AutomationProblem::MixedLayouts
                    | AutomationProblem::NotInActiveLayout
                    | AutomationProblem::InvalidCurve(_)
                    | AutomationProblem::CurveOutOfRange
                    | AutomationProblem::WindowOutOfRange
                    | AutomationProblem::BindsTwice => None,
                    AutomationProblem::MissingRow => unchanged.then_some(remove),
                    AutomationProblem::Region(region) => {
                        if !unchanged {
                            return None;
                        }
                        trim(region, &value.start, &sequence.duration).map(
                            |duration| match duration {
                                Some(duration) => Repair::TrimAutomationClip {
                                    sequence: id,
                                    clip: clip.clone(),
                                    duration,
                                },
                                None => remove,
                            },
                        )
                    }
                    AutomationProblem::NotAutomatable(_)
                    | AutomationProblem::MissingClip
                    | AutomationProblem::MissingNode
                    | AutomationProblem::MissingParam(_) => {
                        unchanged.then(|| Repair::DetachBindings {
                            sequence: id,
                            clip: clip.clone(),
                        })
                    }
                    AutomationProblem::Overlap => {
                        // The envelope the edit placed keeps the parameter.
                        let unchanged_clip = |candidate: &crate::sequence::AutomationClip| {
                            same(
                                Some(candidate),
                                before.and_then(|before| {
                                    before
                                        .automation_clips
                                        .iter()
                                        .find(|other| other.id == candidate.id)
                                }),
                            )
                        };
                        sequence.automation_clips.iter().find_map(|other| {
                            if other.id == value.id || !other.overlaps(value) {
                                return None;
                            }
                            let (giver, keeper) = if unchanged {
                                (value, other)
                            } else if unchanged_clip(other) {
                                (other, value)
                            } else {
                                return None;
                            };
                            let target = giver
                                .targets()
                                .find(|target| keeper.targets().any(|kept| kept == *target))?;
                            Some(Repair::Unbind {
                                sequence: id,
                                clip: giver.id.clone(),
                                target: target.clone(),
                            })
                        })
                    }
                }
            }
        }
    }

    fn composition(
        &self,
        graph: GraphOwner,
        current: &SequenceCompositionGraph,
        before: Option<&SequenceCompositionGraph>,
        problem: &CompositionProblem,
    ) -> Option<Repair> {
        match problem {
            CompositionProblem::Param { node, problem } => {
                let find = finder(|graph: &SequenceCompositionGraph| {
                    graph.nodes.iter().find(|candidate| &candidate.id == node)
                });
                if !same(find(current), before.and_then(find)) {
                    return None;
                }
                param(
                    ParamOwner::Node {
                        graph,
                        node: node.clone(),
                    },
                    problem,
                )
            }
            CompositionProblem::NoLayers
            | CompositionProblem::DuplicateLayerId(_)
            | CompositionProblem::Graph(_)
            | CompositionProblem::MissingLayer(_)
            | CompositionProblem::RepeatedLayer(_) => None,
        }
    }

    fn console(&self, id: ConsoleId, problem: &ConsoleValidationError) -> Option<Repair> {
        let console = self.project.get::<Console>(id)?;
        let before = self.previous.get::<Console>(id);
        match problem {
            ConsoleValidationError::MissingActiveSetup
            | ConsoleValidationError::Timing(_)
            | ConsoleValidationError::Name(_)
            | ConsoleValidationError::DuplicateClipId(_)
            | ConsoleValidationError::DuplicateEventId(_)
            | ConsoleValidationError::PlayedAndHeld { .. }
            | ConsoleValidationError::RepeatedAlwaysClip { .. }
            | ConsoleValidationError::AlwaysLength { .. } => None,
            ConsoleValidationError::Composition(problem) => self.composition(
                GraphOwner::Console(id),
                &console.composition_graph,
                before.map(|before| &before.composition_graph),
                problem,
            ),
            ConsoleValidationError::Clip { clip, problem, .. } => {
                let find = finder(|console: &Console| console.clip(*clip));
                if !same(find(console), before.and_then(find)) {
                    return None;
                }
                match problem {
                    ClipProblem::MissingLayer
                    | ClipProblem::MissingTargetLayout
                    | ClipProblem::MissingTarget
                    | ClipProblem::MissingDefinition => Some(Repair::RemoveConsoleClip {
                        console: id,
                        clip: *clip,
                    }),
                    ClipProblem::NotInActiveLayout
                    | ClipProblem::MixedLayouts
                    | ClipProblem::Region(_) => None,
                    ClipProblem::Param(problem) => param(
                        ParamOwner::ConsoleClip {
                            console: id,
                            clip: *clip,
                        },
                        problem,
                    ),
                }
            }
            ConsoleValidationError::MissingAlwaysClip => {
                same(Some(&console.always), before.map(|before| &before.always))
                    .then_some(Repair::ForgetAlways { console: id })
            }
            ConsoleValidationError::Event { event, problem } => {
                let find =
                    finder(|console: &Console| console.events.iter().find(|on| on.id == *event));
                let value = find(console)?;
                let unchanged = same(Some(value), before.and_then(find));
                match problem {
                    EventProblem::Input(
                        InputProblem::MissingDevice
                        | InputProblem::NotAControl
                        | InputProblem::NotInSetup
                        | InputProblem::NotAButton
                        | InputProblem::NotAnAxis,
                    ) => unchanged.then_some(Repair::RemoveEvent {
                        console: id,
                        event: *event,
                    }),
                    EventProblem::AxisPlays
                    | EventProblem::Copies(_)
                    | EventProblem::DurationBeyondClock
                    | EventProblem::NonPositiveDuration
                    | EventProblem::ShortestAboveLongest
                    | EventProblem::WindowOutOfRange(_) => None,
                    EventProblem::MissingClip
                    | EventProblem::MissingNode
                    | EventProblem::MissingLiveMarks(_)
                    | EventProblem::UnknownParam(_)
                    | EventProblem::NotAutomatable(_) => {
                        unchanged.then_some(Repair::ForgetActions {
                            console: id,
                            event: *event,
                        })
                    }
                    EventProblem::SetByOtherControl(param) => {
                        if unchanged {
                            return Some(Repair::RemoveSet {
                                console: id,
                                event: *event,
                                param: param.clone(),
                            });
                        }
                        // The control the edit chose keeps the parameter.
                        let other = console.events.iter().find(|other| {
                            other.event.control() != value.event.control()
                                && sets(other, param)
                                && same(
                                    Some(*other),
                                    before.and_then(|before| {
                                        before.events.iter().find(|on| on.id == other.id)
                                    }),
                                )
                        })?;
                        Some(Repair::RemoveSet {
                            console: id,
                            event: other.id,
                            param: param.clone(),
                        })
                    }
                }
            }
            ConsoleValidationError::Played {
                clip,
                param,
                problem,
                ..
            } => {
                let find = finder(|console: &Console| console.clip(*clip));
                if !same(find(console), before.and_then(find)) {
                    return None;
                }
                let (console, clip, param) = (id, *clip, param.clone());
                match problem {
                    PlayedProblem::UnknownParam | PlayedProblem::NotAutomatable => {
                        Some(Repair::ForgetPlayed {
                            console,
                            clip,
                            param,
                        })
                    }
                    PlayedProblem::WithoutPress
                    | PlayedProblem::WithoutNotes
                    | PlayedProblem::AlsoSet => Some(Repair::DropPlayed {
                        console,
                        clip,
                        param,
                    }),
                    PlayedProblem::Twice | PlayedProblem::WindowOutOfRange => None,
                }
            }
        }
    }
}

/// Whether event `on` sets a parameter named `param`.
fn sets(on: &crate::console::On, param: &Identifier) -> bool {
    on.actions
        .iter()
        .any(|action| matches!(action, Action::Set { target, .. } if target.param() == param))
}

fn param(owner: ParamOwner, problem: &ParamProblem) -> Option<Repair> {
    match problem {
        ParamProblem::WrongType(param)
        | ParamProblem::Undeclared(param)
        | ParamProblem::MissingMarks(param)
        | ParamProblem::MissingCurve(param)
        | ParamProblem::MissingGradient(param) => Some(Repair::ResetParam {
            owner,
            param: param.clone(),
        }),
        ParamProblem::MissingRequired(_)
        | ParamProblem::InvalidCurve(..)
        | ParamProblem::InvalidGradient(..)
        | ParamProblem::InvalidText(_)
        | ParamProblem::InvalidFontHeight(_)
        | ParamProblem::InvalidImageSize(_) => None,
    }
}

/// For a region that runs past the sequence's end: `Some(Some(duration))` to
/// cut it there, `Some(None)` when nothing of it is left, and `None` for a
/// region that is invalid by itself.
fn trim(
    region: &RegionProblem,
    start: &DonderTime,
    sequence: &DonderDuration,
) -> Option<Option<DonderDuration>> {
    match region {
        RegionProblem::BeyondSequence | RegionProblem::EndBeyondAfterRounding => {}
        RegionProblem::ZeroDuration
        | RegionProblem::Overflow
        | RegionProblem::StartBeyondClock
        | RegionProblem::DurationBeyondClock
        | RegionProblem::RoundsToZero => return None,
    }
    let (Some(exact), Some(micros)) = (
        sequence.0.checked_sub(start.0),
        sequence
            .as_micros_rounded()
            .checked_sub(start.as_micros_rounded()),
    ) else {
        return Some(None);
    };
    // The shorter of the exact rest and the rest on the playback clock ends
    // inside the sequence both ways.
    let duration = DonderDuration(exact.min(Duration::from_micros(micros as u64)));
    Some((duration.as_micros_rounded() > 0).then_some(duration))
}

/// Apply `repair` to `project`; whether it changed anything.
fn apply(project: &mut DonderProject, repair: &Repair) -> bool {
    match repair {
        Repair::RemoveLayoutItems { layout, items } => project
            .find_mut::<Layout>(*layout)
            .is_some_and(|layout| layout.remove_items(items).is_ok()),
        Repair::RemoveRoute { patch, route } => project
            .find_mut::<Patch>(*patch)
            .is_some_and(|patch| retain(&mut patch.routes, |candidate| candidate.id != *route)),
        Repair::ShortenRoute {
            patch,
            route,
            pixels,
        } => project
            .find_mut::<Patch>(*patch)
            .and_then(|patch| {
                patch
                    .routes
                    .iter_mut()
                    .find(|candidate| candidate.id == *route)
            })
            .is_some_and(|route| replace(&mut route.pixels, Some(*pixels))),
        Repair::RemoveClip { sequence, clip } => project
            .find_mut::<Sequence>(*sequence)
            .is_some_and(|sequence| retain(&mut sequence.clips, |candidate| &candidate.id != clip)),
        Repair::TrimClip {
            sequence,
            clip,
            duration,
        } => project
            .find_mut::<Sequence>(*sequence)
            .and_then(|sequence| {
                sequence
                    .clips
                    .iter_mut()
                    .find(|candidate| &candidate.id == clip)
            })
            .is_some_and(|clip| {
                clip.duration != *duration && {
                    Arc::make_mut(clip).duration = duration.clone();
                    true
                }
            }),
        Repair::TrimMarks {
            sequence,
            collection,
        } => project
            .find_mut::<Sequence>(*sequence)
            .is_some_and(|sequence| {
                let end = sequence.duration.0;
                sequence
                    .mark_collections
                    .iter_mut()
                    .find(|candidate| &candidate.key.name == collection)
                    .is_some_and(|collection| {
                        let mut changed = retain(&mut collection.marks, |mark| mark.time.0 <= end);
                        for mark in &mut collection.marks {
                            if mark.end() > end {
                                mark.duration.0 = end - mark.time.0;
                                changed = true;
                            }
                        }
                        changed
                    })
            }),
        Repair::RemoveAutomationClip { sequence, clip } => project
            .find_mut::<Sequence>(*sequence)
            .is_some_and(|sequence| {
                retain(&mut sequence.automation_clips, |candidate| {
                    &candidate.id != clip
                })
            }),
        Repair::TrimAutomationClip {
            sequence,
            clip,
            duration,
        } => project
            .find_mut::<Sequence>(*sequence)
            .and_then(|sequence| {
                sequence
                    .automation_clips
                    .iter_mut()
                    .find(|candidate| &candidate.id == clip)
            })
            .is_some_and(|clip| {
                clip.duration != *duration && {
                    clip.crop(clip.start.clone(), duration.clone());
                    true
                }
            }),
        Repair::DetachBindings { sequence, clip } => {
            let Some(value) = project.get::<Sequence>(*sequence) else {
                return false;
            };
            let (mut gone, mut lost) = (Vec::new(), Vec::new());
            for binding in value
                .automation_clips
                .iter()
                .find(|candidate| &candidate.id == clip)
                .map_or(&[][..], |clip| &clip.bindings)
            {
                match automation_target_param(project, value, &binding.target) {
                    Err(AutomationProblem::MissingClip | AutomationProblem::MissingNode) => {
                        gone.push(binding.target.clone());
                    }
                    Ok(param) if param.automation_mapping().is_some() => {}
                    _ => lost.push(binding.target.clone()),
                }
            }
            (!gone.is_empty() || !lost.is_empty())
                && project
                    .find_mut::<Sequence>(*sequence)
                    .and_then(|sequence| {
                        sequence
                            .automation_clips
                            .iter_mut()
                            .find(|candidate| &candidate.id == clip)
                    })
                    .is_some_and(|clip| {
                        clip.remove_bindings(|target| gone.contains(target));
                        clip.detach_bindings(|target| lost.contains(target));
                        true
                    })
        }
        Repair::Unbind {
            sequence,
            clip,
            target,
        } => project
            .find_mut::<Sequence>(*sequence)
            .and_then(|sequence| {
                sequence
                    .automation_clips
                    .iter_mut()
                    .find(|candidate| &candidate.id == clip)
            })
            .is_some_and(|clip| {
                let count = clip.targets().count();
                clip.remove_bindings(|candidate| candidate == target);
                clip.targets().count() != count
            }),
        Repair::ResetParam { owner, param } => {
            let overrides = match owner {
                ParamOwner::Clip { sequence, clip } => project
                    .find_mut::<Sequence>(*sequence)
                    .and_then(|sequence| {
                        sequence
                            .clips
                            .iter_mut()
                            .find(|candidate| &candidate.id == clip)
                    })
                    .filter(|clip| clip.param_overrides.contains_key(param))
                    .map(|clip| &mut Arc::make_mut(clip).param_overrides),
                ParamOwner::ConsoleClip { console, clip } => project
                    .find_mut::<Console>(*console)
                    .and_then(|console| {
                        console
                            .clips
                            .iter_mut()
                            .find(|candidate| candidate.id == *clip)
                    })
                    .map(|clip| &mut clip.param_overrides),
                ParamOwner::Node { graph, node } => {
                    let graph = match graph {
                        GraphOwner::Sequence(sequence) => project
                            .find_mut::<Sequence>(*sequence)
                            .map(|sequence| &mut sequence.composition_graph),
                        GraphOwner::Console(console) => project
                            .find_mut::<Console>(*console)
                            .map(|console| &mut console.composition_graph),
                    };
                    graph
                        .and_then(|graph| {
                            graph
                                .nodes
                                .iter_mut()
                                .find(|candidate| &candidate.id == node)
                        })
                        .and_then(|node| match &mut node.kind {
                            CompositionGraphNodeKind::Operator(operator) => {
                                Some(&mut operator.params)
                            }
                            _ => None,
                        })
                }
            };
            overrides.is_some_and(|overrides| overrides.shift_remove(param).is_some())
        }
        Repair::RemoveConsoleClip { console, clip } => project
            .find_mut::<Console>(*console)
            .is_some_and(|console| retain(&mut console.clips, |candidate| candidate.id != *clip)),
        Repair::ForgetAlways { console } => {
            project
                .find_mut::<Console>(*console)
                .is_some_and(|console| {
                    let clips = console.clips.iter().map(|clip| clip.id).collect::<Vec<_>>();
                    retain(&mut console.always, |entry| clips.contains(&entry.clip))
                })
        }
        Repair::RemoveEvent { console, event } => project
            .find_mut::<Console>(*console)
            .is_some_and(|console| retain(&mut console.events, |on| on.id != *event)),
        Repair::ForgetActions { console, event } => {
            let Some(value) = project.get::<Console>(*console) else {
                return false;
            };
            let kept = value
                .events
                .iter()
                .find(|on| on.id == *event)
                .map(|on| {
                    on.actions
                        .iter()
                        .map(|action| resolves(project, value, action))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            kept.contains(&false)
                && project
                    .find_mut::<Console>(*console)
                    .and_then(|console| console.events.iter_mut().find(|on| on.id == *event))
                    .is_some_and(|on| {
                        let mut kept = kept.into_iter();
                        on.actions.retain(|_| kept.next().unwrap_or(true));
                        true
                    })
        }
        Repair::RemoveSet {
            console,
            event,
            param,
        } => {
            let Some(value) = project.get::<Console>(*console) else {
                return false;
            };
            let Some(on) = value.events.iter().find(|on| on.id == *event) else {
                return false;
            };
            // The targets of `param` another control sets too.
            let shared = value
                .events
                .iter()
                .filter(|other| other.event.control() != on.event.control())
                .flat_map(|other| &other.actions)
                .filter_map(|action| match action {
                    Action::Set { target, .. } if target.param() == param => Some(target.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            project
                .find_mut::<Console>(*console)
                .and_then(|console| console.events.iter_mut().find(|on| on.id == *event))
                .is_some_and(|on| {
                    retain(&mut on.actions, |action| {
                        !matches!(action, Action::Set { target, .. } if shared.contains(target))
                    })
                })
        }
        Repair::DropPlayed {
            console,
            clip,
            param,
        }
        | Repair::ForgetPlayed {
            console,
            clip,
            param,
        } => project
            .find_mut::<Console>(*console)
            .and_then(|console| {
                console
                    .clips
                    .iter_mut()
                    .find(|candidate| candidate.id == *clip)
            })
            .is_some_and(|clip| retain(&mut clip.played, |value| &value.param != param)),
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
        Action::Mark(key) => console.live_marks.iter().any(|marks| &marks.key == key),
        Action::Set { target, .. } => target_param(
            project,
            &console.composition_graph,
            target,
            |clip: &ConsoleClipId| console.clip(*clip).map(|clip| clip.definition),
        )
        .is_ok_and(|param| param.automation_mapping().is_some()),
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

/// Set `slot` to `value`; whether it differed.
fn replace<T: PartialEq>(slot: &mut T, value: T) -> bool {
    *slot != value && {
        *slot = value;
        true
    }
}
