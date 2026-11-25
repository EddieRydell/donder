//! The project rules an object can break. Each sentence a person reads about
//! one is written here, once, by `Display`.
use std::fmt;

use crate::composition::{CompositionGraphNodeId, LayerId, MarkCollectionId};
use crate::console::{MAX_COPIES, OnId};
use crate::controller::{
    ARTNET_PORT_ADDRESS_MAX, ChannelPlace, ControllerOutput, ControllerPortId,
    ControllerValidationError, DMX_SLOT_LIMIT, E131_UNIVERSE_MAX, E131_UNIVERSE_MIN,
};
use crate::effect::{ClipId, DefinitionProblem};
use crate::fixture::{FixtureDefinitionError, FixtureGeometryError};
use crate::identity::{ObjectId, ObjectKind};
use crate::input_device::InputDeviceValidationError;
use crate::layout::{FixtureId, LayoutError};
use crate::operator::GraphValidationError;
use crate::patch::PixelRouteId;
use crate::patch::PixelSpan;
use crate::sequence::AutomationTarget;
use crate::site::{Field, Member, Site};
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
    Curve(CurveValidationError),
    /// A stored curve's values are 0 to 1; a parameter maps them onto its
    /// range when the curve is bound.
    CurveNotNormalized,
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
    Project(ProjectProblem),
    /// A sequence's or console's clock.
    Timing(TimingProblem),
    Composition(CompositionProblem),
    Console(ConsoleValidationError),
    Admission(AdmissionProblem),
}

/// A patch output that cannot be played as written.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RouteProblem {
    DuplicateId,
    InvalidColorOrder,
    InvalidGamma,
    InvalidBrightness,
    TargetNotFixture,
    Target(LayoutError),
    EmptySpan,
    SpanExceedsTarget {
        span: PixelSpan,
        /// The fixture's pixels.
        pixels: u32,
    },
    MissingController,
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

/// A rule of the project's own members.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectProblem {
    MissingLayout,
    TooManyControllers,
    SharedOscPort(u16),
}

/// A console that cannot be played as written.
#[derive(Clone, Debug, PartialEq)]
pub enum ConsoleValidationError {
    DuplicateEventId(OnId),
    /// `Play` actions play the clip, and `Start`, `Toggle` or `always` hold
    /// it too.
    PlayedAndHeld {
        clip: ClipId,
        name: Identifier,
    },
    MissingAlwaysClip,
    RepeatedAlwaysClip {
        clip: ClipId,
        name: Identifier,
    },
    /// The length `always` repeats a clip every is not positive or leaves
    /// the playback clock.
    AlwaysLength {
        clip: ClipId,
        name: Identifier,
        problem: EventProblem,
    },
    Event {
        event: OnId,
        problem: EventProblem,
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
    Name(NameProblem),
    DuplicateClipId(ClipId),
    Clip {
        clip: ClipId,
        name: Identifier,
        problem: ClipProblem,
    },
    MarkOutsideDuration(MarkCollectionId, Identifier),
    InvalidMarkLabel(MarkCollectionId, Identifier),
    /// Clips play on layers, so a sequence or console keeps at least one.
    NoLayers,
    DuplicateLayerId(LayerId),
    Graph(GraphValidationError),
    MissingLayer(CompositionGraphNodeId),
    RepeatedLayer(CompositionGraphNodeId),
    Param {
        node: CompositionGraphNodeId,
        problem: ParamProblem,
    },
    /// A console's mark collection holds marks; `Mark` actions add them live.
    LiveMarksHold(MarkCollectionId),
}

/// A sequence or console clip.
#[derive(Clone, Debug, PartialEq)]
pub enum ClipProblem {
    MissingLayer,
    Region(RegionProblem),
    /// A sequence's clip is played, but only a console's actions play one.
    NeverPlays,
    /// A console's envelope plays on the session's clock, so it has a start.
    EnvelopePlayed,
    /// A value each copy takes from its press.
    Played(Identifier, PlayedProblem),
    Envelope(EnvelopeProblem),
    MissingTarget,
    Definition(DefinitionProblem),
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
    InvalidText(Identifier),
    InvalidFontHeight(Identifier),
    InvalidImageSize(Identifier),
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
pub enum EnvelopeProblem {
    InvalidCurve(CurveValidationError),
    CurveOutOfRange,
    BindsTwice,
    /// Another envelope drives one of its parameters at the same time.
    Overlap,
    /// A console's `Set` also sets the parameter it binds.
    AlsoSet(AutomationTarget),
    Binding(AutomationTarget, BindingProblem),
}

/// What an envelope binds or a `Set` sets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingProblem {
    WindowOutOfRange,
    NotAutomatable,
    /// It names no timed effect clip; a `Set`, no played one.
    MissingClip,
    MissingNode,
    MissingParam,
}

/// The control an event plays from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputProblem {
    MissingDevice,
    NotAControl,
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
    MissingLiveMarks,
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
    UnplacedRoute(PixelRouteId),
    Binding(Member, BindingError),
    LiveMarksInOperator(CompositionGraphNodeId),
    TooManyParams,
}

