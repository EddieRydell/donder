//! The project rules an object can break. Each sentence a person reads about
//! one is written here, once, by `Display`.
use std::fmt;

use crate::console::{ConsoleClipId, MAX_COPIES, OnId};
use crate::controller::{
    ARTNET_PORT_ADDRESS_MAX, ControllerPortId, ControllerValidationError, DMX_SLOT_LIMIT,
    E131_UNIVERSE_MAX, E131_UNIVERSE_MIN,
};
use crate::effect::ClipId;
use crate::fixture::{FixtureDefinitionError, FixtureElementId, FixtureGeometryError};
use crate::identity::{ObjectId, ObjectKind};
use crate::input_device::{InputControlId, InputDeviceValidationError};
use crate::layout::{FixtureInstanceId, LayoutError};
use crate::operator::GraphValidationError;
use crate::patch::PixelRouteId;
use crate::sequence::{AutomationClipId, CompositionGraphNodeId, SequenceLayerId};
use crate::validation::{MAX_SEQUENCE_FRAME_COUNT, MAX_SEQUENCE_FRAME_RATE};
use donder_runtime_types::{
    BindingError, CurveValidationError, GradientValidationError, Identifier,
};

/// A project rule an object breaks, and the object.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectValidationError {
    pub object: ObjectId,
    pub problem: ValidationProblem,
}

impl fmt::Display for ProjectValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.problem.fmt(f)
    }
}

/// Why an object breaks a project rule.
#[derive(Clone, Debug, PartialEq)]
pub enum ValidationProblem {
    MissingSetup,
    MissingLayout,
    MissingPatch,
    MissingController,
    MissingInputDevice,
    Ownership(OwnershipProblem),
    /// A script definition is stored under an id its declaration does not carry.
    DefinitionMismatch,
    Curve(CurveValidationError),
    Gradient(GradientValidationError),
    Fixture(FixtureDefinitionError),
    Layout(LayoutError),
    Controller(ControllerValidationError),
    InputDevice(InputDeviceValidationError),
    Route {
        route: PixelRouteId,
        problem: RouteProblem,
    },
    Setup(SetupProblem),
    Sequence(SequenceValidationError),
    Console(ConsoleValidationError),
    Admission(AdmissionProblem),
}

/// Objects owned and referred to inconsistently.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnershipProblem {
    DuplicateId(ObjectId),
    StoredUnderOtherId(ObjectId),
    MissingReference(ObjectId),
    DuplicateSequence,
    DuplicateConsole,
    DuplicateController,
    DuplicateInputDevice,
}

/// A patch output that cannot be played as written.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RouteProblem {
    DuplicateId,
    InvalidEncoding,
    MissingLayout,
    TargetNotFixture,
    Target(LayoutError),
    SpanExceedsTarget,
    MissingController,
    MissingPort,
    ChannelOverflow,
    OutputExceedsPort,
    OutputsOverlap,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SetupProblem {
    TooManyControllers,
    SharedOscPort(u16),
    RouteOnOtherLayout(PixelRouteId),
    RouteControllerInactive(PixelRouteId),
}

/// A sequence that cannot be played as written.
#[derive(Clone, Debug, PartialEq)]
pub enum SequenceValidationError {
    MissingActiveSetup,
    Timing(TimingProblem),
    Name(NameProblem),
    Composition(CompositionProblem),
    DuplicateClipId(ClipId),
    DuplicateAutomationClipId(AutomationClipId),
    MarkOutsideDuration(Identifier),
    InvalidMarkLabel(Identifier),
    Clip {
        clip: ClipId,
        name: Identifier,
        problem: ClipProblem,
    },
    Automation {
        clip: AutomationClipId,
        problem: AutomationProblem,
    },
}

/// A console that cannot be played as written.
#[derive(Clone, Debug, PartialEq)]
pub enum ConsoleValidationError {
    MissingActiveSetup,
    Timing(TimingProblem),
    Name(NameProblem),
    Composition(CompositionProblem),
    DuplicateClipId(ConsoleClipId),
    DuplicateEventId(OnId),
    Clip {
        clip: ConsoleClipId,
        name: Identifier,
        problem: ClipProblem,
    },
    /// `Play` actions play the clip, and `Start`, `Toggle` or `always` hold
    /// it too.
    PlayedAndHeld {
        clip: ConsoleClipId,
        name: Identifier,
    },
    MissingAlwaysClip,
    RepeatedAlwaysClip {
        clip: ConsoleClipId,
        name: Identifier,
    },
    Event {
        event: OnId,
        problem: EventProblem,
    },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimingProblem {
    ZeroFrameRate,
    FrameRateAboveLimit,
    ZeroDuration,
    DurationBeyondClock,
    DurationRoundsToZero,
    TooManyFrames,
}

/// A name others refer to that is malformed or taken.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NameProblem {
    NotSnakeCase(Member, Identifier),
    Duplicate(Member, Identifier),
}

