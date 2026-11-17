//! The project rules an object can break. Each sentence a person reads about
//! one is written here, once, by `Display`.
use std::fmt;

use crate::console::{ConsoleClipId, MAX_COPIES, OnId};
use crate::controller::{
    ARTNET_PORT_ADDRESS_MAX, ChannelPlace, ControllerOutput, ControllerPortId,
    ControllerValidationError, DMX_SLOT_LIMIT, E131_UNIVERSE_MAX, E131_UNIVERSE_MIN,
};
use crate::effect::ClipId;
use crate::fixture::{FixtureDefinitionError, FixtureElementId, FixtureGeometryError};
use crate::identity::{ObjectId, ObjectKind};
use crate::input_device::{InputControlId, InputDeviceValidationError};
use crate::layout::{FixtureInstanceId, LayoutError};
use crate::operator::GraphValidationError;
use crate::patch::PixelRouteId;
use crate::patch::PixelSpan;
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
        /// The fixture it drives, when its layout has it.
        fixture: Option<Identifier>,
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
    InvalidColorOrder,
    InvalidGamma,
    InvalidBrightness,
    MissingLayout,
    TargetNotFixture,
    Target(LayoutError),
    EmptySpan,
    SpanExceedsTarget {
        span: PixelSpan,
        /// The fixture's pixels.
        pixels: u32,
    },
    MissingController,
    /// The route names a port of an E1.31 or Art-Net controller.
    PortOnUniverses,
    /// The route on a Donder controller names none of its ports.
    NeedsPort,
    MissingPort,
    ChannelOverflow,
    /// Zero-based `channels` end past the `available` channels of `output`.
    PastLastChannel {
        channels: std::ops::Range<u32>,
        available: u32,
        output: ControllerOutput,
    },
    /// The route's zero-based `pixels` share the channels at `place` with
    /// an earlier route, of the fixture `other`.
    OutputsOverlap {
        pixels: std::ops::Range<u32>,
        other: Identifier,
        place: ChannelPlace,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SetupProblem {
    TooManyControllers,
    SharedOscPort(u16),
    /// A route, of the fixture when its layout has it, targets another layout.
    RouteOnOtherLayout {
        route: PixelRouteId,
        fixture: Option<Identifier>,
    },
    /// A route, of the fixture when its layout has it, plays through a
    /// controller the setup does not have.
    RouteControllerInactive {
        route: PixelRouteId,
        fixture: Option<Identifier>,
    },
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
    /// The length `always` repeats a clip every is not positive or leaves
    /// the playback clock.
    AlwaysLength {
        clip: ConsoleClipId,
        name: Identifier,
        problem: EventProblem,
    },
    Event {
        event: OnId,
        problem: EventProblem,
    },
    Played {
        clip: ConsoleClipId,
        name: Identifier,
        param: Identifier,
        problem: PlayedProblem,
    },
}

/// A clip's played value that cannot be played.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayedProblem {
    Twice,
    UnknownParam,
    NotAutomatable,
    /// The window leaves `0..1`.
    WindowOutOfRange,
    /// `always`, or an event other than a press, starts the clip.
    WithoutPress,
    /// A control that starts the clip reads no notes.
    WithoutNotes,
    /// A `Set` writes the parameter too.
    AlsoSet,
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
    /// Clips play on layers, so a sequence or console keeps at least one.
    NoLayers,
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
    WindowOutOfRange,
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
    MissingClip,
    MissingNode,
    MissingLiveMarks(Identifier),
    /// `Set` names a parameter its clip or operator does not have.
    UnknownParam(Identifier),
    NotAutomatable(Identifier),
    /// Another control's events set the same parameter.
    SetByOtherControl(Identifier),
    Copies(u8),
    DurationBeyondClock,
    NonPositiveDuration,
    ShortestAboveLongest,
    /// A `Set`'s window leaves `0..1`.
    WindowOutOfRange(Identifier),
}