/// Where in an object a problem lies: the member, and the field of it or of
/// the object.
type Place = (Option<Member>, Option<Field>);

impl ProjectValidationError {
    /// The object, member and field the problem concerns.
    pub fn site(&self) -> Site {
        let (member, field) = self.problem.place();
        Site {
            object: self.object,
            member,
            field,
        }
    }
}

impl ValidationProblem {
    fn place(&self) -> Place {
        match self {
            Self::Curve(_) | Self::CurveNotNormalized | Self::Gradient(_) => {
                (None, Some(Field::Points))
            }
            Self::Fixture(error) => match error {
                FixtureDefinitionError::TooManyPixels(_) => (None, None),
                FixtureDefinitionError::DuplicateElement { element, .. }
                | FixtureDefinitionError::InvalidElement { element, .. } => {
                    (Some(Member::Shape(*element)), None)
                }
            },
            Self::Layout(error) => (
                error.item().map(Member::LayoutItem),
                match error {
                    LayoutError::InvalidName(..) | LayoutError::DuplicateName(..) => {
                        Some(Field::Name)
                    }
                    LayoutError::InvalidTransform(_) => Some(Field::Placement),
                    LayoutError::MissingDefinition(_) => Some(Field::Definition),
                    _ => None,
                },
            ),
            Self::Controller(error) => (error.port().map(Member::Port), error.field()),
            Self::InputDevice(error) => (
                error.control().map(Member::Control),
                match error {
                    InputDeviceValidationError::DuplicateControl(_) => None,
                    InputDeviceValidationError::InvalidControlName(..)
                    | InputDeviceValidationError::DuplicateControlName(..) => Some(Field::Name),
                    InputDeviceValidationError::DuplicateInput { .. }
                    | InputDeviceValidationError::ForeignInput(..)
                    | InputDeviceValidationError::InputOutOfRange(..)
                    | InputDeviceValidationError::InvalidAddress(..)
                    | InputDeviceValidationError::InvalidAxisRange(..) => Some(Field::Input),
                    InputDeviceValidationError::InvalidOscPort => Some(Field::Port),
                },
            ),
            Self::Route { route, problem, .. } => (Some(Member::Route(*route)), problem.field()),
            Self::Timing(timing) => (None, Some(timing.field())),
            Self::Composition(problem) => problem.place(),
            Self::Console(error) => error.place(),
            Self::Admission(AdmissionProblem::Binding(member, _)) => (Some(member.clone()), None),
            Self::Admission(AdmissionProblem::LiveMarksInOperator(node)) => {
                (Some(Member::Node(node.clone())), None)
            }
            Self::Admission(AdmissionProblem::UnplacedRoute(route)) => {
                (Some(Member::Route(*route)), None)
            }
            Self::Project(_)
            | Self::Admission(
                AdmissionProblem::GeometryOutOfRange | AdmissionProblem::TooManyParams,
            ) => (None, None),
        }
    }
}

impl RouteProblem {
    fn field(&self) -> Option<Field> {
        Some(match self {
            Self::DuplicateId => return None,
            Self::InvalidColorOrder => Field::ColorOrder,
            Self::InvalidGamma => Field::Gamma,
            Self::InvalidBrightness => Field::Brightness,
            Self::TargetNotFixture | Self::Target(_) => Field::Target,
            Self::EmptySpan | Self::SpanExceedsTarget { .. } => Field::Pixels,
            Self::MissingController => Field::Controller,
            Self::MissingPort => Field::Port,
            Self::ChannelOverflow | Self::PastLastChannel { .. } | Self::OutputsOverlap { .. } => {
                Field::FirstChannel
            }
        })
    }
}

impl TimingProblem {
    fn field(self) -> Field {
        match self {
            Self::ZeroFrameRate | Self::FrameRateAboveLimit => Field::FrameRate,
            Self::ZeroDuration
            | Self::DurationBeyondClock
            | Self::DurationRoundsToZero
            | Self::TooManyFrames => Field::Duration,
        }
    }
}