/// The layers and composition graph of a sequence or console.
#[derive(Clone, Debug, PartialEq)]
pub enum CompositionProblem {
    DuplicateLayerId(SequenceLayerId),
    Graph(GraphValidationError),
    MissingLayer(CompositionGraphNodeId),
    RepeatedLayer(CompositionGraphNodeId),
    Param {
        node: CompositionGraphNodeId,
        problem: ParamProblem,
    },
}

/// A sequence or console clip.
#[derive(Clone, Debug, PartialEq)]
pub enum ClipProblem {
    MissingLayer,
    Region(RegionProblem),
    NotInActiveLayout,
    MixedLayouts,
    MissingTargetLayout,
    MissingTarget,
    MissingDefinition,
    Param(ParamProblem),
}

/// A parameter value of a clip or operator.
#[derive(Clone, Debug, PartialEq)]
pub enum ParamProblem {
    WrongType(Identifier),
    MissingRequired(Identifier),
    Undeclared(Identifier),
    MissingMarks(Identifier),
    MissingCurve(Identifier),
    MissingGradient(Identifier),
    InvalidCurve(Identifier, CurveValidationError),
    InvalidGradient(Identifier, GradientValidationError),
}

/// A clip's place in time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegionProblem {
    ZeroDuration,
    Overflow,
    BeyondSequence,
    StartBeyondClock,
    DurationBeyondClock,
    RoundsToZero,
    EndBeyondAfterRounding,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AutomationProblem {
    MixedLayouts,
    NotInActiveLayout,
    MissingRow,
    Region(RegionProblem),
    InvalidCurve(CurveValidationError),
    CurveOutOfRange,
    BindsTwice,
    NotAutomatable(Identifier),
    MissingClip,
    MissingNode,
    MissingParam(Identifier),
    Overlap,
}

/// The control an event plays from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputProblem {
    MissingDevice,
    NotAControl,
    NotInSetup,
    /// `Press` or `Release` of an axis.
    NotAButton,
    /// `Move` of a button.
    NotAnAxis,
}

/// An event of a console and its actions.
#[derive(Clone, Debug, PartialEq)]
pub enum EventProblem {
    Input(InputProblem),
    /// An axis's `Move` only sets parameters.
    AxisPlays,
    /// Played values on an event other than a press.
    ValuesWithoutPress,
    MissingClip,
    MissingNode,
    MissingLiveMarks(Identifier),
    /// `Set` names a parameter its clip or operator does not have.
    UnknownParam(Identifier),
    NotAutomatable(Identifier),
    /// Actions also play the parameter on the copies they start.
    SetAlsoPlayed(Identifier),
    /// Another control's events set the same parameter.
    SetByOtherControl(Identifier),
    Copies(u8),
    DurationBeyondClock,
    NonPositiveDuration,
    ShortestAboveLongest,
    ValueTwice(Identifier),
    PitchWithoutNotes(Identifier),
    UnknownValue(Identifier),
    ValueNotAutomatable(Identifier),
}

/// Rules found while binding a validated project for playback.
#[derive(Clone, Debug, PartialEq)]
pub enum AdmissionProblem {
    UnknownAutomationParam,
    AutomationWithoutClip,
    GeometryOutOfRange,
    InvalidEncoding(PixelRouteId),
    TimingUnrepresentable,
    MissingParam(Identifier),
    Binding(Member, BindingError),
    LiveMarksInOperator(CompositionGraphNodeId),
    TooManyParams,
}

/// A named part of an object that a problem concerns.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Member {
    Layer(SequenceLayerId),
    Clip(ClipId),
    Marks(Identifier),
    AutomationClip(AutomationClipId),
    Node(CompositionGraphNodeId),
    LiveMarks(Identifier),
    ConsoleClip(ConsoleClipId),
    Event(OnId),
    Control(InputControlId),
    Port(ControllerPortId),
    LayoutItem(FixtureInstanceId),
    Route(PixelRouteId),
    Shape(FixtureElementId),
}

/// Which field of a member a problem concerns.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemberPart {
    /// A parameter value it sets.
    Param(Identifier),
    /// A value an action plays on the copy it starts.
    PlayedValue(Identifier),
    /// The control an event reads.
    Input,
    /// A shape of a fixture definition written in place.
    Shape(FixtureElementId),
}