/// Rules found while binding a validated project for playback.
#[derive(Clone, Debug, PartialEq)]
pub enum AdmissionProblem {
    GeometryOutOfRange,
    InvalidEncoding(PixelRouteId),
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
    /// A value each copy of a console clip takes from its press.
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
            | Self::RepeatedAlwaysClip { clip, .. }
            | Self::AlwaysLength { clip, .. } => Some(ProblemSite::of(Member::ConsoleClip(*clip))),
            Self::DuplicateEventId(event) => Some(ProblemSite::of(Member::Event(*event))),
            Self::Clip { clip, problem, .. } => Some(problem.site(Member::ConsoleClip(*clip))),
            Self::Event { event, problem } => {
                let member = Member::Event(*event);
                Some(match problem {
                    EventProblem::Input(_) => ProblemSite::with(member, MemberPart::Input),
                    EventProblem::UnknownParam(param)
                    | EventProblem::NotAutomatable(param)
                    | EventProblem::SetByOtherControl(param)
                    | EventProblem::WindowOutOfRange(param) => {
                        ProblemSite::with(member, MemberPart::Param(param.clone()))
                    }
                    EventProblem::AxisPlays
                    | EventProblem::MissingClip
                    | EventProblem::MissingNode
                    | EventProblem::MissingLiveMarks(_)
                    | EventProblem::Copies(_)
                    | EventProblem::DurationBeyondClock
                    | EventProblem::NonPositiveDuration
                    | EventProblem::ShortestAboveLongest => ProblemSite::of(member),
                })
            }
            Self::Played { clip, param, .. } => Some(ProblemSite::with(
                Member::ConsoleClip(*clip),
                MemberPart::PlayedValue(param.clone()),
            )),
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
            Self::NoLayers => None,
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
            Self::DefinitionMismatch => f.write_str("the definition doesn't match its script"),
            Self::Curve(error) => write!(f, "invalid curve: {}", curve_error(error)),
            Self::Gradient(error) => write!(f, "invalid gradient: {}", gradient_error(error)),
            Self::Fixture(error) => error.fmt(f),
            Self::Layout(error) => error.fmt(f),
            Self::Controller(error) => error.fmt(f),
            Self::InputDevice(error) => error.fmt(f),
            Self::Route {
                fixture: Some(fixture),
                problem,
                ..
            } => write!(f, "the output of {}: {problem}", fixture.as_str()),
            Self::Route { problem, .. } => write!(f, "an output: {problem}"),
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
        match self {
            Self::DuplicateId => f.write_str("another output has the same id"),
            Self::InvalidColorOrder => f.write_str("its color order repeats a color"),
            Self::InvalidGamma => f.write_str("its gamma is not above 0"),
            Self::InvalidBrightness => f.write_str("its brightness is not from 0 to 1"),
            Self::MissingLayout => f.write_str("its layout is missing"),
            Self::TargetNotFixture => f.write_str("it targets a group, not a fixture"),
            Self::Target(error) => error.fmt(f),
            Self::EmptySpan => f.write_str("it routes no pixels"),
            Self::SpanExceedsTarget { span, pixels } => write!(
                f,
                "pixels {} run past the fixture's {pixels}",
                Span(span.start..span.start.saturating_add(span.count))
            ),
            Self::MissingController => f.write_str("its controller is missing"),
            Self::PortOnUniverses => f.write_str("E1.31 and Art-Net controllers have no ports"),
            Self::NeedsPort => f.write_str("it needs a port"),
            Self::MissingPort => f.write_str("its port is missing"),
            Self::ChannelOverflow => {
                f.write_str("it has more channels than a controller can address")
            }
            Self::PastLastChannel {
                channels,
                available,
                output,
            } => write!(
                f,
                "channels {} run past the {available} channels of its {}",
                Span(channels.clone()),
                match output {
                    ControllerOutput::Universes => "controller",
                    ControllerOutput::Port(_) => "output",
                }
            ),
            Self::OutputsOverlap {
                pixels,
                other,
                place,
            } => write!(
                f,
                "pixels {} overlap {} on {place}",
                Span(pixels.clone()),
                other.as_str()
            ),
        }
    }
}

/// A nonempty zero-based range, as people count: `1–170`, or `4` alone.
struct Span(std::ops::Range<u32>);

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self(range) = self;
        if range.end <= range.start + 1 {
            write!(f, "{}", range.start + 1)
        } else {
            write!(f, "{}–{}", range.start + 1, range.end)
        }
    }
}

impl fmt::Display for ChannelPlace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Universes { first, last } if first.universe == last.universe => {
                if first.channel == last.channel {
                    write!(f, "universe {} channel {}", first.universe, first.channel)
                } else {
                    write!(
                        f,
                        "universe {} channels {}–{}",
                        first.universe, first.channel, last.channel
                    )
                }
            }
            Self::Universes { first, last } => write!(
                f,
                "universe {} channel {} to universe {} channel {}",
                first.universe, first.channel, last.universe, last.channel
            ),
            Self::Port { port, first, last } if first == last => {
                write!(f, "{} channel {first}", port.as_str())
            }
            Self::Port { port, first, last } => {
                write!(f, "{} channels {first}–{last}", port.as_str())
            }
        }
    }
}