impl ConsoleValidationError {
    fn place(&self) -> Place {
        match self {
            Self::PlayedAndHeld { clip, .. } | Self::RepeatedAlwaysClip { clip, .. } => {
                (Some(Member::Clip(*clip)), None)
            }
            Self::AlwaysLength { clip, .. } => (Some(Member::Clip(*clip)), Some(Field::Length)),
            Self::MissingAlwaysClip => (None, Some(Field::Always)),
            Self::DuplicateEventId(event) => (Some(Member::Event(*event)), None),
            Self::Event { event, problem } => (
                Some(Member::Event(*event)),
                match problem {
                    EventProblem::Input(_) => Some(Field::Input),
                    EventProblem::UnknownParam(param)
                    | EventProblem::NotAutomatable(param)
                    | EventProblem::SetByOtherControl(param)
                    | EventProblem::WindowOutOfRange(param) => Some(Field::Set(param.clone())),
                    EventProblem::Copies(_) => Some(Field::Copies),
                    EventProblem::DurationBeyondClock
                    | EventProblem::NonPositiveDuration
                    | EventProblem::ShortestAboveLongest => Some(Field::Length),
                    EventProblem::MissingClip => Some(Field::Definition),
                    EventProblem::AxisPlays
                    | EventProblem::MissingNode
                    | EventProblem::MissingLiveMarks => None,
                },
            ),
        }
    }
}

impl NameProblem {
    /// The member whose name breaks the rule.
    fn member(&self) -> &Member {
        let (Self::NotSnakeCase(member, _) | Self::Duplicate(member, _)) = self;
        member
    }
}

impl CompositionProblem {
    fn place(&self) -> Place {
        match self {
            Self::DuplicateClipId(clip) => (Some(Member::Clip(*clip)), None),
            Self::Clip { clip, problem, .. } => (Some(Member::Clip(*clip)), problem.field()),
            Self::NoLayers => (None, None),
            Self::DuplicateLayerId(layer) => (Some(Member::Layer(layer.clone())), None),
            Self::Graph(error) => (error.node().map(Member::Node), None),
            Self::MissingLayer(node) | Self::RepeatedLayer(node) => {
                (Some(Member::Node(node.clone())), None)
            }
            Self::Param { node, problem } => (
                Some(Member::Node(node.clone())),
                Some(Field::Param(problem.param().clone())),
            ),
            Self::Name(problem) => (Some(problem.member().clone()), Some(Field::Name)),
            Self::MarkOutsideDuration(marks, _)
            | Self::InvalidMarkLabel(marks, _)
            | Self::LiveMarksHold(marks) => (Some(Member::Marks(*marks)), Some(Field::Points)),
        }
    }
}

impl ClipProblem {
    fn field(&self) -> Option<Field> {
        Some(match self {
            Self::NeverPlays | Self::EnvelopePlayed => return None,
            Self::Played(param, _) => Field::Played(param.clone()),
            Self::Envelope(EnvelopeProblem::InvalidCurve(_) | EnvelopeProblem::CurveOutOfRange) => {
                Field::Points
            }
            Self::Envelope(
                EnvelopeProblem::Binding(target, _) | EnvelopeProblem::AlsoSet(target),
            ) => Field::Binding(target.clone()),
            Self::Envelope(EnvelopeProblem::BindsTwice | EnvelopeProblem::Overlap) => return None,
            Self::MissingLayer => Field::Layer,
            Self::Region(RegionProblem::StartBeyondClock) => Field::Start,
            Self::Region(_) => Field::Duration,
            Self::Definition(_) => Field::Definition,
            Self::MissingTarget => Field::Target,
            Self::Param(problem) => Field::Param(problem.param().clone()),
        })
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
            | Self::InvalidGradient(param, _)
            | Self::InvalidText(param)
            | Self::InvalidFontHeight(param)
            | Self::InvalidImageSize(param) => param,
        }
    }
}

impl fmt::Display for ValidationProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Curve(error) => write!(f, "invalid curve: {}", curve_error(error)),
            Self::CurveNotNormalized => f.write_str("a curve's values are 0 to 1"),
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
            Self::Project(problem) => problem.fmt(f),
            Self::Timing(problem) => problem.fmt(f),
            Self::Composition(problem) => problem.fmt(f),
            Self::Console(error) => error.fmt(f),
            Self::Admission(problem) => problem.fmt(f),
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
            Self::TargetNotFixture => f.write_str("it targets a group, not a fixture"),
            Self::Target(error) => error.fmt(f),
            Self::EmptySpan => f.write_str("it routes no pixels"),
            Self::SpanExceedsTarget { span, pixels } => write!(
                f,
                "pixels {} run past the fixture's {pixels}",
                Span(span.start..span.start.saturating_add(span.count))
            ),
            Self::MissingController => f.write_str("its controller is missing"),
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