/// Where in an object a problem lies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProblemSite {
    pub member: Member,
    pub part: Option<MemberPart>,
}

impl ProblemSite {
    fn of(member: Member) -> Self {
        Self { member, part: None }
    }

    fn with(member: Member, part: MemberPart) -> Self {
        Self {
            member,
            part: Some(part),
        }
    }
}

impl ValidationProblem {
    /// The member of the object the problem concerns, when it names one.
    pub fn site(&self) -> Option<ProblemSite> {
        match self {
            Self::Fixture(error) => match error {
                FixtureDefinitionError::TooManyPixels(_) => None,
                FixtureDefinitionError::DuplicateElement { element, .. }
                | FixtureDefinitionError::InvalidElement { element, .. } => {
                    Some(ProblemSite::of(Member::Shape(*element)))
                }
            },
            Self::Layout(LayoutError::InvalidFixture {
                fixture,
                error:
                    FixtureGeometryError::InvalidElement(shape)
                    | FixtureGeometryError::DuplicateElement(shape),
            }) => Some(ProblemSite::with(
                Member::LayoutItem(*fixture),
                MemberPart::Shape(*shape),
            )),
            Self::Layout(error) => error
                .item()
                .map(|item| ProblemSite::of(Member::LayoutItem(item))),
            Self::Controller(error) => error.port().map(|port| ProblemSite::of(Member::Port(port))),
            Self::InputDevice(error) => error
                .control()
                .map(|control| ProblemSite::of(Member::Control(control))),
            Self::Route { route, .. } => Some(ProblemSite::of(Member::Route(*route))),
            Self::Sequence(error) => error.site(),
            Self::Console(error) => error.site(),
            Self::Admission(AdmissionProblem::Binding(member, _)) => {
                Some(ProblemSite::of(member.clone()))
            }
            Self::Admission(AdmissionProblem::LiveMarksInOperator(node)) => {
                Some(ProblemSite::of(Member::Node(node.clone())))
            }
            Self::MissingSetup
            | Self::MissingLayout
            | Self::MissingPatch
            | Self::MissingController
            | Self::MissingInputDevice
            | Self::Ownership(_)
            | Self::DefinitionMismatch
            | Self::Curve(_)
            | Self::Gradient(_)
            | Self::Setup(_)
            | Self::Admission(_) => None,
        }
    }
}

impl SequenceValidationError {
    fn site(&self) -> Option<ProblemSite> {
        match self {
            Self::Name(problem) => Some(problem.site()),
            Self::Composition(problem) => problem.site(),
            Self::DuplicateClipId(clip) => Some(ProblemSite::of(Member::Clip(clip.clone()))),
            Self::DuplicateAutomationClipId(clip) | Self::Automation { clip, .. } => {
                Some(ProblemSite::of(Member::AutomationClip(clip.clone())))
            }
            Self::MarkOutsideDuration(name) | Self::InvalidMarkLabel(name) => {
                Some(ProblemSite::of(Member::Marks(name.clone())))
            }
            Self::Clip { clip, problem, .. } => Some(problem.site(Member::Clip(clip.clone()))),
            Self::MissingActiveSetup | Self::Timing(_) => None,
        }
    }
}

impl ConsoleValidationError {
    fn site(&self) -> Option<ProblemSite> {
        match self {
            Self::Name(problem) => Some(problem.site()),
            Self::Composition(problem) => problem.site(),
            Self::DuplicateClipId(clip)
            | Self::PlayedAndHeld { clip, .. }
            | Self::RepeatedAlwaysClip { clip, .. } => {
                Some(ProblemSite::of(Member::ConsoleClip(*clip)))
            }
            Self::DuplicateEventId(event) => Some(ProblemSite::of(Member::Event(*event))),
            Self::Clip { clip, problem, .. } => Some(problem.site(Member::ConsoleClip(*clip))),
            Self::Event { event, problem } => {
                let member = Member::Event(*event);
                Some(match problem {
                    EventProblem::Input(_) => ProblemSite::with(member, MemberPart::Input),
                    EventProblem::UnknownParam(param)
                    | EventProblem::NotAutomatable(param)
                    | EventProblem::SetAlsoPlayed(param)
                    | EventProblem::SetByOtherControl(param) => {
                        ProblemSite::with(member, MemberPart::Param(param.clone()))
                    }
                    EventProblem::ValueTwice(param)
                    | EventProblem::PitchWithoutNotes(param)
                    | EventProblem::UnknownValue(param)
                    | EventProblem::ValueNotAutomatable(param) => {
                        ProblemSite::with(member, MemberPart::PlayedValue(param.clone()))
                    }
                    EventProblem::AxisPlays
                    | EventProblem::ValuesWithoutPress
                    | EventProblem::MissingClip
                    | EventProblem::MissingNode
                    | EventProblem::MissingLiveMarks(_)
                    | EventProblem::Copies(_)
                    | EventProblem::DurationBeyondClock
                    | EventProblem::NonPositiveDuration
                    | EventProblem::ShortestAboveLongest => ProblemSite::of(member),
                })
            }
            Self::MissingActiveSetup | Self::Timing(_) | Self::MissingAlwaysClip => None,
        }
    }
}