impl fmt::Display for SetupProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyControllers => {
                f.write_str("the setup has more controllers than outputs can address")
            }
            Self::SharedOscPort(port) => {
                write!(f, "two OSC input devices listen on port {port}")
            }
            Self::RouteOnOtherLayout { fixture, .. } => write!(
                f,
                "the patch sends {} on another layout",
                Fixture(fixture.as_ref())
            ),
            Self::RouteControllerInactive { fixture, .. } => write!(
                f,
                "the patch sends {} to a controller outside the setup",
                Fixture(fixture.as_ref())
            ),
        }
    }
}

impl fmt::Display for TimingProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroFrameRate => f.write_str("the frame rate must be greater than zero"),
            Self::FrameRateAboveLimit => {
                write!(f, "the frame rate is above {MAX_SEQUENCE_FRAME_RATE} fps")
            }
            Self::ZeroDuration => f.write_str("the duration must be positive"),
            Self::DurationBeyondClock => f.write_str("the duration is too long"),
            Self::DurationRoundsToZero => f.write_str("the duration is shorter than a frame"),
            Self::TooManyFrames => write!(
                f,
                "the sequence is longer than {MAX_SEQUENCE_FRAME_COUNT} frames"
            ),
        }
    }
}

impl fmt::Display for NameProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotSnakeCase(_, name) => write!(f, "{} is not a valid name", name.as_str()),
            Self::Duplicate(member, name) => {
                write!(f, "two {}s are named {}", member.noun(), name.as_str())
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
            Self::MissingActiveSetup => f.write_str("the project's setup is missing"),
            Self::Timing(problem) => problem.fmt(f),
            Self::Name(problem) => problem.fmt(f),
            Self::Composition(problem) => problem.fmt(f),
            Self::DuplicateClipId(clip) => write!(f, "clip id {} is used twice", clip.0),
            Self::DuplicateAutomationClipId(clip) => {
                write!(f, "automation clip id {} is used twice", clip.0)
            }
            Self::MarkOutsideDuration(name) => {
                write!(f, "a mark of {} is past the sequence's end", name.as_str())
            }
            Self::InvalidMarkLabel(name) => write!(
                f,
                "a mark label of {} is empty, spans lines or has outer spaces",
                name.as_str()
            ),
            Self::Clip { name, problem, .. } => write!(f, "clip {}: {problem}", name.as_str()),
            Self::Automation { problem, .. } => problem.fmt(f),
        }
    }
}

impl fmt::Display for ConsoleValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingActiveSetup => f.write_str("the project's setup is missing"),
            Self::Timing(problem) => problem.fmt(f),
            Self::Name(problem) => problem.fmt(f),
            Self::Composition(problem) => problem.fmt(f),
            Self::DuplicateClipId(clip) => write!(f, "clip id {} is used twice", clip.0),
            Self::DuplicateEventId(event) => write!(f, "event id {} is used twice", event.0),
            Self::Clip { name, problem, .. } => write!(f, "clip {}: {problem}", name.as_str()),
            Self::PlayedAndHeld { name, .. } => write!(
                f,
                "clip {} is played by Play and held by Start, Toggle or Always",
                name.as_str()
            ),
            Self::MissingAlwaysClip => f.write_str("an Always clip is missing"),
            Self::RepeatedAlwaysClip { name, .. } => {
                write!(f, "clip {} is set to Always twice", name.as_str())
            }
            Self::AlwaysLength { name, problem, .. } => {
                write!(f, "Always clip {}: {problem}", name.as_str())
            }
            Self::Event { problem, .. } => problem.fmt(f),
            Self::Played {
                name,
                param,
                problem,
                ..
            } => {
                let (clip, param) = (name.as_str(), param.as_str());
                match problem {
                    PlayedProblem::Twice => write!(f, "clip {clip} plays {param} twice"),
                    PlayedProblem::UnknownParam => {
                        write!(f, "clip {clip}'s effect has no parameter {param}")
                    }
                    PlayedProblem::NotAutomatable => {
                        write!(f, "clip {clip} can't play parameter {param}")
                    }
                    PlayedProblem::WindowOutOfRange => {
                        write!(f, "clip {clip}: the range of {param} leaves 0 to 1")
                    }
                    PlayedProblem::WithoutPress => {
                        write!(f, "clip {clip} plays {param} but no press starts it")
                    }
                    PlayedProblem::WithoutNotes => {
                        write!(f, "clip {clip} plays {param} from a note no control sends")
                    }
                    PlayedProblem::AlsoSet => {
                        write!(f, "clip {clip} plays {param}, which a Set also writes")
                    }
                }
            }
        }
    }
}