impl fmt::Display for ProjectProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingLayout => f.write_str("the project's layout is missing"),
            Self::TooManyControllers => {
                f.write_str("the project has more controllers than outputs can address")
            }
            Self::SharedOscPort(port) => {
                write!(f, "two OSC input devices listen on port {port}")
            }
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

impl fmt::Display for ConsoleValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateEventId(event) => write!(f, "event id {} is used twice", event.0),
            Self::PlayedAndHeld { name, .. } => write!(
                f,
                "clip {} is played by Play and held by Start, Toggle or Always",
                name.as_str()
            ),
            Self::MissingAlwaysClip => f.write_str("an Always names no played clip"),
            Self::RepeatedAlwaysClip { name, .. } => {
                write!(f, "clip {} is set to Always twice", name.as_str())
            }
            Self::AlwaysLength { name, problem, .. } => {
                write!(f, "Always clip {}: {problem}", name.as_str())
            }
            Self::Event { problem, .. } => problem.fmt(f),
        }
    }
}

impl fmt::Display for CompositionProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateClipId(clip) => write!(f, "clip id {} is used twice", clip.0),
            Self::Clip { name, problem, .. } => write!(f, "clip {}: {problem}", name.as_str()),
            Self::Name(problem) => problem.fmt(f),
            Self::MarkOutsideDuration(_, name) => {
                write!(
                    f,
                    "a mark of {} ends past the sequence's end",
                    name.as_str()
                )
            }
            Self::InvalidMarkLabel(_, name) => write!(
                f,
                "a mark label of {} is empty, spans lines or has outer spaces",
                name.as_str()
            ),
            Self::NoLayers => f.write_str("there must be at least one layer"),
            Self::DuplicateLayerId(layer) => write!(f, "layer id {} is used twice", layer.0),
            Self::Graph(error) => error.fmt(f),
            Self::MissingLayer(_) => f.write_str("a layer in the graph is missing"),
            Self::RepeatedLayer(_) => f.write_str("the layer is in the graph twice"),
            Self::Param { problem, .. } => problem.fmt(f),
            Self::LiveMarksHold(_) => {
                f.write_str("a console's marks start empty; Mark actions add them")
            }
        }
    }
}

impl fmt::Display for ClipProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NeverPlays => f.write_str("only a console plays a clip without a start"),
            Self::EnvelopePlayed => f.write_str("an envelope needs a start"),
            Self::Envelope(problem) => problem.fmt(f),
            Self::Played(param, problem) => {
                let param = param.as_str();
                match problem {
                    PlayedProblem::Twice => write!(f, "plays {param} twice"),
                    PlayedProblem::UnknownParam => {
                        write!(f, "its effect has no parameter {param}")
                    }
                    PlayedProblem::NotAutomatable => {
                        write!(f, "can't play parameter {param}")
                    }
                    PlayedProblem::WindowOutOfRange => {
                        write!(f, "the range of {param} leaves 0 to 1")
                    }
                    PlayedProblem::WithoutPress => {
                        write!(f, "plays {param} but no press starts it")
                    }
                    PlayedProblem::WithoutNotes => {
                        write!(f, "plays {param} from a note no control sends")
                    }
                    PlayedProblem::AlsoSet => {
                        write!(f, "plays {param}, which a Set also writes")
                    }
                }
            }
            Self::MissingLayer => f.write_str("the clip's layer is missing"),
            Self::Region(problem) => problem.fmt(f),
            Self::MissingTarget => f.write_str("the clip's target isn't in the layout"),
            Self::Definition(problem) => problem.fmt(f),
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
            Self::InvalidText(param) => write!(
                f,
                "parameter {}'s text has more than {} characters or a NUL",
                param.as_str(),
                donder_runtime_types::TEXT_LENGTH
            ),
            Self::InvalidFontHeight(param) => write!(
                f,
                "parameter {}'s font height is not 1 to {} pixels",
                param.as_str(),
                crate::FontFace::MAX_HEIGHT
            ),
            Self::InvalidImageSize(param) => write!(
                f,
                "parameter {}'s image size is not 1 to {} pixels",
                param.as_str(),
                crate::ImageSource::MAX_SIZE
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

impl fmt::Display for EnvelopeProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCurve(error) => {
                write!(f, "invalid envelope: {}", curve_error(error))
            }
            Self::CurveOutOfRange => f.write_str("envelope values leave 0 to 1"),
            Self::BindsTwice => f.write_str("the envelope drives one parameter twice"),
            Self::Overlap => f.write_str("two envelopes drive one parameter at once"),
            Self::AlsoSet(target) => write!(f, "a control also sets {}", target.param().as_str()),
            Self::Binding(target, problem) => {
                let param = target.param().as_str();
                match problem {
                    BindingProblem::WindowOutOfRange => {
                        write!(f, "the range of {param} leaves 0 to 1")
                    }
                    BindingProblem::NotAutomatable => {
                        write!(f, "parameter {param} can't follow an envelope")
                    }
                    BindingProblem::MissingClip => f.write_str("the envelope's clip is missing"),
                    BindingProblem::MissingNode => {
                        f.write_str("the envelope's operator is missing")
                    }
                    BindingProblem::MissingParam => {
                        write!(f, "the envelope's parameter {param} is missing")
                    }
                }
            }
        }
    }
}