impl NameProblem {
    fn site(&self) -> ProblemSite {
        let (Self::NotSnakeCase(member, _) | Self::Duplicate(member, _)) = self;
        ProblemSite::of(member.clone())
    }
}

impl CompositionProblem {
    fn site(&self) -> Option<ProblemSite> {
        match self {
            Self::DuplicateLayerId(layer) => Some(ProblemSite::of(Member::Layer(layer.clone()))),
            Self::Graph(error) => error.node().map(|node| ProblemSite::of(Member::Node(node))),
            Self::MissingLayer(node) | Self::RepeatedLayer(node) => {
                Some(ProblemSite::of(Member::Node(node.clone())))
            }
            Self::Param { node, problem } => Some(ProblemSite::with(
                Member::Node(node.clone()),
                MemberPart::Param(problem.param().clone()),
            )),
        }
    }
}

impl ClipProblem {
    fn site(&self, member: Member) -> ProblemSite {
        match self {
            Self::Param(problem) => {
                ProblemSite::with(member, MemberPart::Param(problem.param().clone()))
            }
            _ => ProblemSite::of(member),
        }
    }
}

impl ParamProblem {
    /// The parameter the problem concerns.
    fn param(&self) -> &Identifier {
        match self {
            Self::WrongType(param)
            | Self::MissingRequired(param)
            | Self::Undeclared(param)
            | Self::MissingMarks(param)
            | Self::MissingCurve(param)
            | Self::MissingGradient(param)
            | Self::InvalidCurve(param, _)
            | Self::InvalidGradient(param, _) => param,
        }
    }
}

impl fmt::Display for ValidationProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSetup => f.write_str("the project setup is missing"),
            Self::MissingLayout => f.write_str("the referenced layout is missing"),
            Self::MissingPatch => f.write_str("the setup patch is missing"),
            Self::MissingController => f.write_str("a referenced controller is missing"),
            Self::MissingInputDevice => f.write_str("a referenced input device is missing"),
            Self::Ownership(problem) => problem.fmt(f),
            Self::DefinitionMismatch => {
                f.write_str("the definition does not match its compiled declaration")
            }
            Self::Curve(error) => write!(f, "invalid curve: {}", curve_error(error)),
            Self::Gradient(error) => write!(f, "invalid gradient: {}", gradient_error(error)),
            Self::Fixture(error) => error.fmt(f),
            Self::Layout(error) => error.fmt(f),
            Self::Controller(error) => error.fmt(f),
            Self::InputDevice(error) => error.fmt(f),
            Self::Route { route, problem } => write!(f, "output {}: {problem}", route.0),
            Self::Setup(problem) => problem.fmt(f),
            Self::Sequence(error) => error.fmt(f),
            Self::Console(error) => error.fmt(f),
            Self::Admission(problem) => problem.fmt(f),
        }
    }
}

impl fmt::Display for OwnershipProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateId(id) => write!(f, "the {} id {} is used twice", id.kind(), id.raw()),
            Self::StoredUnderOtherId(id) => {
                write!(f, "a reusable {} is stored under another id", id.kind())
            }
            Self::MissingReference(id) => write!(f, "a referenced {} is missing", id.kind()),
            Self::DuplicateSequence => f.write_str("a sequence appears twice in the project"),
            Self::DuplicateConsole => f.write_str("a console appears twice in the project"),
            Self::DuplicateController => f.write_str("a controller appears twice in the setup"),
            Self::DuplicateInputDevice => f.write_str("an input device appears twice in the setup"),
        }
    }
}

impl fmt::Display for RouteProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::DuplicateId => "another output has the same id",
            Self::InvalidEncoding => "invalid LED encoding, gamma, or brightness",
            Self::MissingLayout => "the target layout is missing",
            Self::TargetNotFixture => "an output targets a fixture, not a group",
            Self::Target(error) => return write!(f, "{error}"),
            Self::SpanExceedsTarget => "the pixel span exceeds its target",
            Self::MissingController => "the controller is missing",
            Self::MissingPort => "the controller has no such port",
            Self::ChannelOverflow => "the channel count overflows",
            Self::OutputExceedsPort => "the output exceeds its controller port",
            Self::OutputsOverlap => "the output overlaps another on its controller port",
        })
    }
}