impl fmt::Display for CompositionProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoLayers => f.write_str("there must be at least one layer"),
            Self::DuplicateLayerId(layer) => write!(f, "layer id {} is used twice", layer.0),
            Self::Graph(error) => error.fmt(f),
            Self::MissingLayer(_) => f.write_str("a layer in the graph is missing"),
            Self::RepeatedLayer(_) => f.write_str("the layer is in the graph twice"),
            Self::Param { problem, .. } => problem.fmt(f),
        }
    }
}

impl fmt::Display for ClipProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingLayer => f.write_str("the clip's layer is missing"),
            Self::Region(problem) => problem.fmt(f),
            Self::NotInActiveLayout => f.write_str("the clip's target isn't in the setup's layout"),
            Self::MixedLayouts => f.write_str("clip targets are in different layouts"),
            Self::MissingTargetLayout => f.write_str("the clip's layout is missing"),
            Self::MissingTarget => f.write_str("the clip's target isn't in its layout"),
            Self::MissingDefinition => f.write_str("the effect is missing"),
            Self::Param(problem) => problem.fmt(f),
        }
    }
}

impl fmt::Display for ParamProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongType(param) => {
                write!(f, "parameter {} has the wrong type", param.as_str())
            }
            Self::MissingRequired(param) => {
                write!(f, "parameter {} needs a value", param.as_str())
            }
            Self::Undeclared(param) => write!(f, "no parameter {}", param.as_str()),
            Self::MissingMarks(param) => {
                write!(f, "parameter {}'s marks are missing", param.as_str())
            }
            Self::MissingCurve(param) => {
                write!(f, "parameter {}'s curve is missing", param.as_str())
            }
            Self::MissingGradient(param) => {
                write!(f, "parameter {}'s gradient is missing", param.as_str())
            }
            Self::InvalidCurve(param, error) => write!(
                f,
                "parameter {} has an invalid curve: {}",
                param.as_str(),
                curve_error(error)
            ),
            Self::InvalidGradient(param, error) => write!(
                f,
                "parameter {} has an invalid gradient: {}",
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
            Self::BeyondSequence => "it runs past the sequence's end",
            Self::StartBeyondClock => "it starts too late",
            Self::DurationBeyondClock => "it is too long",
            Self::RoundsToZero => "it is shorter than a frame",
            Self::EndBeyondAfterRounding => "it runs past the end",
        })
    }
}

impl fmt::Display for AutomationProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MixedLayouts => f.write_str("the sequence's rows are in different layouts"),
            Self::NotInActiveLayout => {
                f.write_str("the envelope's row isn't in the setup's layout")
            }
            Self::MissingRow => f.write_str("the envelope's row is missing"),
            Self::Region(problem) => problem.fmt(f),
            Self::InvalidCurve(error) => {
                write!(f, "invalid envelope: {}", curve_error(error))
            }
            Self::CurveOutOfRange => f.write_str("envelope values leave 0 to 1"),
            Self::WindowOutOfRange => f.write_str("a parameter's range leaves 0 to 1"),
            Self::BindsTwice => f.write_str("the envelope drives one parameter twice"),
            Self::NotAutomatable(param) => {
                write!(f, "parameter {} can't follow an envelope", param.as_str())
            }
            Self::MissingClip => f.write_str("the envelope's clip is missing"),
            Self::MissingNode => f.write_str("the envelope's operator is missing"),
            Self::MissingParam(param) => {
                write!(f, "the envelope's parameter {} is missing", param.as_str())
            }
            Self::Overlap => f.write_str("two envelopes drive one parameter at once"),
        }
    }
}

impl fmt::Display for InputProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MissingDevice => "the input device is missing",
            Self::NotAControl => "the control isn't on its device",
            Self::NotInSetup => "the input device isn't in the setup",
            Self::NotAButton => "an axis can't be pressed or released",
            Self::NotAnAxis => "a button can't move",
        })
    }
}

impl fmt::Display for EventProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(problem) => problem.fmt(f),
            Self::AxisPlays => f.write_str("moving an axis can only set parameters"),
            Self::MissingClip => f.write_str("an action's clip is missing"),
            Self::MissingNode => f.write_str("a Set's operator is missing"),
            Self::UnknownParam(param) => write!(f, "no parameter {}", param.as_str()),
            Self::NotAutomatable(param) => write!(f, "parameter {} can't be set", param.as_str()),
            Self::SetByOtherControl(param) => {
                write!(f, "another control sets parameter {}", param.as_str())
            }
            Self::MissingLiveMarks(name) => write!(f, "no live marks named {}", name.as_str()),
            Self::Copies(count) => write!(f, "{count} copies is outside 1 to {MAX_COPIES}"),
            Self::DurationBeyondClock => f.write_str("a length is too long"),
            Self::NonPositiveDuration => f.write_str("a length must be positive"),
            Self::ShortestAboveLongest => f.write_str("Shortest is longer than Longest"),
            Self::WindowOutOfRange(param) => {
                write!(f, "the range of {} leaves 0 to 1", param.as_str())
            }
        }
    }
}

