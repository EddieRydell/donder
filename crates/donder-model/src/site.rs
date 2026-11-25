//! One address for every part of a project: an object, a member of it, and a
//! field of either. Problems name where they lie, repairs where they change,
//! and edits where they write, all by a `Site`.
use donder_runtime_types::Identifier;

use crate::composition::{CompositionGraphNodeId, LayerId, MarkCollectionId};
use crate::console::OnId;
use crate::controller::ControllerPortId;
use crate::effect::ClipId;
use crate::fixture::FixtureElementId;
use crate::identity::ObjectId;
use crate::input_device::InputControlId;
use crate::layout::FixtureId;
use crate::patch::PixelRouteId;
use crate::sequence::AutomationTarget;

/// An object, one of its members, and a field of the member or object.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Site {
    pub object: ObjectId,
    pub member: Option<Member>,
    pub field: Option<Field>,
}

impl Site {
    /// The object as a whole.
    pub fn object(object: impl Into<ObjectId>) -> Self {
        Self {
            object: object.into(),
            member: None,
            field: None,
        }
    }

    /// A member of `object`.
    pub fn member(object: impl Into<ObjectId>, member: Member) -> Self {
        Self {
            member: Some(member),
            ..Self::object(object)
        }
    }

    /// The same place, at `field`.
    pub fn at(self, field: Field) -> Self {
        Self {
            field: Some(field),
            ..self
        }
    }
}

/// A part of an object with an id of its own.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Member {
    Layer(LayerId),
    Clip(ClipId),
    /// A sequence's mark collection, or a console's live marks.
    Marks(MarkCollectionId),
    Node(CompositionGraphNodeId),
    Event(OnId),
    Control(InputControlId),
    Port(ControllerPortId),
    Route(PixelRouteId),
    LayoutItem(FixtureId),
    Shape(FixtureElementId),
}

impl Member {
    /// What a person calls this kind of member.
    pub fn noun(&self) -> &'static str {
        match self {
            Self::Layer(_) => "layer",
            Self::Clip(_) => "clip",
            Self::Marks(_) => "mark collection",
            Self::Node(_) => "graph node",
            Self::Event(_) => "control event",
            Self::Control(_) => "control",
            Self::Port(_) => "port",
            Self::LayoutItem(_) => "fixture",
            Self::Route(_) => "output",
            Self::Shape(_) => "shape",
        }
    }
}

/// A field of an object or member.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Field {
    Name,
    Description,
    Color,
    Start,
    Duration,
    FrameRate,
    Layer,
    /// What a clip plays on, or the fixture a route drives.
    Target,
    Scope,
    /// The effect a clip plays, or the fixture definition a layout item places.
    Definition,
    /// A parameter value of a clip or operator.
    Param(Identifier),
    /// A value each copy of a console clip takes from its press.
    Played(Identifier),
    /// An envelope's binding to a parameter.
    Binding(AutomationTarget),
    /// An event's `Set`s of a parameter.
    Set(Identifier),
    /// The clips a console plays for the whole session.
    Always,
    /// The control an event reads, or what a control reads.
    Input,
    Copies,
    Length,
    Pixels,
    FirstChannel,
    ColorOrder,
    Gamma,
    Brightness,
    Destination,
    Interface,
    SenderName,
    Priority,
    UniverseStart,
    UniverseCount,
    UniverseChannels,
    /// Where FSEQ files place a controller's first channel.
    FseqChannel,
    Device,
    /// A controller port's channels.
    Channels,
    /// A layout item's position, rotation and scale.
    Placement,
    /// A curve's or gradient's points, or a mark collection's marks.
    Points,
    Controller,
    /// A route's controller port, or the port an input device listens on.
    Port,
    /// A sequence's audio file.
    Audio,
}