impl fmt::Display for SetupProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyControllers => {
                f.write_str("the setup has more controllers than outputs can address")
            }
            Self::SharedOscPort(port) => {
                write!(
                    f,
                    "two OSC input devices of the setup listen on port {port}"
                )
            }
            Self::RouteOnOtherLayout(route) => {
                write!(
                    f,
                    "output {} targets a layout other than the setup's",
                    route.0
                )
            }
            Self::RouteControllerInactive(route) => write!(
                f,
                "output {} plays through a controller that is not in the setup",
                route.0
            ),
        }
    }
}

impl fmt::Display for TimingProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroFrameRate => f.write_str("the frame rate must be greater than zero"),
            Self::FrameRateAboveLimit => write!(
                f,
                "the frame rate exceeds the limit of {MAX_SEQUENCE_FRAME_RATE} frames per second"
            ),
            Self::ZeroDuration => f.write_str("the duration must be positive"),
            Self::DurationBeyondClock => {
                f.write_str("the duration exceeds the runtime clock range")
            }
            Self::DurationRoundsToZero => {
                f.write_str("the duration rounds to zero on the runtime clock")
            }
            Self::TooManyFrames => write!(
                f,
                "the sequence exceeds the frame budget of {MAX_SEQUENCE_FRAME_COUNT} frames"
            ),
        }
    }
}

impl fmt::Display for NameProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotSnakeCase(_, name) => {
                write!(f, "`{}` is not a snake_case name", name.as_str())
            }
            Self::Duplicate(member, name) => {
                write!(f, "{} `{}` is declared twice", member.noun(), name.as_str())
            }
        }
    }
}

impl Member {
    /// What a person calls this kind of member.
    fn noun(&self) -> &'static str {
        match self {
            Self::Layer(_) => "layer",
            Self::Clip(_) => "clip",
            Self::Marks(_) => "mark collection",
            Self::AutomationClip(_) => "automation clip",
            Self::Node(_) => "graph node",
            Self::LiveMarks(_) => "live mark collection",
            Self::ConsoleClip(_) => "clip",
            Self::Event(_) => "event",
            Self::Control(_) => "control",
            Self::Port(_) => "port",
            Self::LayoutItem(_) => "layout item",
            Self::Route(_) => "output",
            Self::Shape(_) => "shape",
        }
    }
}

impl fmt::Display for SequenceValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingActiveSetup => f.write_str("the active setup is missing"),
            Self::Timing(problem) => problem.fmt(f),
            Self::Name(problem) => problem.fmt(f),
            Self::Composition(problem) => problem.fmt(f),
            Self::DuplicateClipId(clip) => write!(f, "clip id {} is used twice", clip.0),
            Self::DuplicateAutomationClipId(clip) => {
                write!(f, "automation clip id {} is used twice", clip.0)
            }
            Self::MarkOutsideDuration(name) => write!(
                f,
                "a mark of `{}` lies outside the sequence duration",
                name.as_str()
            ),
            Self::InvalidMarkLabel(name) => write!(
                f,
                "a mark label of `{}` must be one non-empty line without surrounding whitespace",
                name.as_str()
            ),
            Self::Clip { name, problem, .. } => write!(f, "clip `{}`: {problem}", name.as_str()),
            Self::Automation { problem, .. } => problem.fmt(f),
        }
    }
}

impl fmt::Display for ConsoleValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingActiveSetup => f.write_str("the active setup is missing"),
            Self::Timing(problem) => problem.fmt(f),
            Self::Name(problem) => problem.fmt(f),
            Self::Composition(problem) => problem.fmt(f),
            Self::DuplicateClipId(clip) => write!(f, "clip id {} is used twice", clip.0),
            Self::DuplicateEventId(event) => write!(f, "event id {} is used twice", event.0),
            Self::Clip { name, problem, .. } => write!(f, "clip `{}`: {problem}", name.as_str()),
            Self::PlayedAndHeld { name, .. } => write!(
                f,
                "clip `{}` is played by `Play` and also held by `Start`, `Toggle` or `always`",
                name.as_str()
            ),
            Self::MissingAlwaysClip => f.write_str("`always` names a missing clip"),
            Self::RepeatedAlwaysClip { name, .. } => {
                write!(f, "`always` lists clip `{}` twice", name.as_str())
            }
            Self::Event { problem, .. } => problem.fmt(f),
        }
    }
}