impl fmt::Display for InputProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MissingDevice => "the input device is missing",
            Self::NotAControl => "the control isn't on its device",
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
            Self::MissingClip => f.write_str("an action names no played clip"),
            Self::MissingNode => f.write_str("a Set's operator is missing"),
            Self::UnknownParam(param) => write!(f, "no parameter {}", param.as_str()),
            Self::NotAutomatable(param) => write!(f, "parameter {} can't be set", param.as_str()),
            Self::SetByOtherControl(param) => {
                write!(f, "another control sets parameter {}", param.as_str())
            }
            Self::MissingLiveMarks => f.write_str("its live marks are missing"),
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
            Self::UnplacedRoute(_) => f.write_str("an output's channels cannot be placed"),
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
            Self::Layout => "layout",
            Self::Controller => "controller",
            Self::InputDevice => "input device",
            Self::Sequence => "sequence",
            Self::Console => "console",
            Self::FixtureDefinition => "fixture definition",
            Self::Curve => "curve",
            Self::Gradient => "gradient",
            Self::Script => "script",
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
            Self::InvalidTransform(_) => f.write_str("the item's placement is invalid"),
            Self::MissingFixture(item) => write!(f, "layout item {} is missing", item.0),
            Self::DuplicateMember(_, name) => {
                write!(f, "layout item {} is in one group twice", name.as_str())
            }
            Self::Cycle(_, name) => write!(f, "layout item {} contains itself", name.as_str()),
            Self::Unreachable(_, name) => {
                write!(f, "layout item {} is in no group", name.as_str())
            }
            Self::InvalidSlice { .. } => f.write_str("a slice must name a placed fixture"),
            Self::SliceOutOfRange { .. } => {
                f.write_str("a slice must select pixels its fixture has")
            }
        }
    }
}

impl LayoutError {
    /// The item the error concerns, when it names one of the layout's items.
    fn item(&self) -> Option<FixtureId> {
        match self {
            Self::TooManyPixels(item)
            | Self::DuplicateId(item)
            | Self::InvalidName(item, _)
            | Self::DuplicateName(item, _)
            | Self::InvalidTransform(item)
            | Self::DuplicateMember(item, _)
            | Self::Cycle(item, _)
            | Self::Unreachable(item, _)
            | Self::InvalidSlice { group: item, .. }
            | Self::SliceOutOfRange { group: item, .. }
            | Self::MissingDefinition(item) => Some(*item),
            Self::MissingFixture(_) => None,
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
            Self::EmptyPortName(_) => f.write_str("a port has no name"),
            Self::DuplicatePortName(_) => f.write_str("two ports have the same name"),
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
            | Self::EmptyPortName(port)
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

    fn field(&self) -> Option<Field> {
        Some(match self {
            Self::EmptySenderName => Field::SenderName,
            Self::InvalidPriority(_) => Field::Priority,
            Self::NoUniverses => Field::UniverseCount,
            Self::InvalidUniverseChannels(_) => Field::UniverseChannels,
            Self::E131UniversesOutOfRange(_) | Self::ArtNetUniversesOutOfRange(_) => {
                Field::UniverseStart
            }
            Self::EmptyPortName(_) | Self::DuplicatePortName(_) => Field::Name,
            Self::EmptyPort(_) => Field::Channels,
            Self::DuplicatePort(_)
            | Self::InvalidDonderOutput { .. }
            | Self::DuplicateDonderOutput(_) => return None,
        })
    }
}

impl fmt::Display for DefinitionProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Missing => "the definition is missing",
            Self::NotAnEffect => "an operator is not an effect",
            Self::NotAnOperator => "an effect is not an operator",
        })
    }
}