impl fmt::Display for AdmissionProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GeometryOutOfRange => f.write_str("a fixture is placed too far out"),
            Self::InvalidEncoding(_) => f.write_str("an output's encoding is invalid"),
            Self::Binding(_, error) => f.write_str(&error.message),
            Self::LiveMarksInOperator(_) => {
                f.write_str("operators in a console can't read live marks")
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
            Self::InvalidElement(_) => "the shape's name, placement, size or pixels are invalid",
        })
    }
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyPixels(_) => f.write_str("the target has too many pixels"),
            Self::DuplicateId(item) => write!(f, "layout item id {} is used twice", item.0),
            Self::InvalidName(_, name) => write!(f, "{} is not a valid name", name.as_str()),
            Self::DuplicateName(_, name) => {
                write!(f, "two layout items are named {}", name.as_str())
            }
            Self::MissingDefinition(_) => f.write_str("a fixture definition is missing"),
            Self::InvalidFixture { error, .. } => error.fmt(f),
            Self::InvalidTransform(_) => f.write_str("the item's placement is invalid"),
            Self::MissingFixture(item) => write!(f, "layout item {} is missing", item.0),
            Self::WrongLayout(_) => f.write_str("the target is in another layout"),
            Self::DuplicateMember(_, name) => {
                write!(f, "layout item {} is in one group twice", name.as_str())
            }
            Self::Cycle(_, name) => write!(f, "layout item {} contains itself", name.as_str()),
            Self::Unreachable(_, name) => {
                write!(f, "layout item {} is in no group", name.as_str())
            }
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

/// A route's fixture by name, when its layout has it.
struct Fixture<'a>(Option<&'a Identifier>);

impl fmt::Display for Fixture<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(fixture) => f.write_str(fixture.as_str()),
            None => f.write_str("a fixture"),
        }
    }
}

impl fmt::Display for ControllerValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySenderName => f.write_str("the E1.31 sender name is empty"),
            Self::InvalidPriority(priority) => {
                write!(f, "E1.31 priority {priority} is outside 1–200")
            }
            Self::NoUniverses => f.write_str("the controller sends no universes"),
            Self::InvalidUniverseChannels(channels) => write!(
                f,
                "{channels} channels per universe is outside 1–{DMX_SLOT_LIMIT}"
            ),
            Self::E131UniversesOutOfRange(universes) => write!(
                f,
                "E1.31 universes {}–{} are outside {E131_UNIVERSE_MIN}–{E131_UNIVERSE_MAX}",
                universes.start,
                u32::from(universes.start) + u32::from(universes.count) - 1
            ),
            Self::ArtNetUniversesOutOfRange(universes) => write!(
                f,
                "Art-Net universes {}–{} run past port-address {ARTNET_PORT_ADDRESS_MAX}",
                universes.start,
                u32::from(universes.start) + u32::from(universes.count) - 1
            ),
            Self::DuplicatePort(port) => write!(f, "port id {} is used twice", port.0),
            Self::InvalidPortName(_) => f.write_str("names use lowercase letters, digits and _"),
            Self::DuplicatePortName(_) => f.write_str("two outputs have the same name"),
            Self::EmptyPort(_) => f.write_str("an output has no channels"),
            Self::InvalidDonderOutput { output, .. } => {
                write!(f, "the controller has no output {output}")
            }
            Self::DuplicateDonderOutput(output) => {
                write!(f, "two ports drive output {output}")
            }
        }
    }
}

impl ControllerValidationError {
    /// The port the error concerns.
    fn port(&self) -> Option<ControllerPortId> {
        match self {
            Self::DuplicatePort(port)
            | Self::InvalidPortName(port)
            | Self::DuplicatePortName(port)
            | Self::EmptyPort(port)
            | Self::InvalidDonderOutput { port, .. } => Some(*port),
            Self::EmptySenderName
            | Self::InvalidPriority(_)
            | Self::NoUniverses
            | Self::InvalidUniverseChannels(_)
            | Self::E131UniversesOutOfRange(_)
            | Self::ArtNetUniversesOutOfRange(_)
            | Self::DuplicateDonderOutput(_) => None,
        }
    }
}