impl fmt::Display for CompositionProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateLayerId(layer) => write!(f, "layer id {} is used twice", layer.0),
            Self::Graph(error) => error.fmt(f),
            Self::MissingLayer(_) => f.write_str("a layer node names a missing layer"),
            Self::RepeatedLayer(_) => {
                f.write_str("the composition graph contains this layer more than once")
            }
            Self::Param { problem, .. } => problem.fmt(f),
        }
    }
}

impl fmt::Display for ClipProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingLayer => f.write_str("the clip's layer is missing"),
            Self::Region(problem) => problem.fmt(f),
            Self::NotInActiveLayout => {
                f.write_str("the clip target is not present in the active layout")
            }
            Self::MixedLayouts => f.write_str("all clip targets must use the same layout"),
            Self::MissingTargetLayout => f.write_str("the clip target layout is missing"),
            Self::MissingTarget => f.write_str("the clip target is not in its layout"),
            Self::MissingDefinition => f.write_str("the effect definition is missing"),
            Self::Param(problem) => problem.fmt(f),
        }
    }
}

impl fmt::Display for ParamProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongType(param) => {
                write!(f, "parameter `{}` has the wrong type", param.as_str())
            }
            Self::MissingRequired(param) => {
                write!(f, "required parameter `{}` is missing", param.as_str())
            }
            Self::Undeclared(param) => write!(f, "no parameter `{}`", param.as_str()),
            Self::MissingMarks(param) => write!(
                f,
                "parameter `{}` names a missing mark collection",
                param.as_str()
            ),
            Self::MissingCurve(param) => {
                write!(f, "parameter `{}` names a missing curve", param.as_str())
            }
            Self::MissingGradient(param) => {
                write!(f, "parameter `{}` names a missing gradient", param.as_str())
            }
            Self::InvalidCurve(param, error) => write!(
                f,
                "parameter `{}` has an invalid curve: {}",
                param.as_str(),
                curve_error(error)
            ),
            Self::InvalidGradient(param, error) => write!(
                f,
                "parameter `{}` has an invalid gradient: {}",
                param.as_str(),
                gradient_error(error)
            ),
        }
    }
}

impl fmt::Display for RegionProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ZeroDuration => "the duration must be positive",
            Self::Overflow => "the timing overflows",
            Self::BeyondSequence => "it extends beyond the sequence duration",
            Self::StartBeyondClock => "the start exceeds the runtime clock range",
            Self::DurationBeyondClock => "the duration exceeds the runtime clock range",
            Self::RoundsToZero => "the duration rounds to zero on the runtime clock",
            Self::EndBeyondAfterRounding => {
                "the end exceeds the runtime clock range or sequence duration after rounding"
            }
        })
    }
}

impl fmt::Display for AutomationProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MixedLayouts => {
                f.write_str("all timeline row targets in a sequence must use the same layout")
            }
            Self::NotInActiveLayout => {
                f.write_str("the automation row is not in the active layout")
            }
            Self::MissingRow => f.write_str("the automation row target is missing"),
            Self::Region(problem) => problem.fmt(f),
            Self::InvalidCurve(error) => {
                write!(f, "invalid automation curve: {}", curve_error(error))
            }
            Self::CurveOutOfRange => f.write_str("automation curve values must lie in 0..1"),
            Self::BindsTwice => f.write_str("the automation clip binds one parameter twice"),
            Self::NotAutomatable(param) => {
                write!(f, "parameter `{}` cannot be automated", param.as_str())
            }
            Self::MissingClip => f.write_str("the automated clip is missing"),
            Self::MissingNode => f.write_str("the automated graph node is missing"),
            Self::MissingParam(param) => {
                write!(f, "the automated parameter `{}` is missing", param.as_str())
            }
            Self::Overlap => {
                f.write_str("automation clips that bind the same parameter must not overlap")
            }
        }
    }
}

impl fmt::Display for InputProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MissingDevice => "the input device is missing",
            Self::NotAControl => "the input is not a control of its device",
            Self::NotInSetup => "the input device is not in the project's setup",
            Self::NotAButton => "`Press` and `Release` read a button, not an axis",
            Self::NotAnAxis => "`Move` reads an axis, not a button",
        })
    }
}

impl fmt::Display for EventProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(problem) => problem.fmt(f),
            Self::AxisPlays => f.write_str("a `Move` can only `Set` parameters"),
            Self::ValuesWithoutPress => f.write_str("only a `Press` plays values"),
            Self::MissingClip => f.write_str("an action names a missing clip"),
            Self::MissingNode => f.write_str("`Set` names a missing operator node"),
            Self::UnknownParam(param) => write!(f, "no parameter `{}`", param.as_str()),
            Self::NotAutomatable(param) => write!(
                f,
                "`{}` cannot be set; `Set` writes a float, int, bool, enum or curve parameter",
                param.as_str()
            ),
            Self::SetAlsoPlayed(param) => write!(
                f,
                "parameter `{}` is also played by an action's values",
                param.as_str()
            ),
            Self::SetByOtherControl(param) => write!(
                f,
                "parameter `{}` is set by another control",
                param.as_str()
            ),
            Self::MissingLiveMarks(name) => {
                write!(f, "no live mark collection `{}`", name.as_str())
            }
            Self::Copies(count) => write!(
                f,
                "plays {count} copies; `Play` plays 1 to {MAX_COPIES} at once"
            ),
            Self::DurationBeyondClock => f.write_str("a length exceeds the runtime clock range"),
            Self::NonPositiveDuration => f.write_str("a length must be positive"),
            Self::ShortestAboveLongest => {
                f.write_str("a `Scaled` length's `shortest` is longer than its `longest`")
            }
            Self::ValueTwice(param) => write!(f, "`{}` is played twice", param.as_str()),
            Self::PitchWithoutNotes(param) => write!(
                f,
                "`{}` is played from a note, but the input plays no notes",
                param.as_str()
            ),
            Self::UnknownValue(param) => write!(f, "no parameter `{}`", param.as_str()),
            Self::ValueNotAutomatable(param) => write!(
                f,
                "`{}` cannot be played; a played value sets a float, int, bool, enum or curve parameter",
                param.as_str()
            ),
        }
    }
}

impl fmt::Display for AdmissionProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownAutomationParam => f.write_str("an automated parameter is unknown"),
            Self::AutomationWithoutClip => f.write_str("an automation target has no clip"),
            Self::GeometryOutOfRange => {
                f.write_str("transformed fixture geometry is outside the playback coordinate range")
            }
            Self::InvalidEncoding(route) => {
                write!(f, "output {} has an invalid output encoding", route.0)
            }
            Self::TimingUnrepresentable => {
                f.write_str("the timing is not representable by the playback clock")
            }
            Self::MissingParam(param) => write!(f, "parameter `{}` has no value", param.as_str()),
            Self::Binding(_, error) => f.write_str(&error.message),
            Self::LiveMarksInOperator(_) => {
                f.write_str("operators in a console cannot read live mark collections")
            }
            Self::TooManyParams => f.write_str("the definition has too many parameters"),
        }
    }
}

fn curve_error(error: &CurveValidationError) -> &'static str {
    match error {
        CurveValidationError::NonFinitePoint => "a point is not finite",
        CurveValidationError::PositionOutOfRange => "a position lies outside 0..1",
        CurveValidationError::PositionsNotStrictlyIncreasing => "positions decrease",
    }
}

fn gradient_error(error: &GradientValidationError) -> &'static str {
    match error {
        GradientValidationError::InvalidPosition => "a stop position is not in 0..1",
        GradientValidationError::PositionsOutOfOrder => "stop positions decrease",
    }
}

impl fmt::Display for ObjectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Project => "project",
            Self::Setup => "setup",
            Self::Layout => "layout",
            Self::Patch => "patch",
            Self::Controller => "controller",
            Self::InputDevice => "input device",
            Self::Sequence => "sequence",
            Self::Console => "console",
            Self::FixtureDefinition => "fixture definition",
            Self::Curve => "curve",
            Self::Gradient => "gradient",
            Self::EffectDefinition => "effect",
            Self::OperatorDefinition => "operator",
        })
    }
}

impl fmt::Display for FixtureDefinitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyPixels(_) => FixtureGeometryError::TooManyPixels.fmt(f),
            Self::DuplicateElement { element, .. } => {
                FixtureGeometryError::DuplicateElement(*element).fmt(f)
            }
            Self::InvalidElement { element, .. } => {
                FixtureGeometryError::InvalidElement(*element).fmt(f)
            }
        }
    }
}

impl fmt::Display for FixtureGeometryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TooManyPixels => "the fixture has too many pixels",
            Self::DuplicateElement(_) => "the shape's id or name is used twice",
            Self::InvalidElement(_) => {
                "invalid shape: a snake_case name, a finite transform with nonzero scale, a diameter of 1µm to 100m, and a geometry of 1 or more pixels"
            }
        })
    }
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyPixels(_) => f.write_str("the target has too many pixels"),
            Self::DuplicateId(item) => write!(f, "layout item id {} is used twice", item.0),
            Self::InvalidName(_, name) => {
                write!(f, "`{}` is not a snake_case name", name.as_str())
            }
            Self::DuplicateName(_, name) => {
                write!(f, "duplicate layout item name `{}`", name.as_str())
            }
            Self::MissingDefinition(_) => f.write_str("a fixture definition is missing"),
            Self::InvalidFixture { error, .. } => error.fmt(f),
            Self::InvalidTransform(_) => {
                f.write_str("invalid transform: rotations are finite, scales finite and nonzero")
            }
            Self::MissingFixture(item) => write!(f, "layout item {} is missing", item.0),
            Self::WrongLayout(_) => f.write_str("the target is in another layout"),
            Self::DuplicateMember(_, name) => write!(
                f,
                "layout item `{}` is listed twice in one group",
                name.as_str()
            ),
            Self::Cycle(_, name) => write!(
                f,
                "layout item `{}` contains itself through its members",
                name.as_str()
            ),
            Self::Unreachable(_, name) => write!(
                f,
                "layout item `{}` is not in `root` or any group",
                name.as_str()
            ),
        }
    }
}

impl LayoutError {
    /// The item the error concerns, when it names one of the layout's items.
    fn item(&self) -> Option<FixtureInstanceId> {
        match self {
            Self::TooManyPixels(item)
            | Self::DuplicateId(item)
            | Self::InvalidName(item, _)
            | Self::DuplicateName(item, _)
            | Self::InvalidTransform(item)
            | Self::DuplicateMember(item, _)
            | Self::Cycle(item, _)
            | Self::Unreachable(item, _)
            | Self::InvalidFixture { fixture: item, .. } => Some(*item),
            Self::MissingDefinition(_) | Self::MissingFixture(_) | Self::WrongLayout(_) => None,
        }
    }
}

impl fmt::Display for ControllerValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::controller::ControllerPortAddress as Address;
        match self {
            Self::EmptySenderName => f.write_str("an E1.31 sender name is not empty"),
            Self::InvalidPriority(priority) => {
                write!(f, "E1.31 priority {priority} is outside 1..200")
            }
            Self::DuplicatePort(port) => write!(f, "port id {} is used twice", port.0),
            Self::InvalidPortName(_) => {
                f.write_str("port names are snake_case and unique within their controller")
            }
            Self::EmptyPort(_) => f.write_str("a port carries at least one slot"),
            Self::TooManySlots { slots, .. } => {
                write!(f, "{slots} slots exceed the {DMX_SLOT_LIMIT} a DMX universe carries")
            }
            Self::ProtocolAddressMismatch(_) => f.write_str(
                "this controller's port addresses are `Universe { ... }` for E1.31, `ArtNetPort { ... }` for Art-Net, and `Output { ... }` for Donder",
            ),
            Self::InvalidE131Universe { universe, .. } => {
                write!(
                    f,
                    "E1.31 universe {universe} is outside {E131_UNIVERSE_MIN}..{E131_UNIVERSE_MAX}"
                )
            }
            Self::InvalidArtNetPort { address, .. } => {
                write!(
                    f,
                    "Art-Net port address {address} is above {ARTNET_PORT_ADDRESS_MAX}"
                )
            }
            Self::InvalidDonderOutput { output, .. } => write!(
                f,
                "Donder output {output} is outside 1 to the controller's port count"
            ),
            Self::DuplicateProtocolAddress(address) => match address {
                Address::E131Universe(universe) => {
                    write!(f, "two ports send universe {universe}")
                }
                Address::ArtNetPort(port) => write!(f, "two ports send Art-Net port {port}"),
                Address::DonderOutput(output) => write!(f, "two ports drive output {output}"),
            },
        }
    }
}

impl ControllerValidationError {
    /// The port the error concerns.
    fn port(&self) -> Option<ControllerPortId> {
        match self {
            Self::DuplicatePort(port)
            | Self::InvalidPortName(port)
            | Self::EmptyPort(port)
            | Self::ProtocolAddressMismatch(port)
            | Self::TooManySlots { port, .. }
            | Self::InvalidE131Universe { port, .. }
            | Self::InvalidArtNetPort { port, .. }
            | Self::InvalidDonderOutput { port, .. } => Some(*port),
            Self::EmptySenderName
            | Self::InvalidPriority(_)
            | Self::DuplicateProtocolAddress(_) => None,
        }
    }
}
