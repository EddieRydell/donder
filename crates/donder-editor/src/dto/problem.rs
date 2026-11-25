use serde::{Deserialize, Serialize};
use specta::Type;

use super::{GuiObjectRef, SequenceAutomationTarget};

/// Why an edit, or a draft of one, of the open object is refused: the plain
/// message, and the site it concerns when the editor shows one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiProblem {
    pub(crate) message: String,
    pub(crate) site: Option<GuiSite>,
}

impl GuiProblem {
    /// A problem no field of the editor shows; the status bar says it.
    pub fn without_site(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            site: None,
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

/// A place in a project: the model's [`donder_model::Site`]. No object is the
/// open object, no member the object itself, and no field the member or
/// object as a whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiSite {
    pub(crate) object: Option<GuiObjectRef>,
    pub(crate) member: Option<GuiMember>,
    pub(crate) field: Option<GuiField>,
}

/// A member of the open object, by id. A sequence's and a console's clips
/// are both `Clip`: the open object tells them apart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiMember {
    Layer {
        id: u32,
    },
    Clip {
        id: u32,
    },
    /// A sequence's mark collection, or a console's live marks.
    Marks {
        id: u32,
    },
    Node {
        id: u32,
    },
    Event {
        id: u32,
    },
    Control {
        id: u32,
    },
    Port {
        id: u32,
    },
    Route {
        id: u32,
    },
    LayoutItem {
        id: u32,
    },
    Shape {
        id: u32,
    },
}

/// A field of a member or of the open object; see [`donder_model::Field`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiField {
    Name,
    Description,
    Color,
    Start,
    Duration,
    FrameRate,
    Layer,
    Target,
    Scope,
    Definition,
    Param { name: String },
    Played { name: String },
    Binding { target: SequenceAutomationTarget },
    Set { name: String },
    Always,
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
    FseqChannel,
    Device,
    Channels,
    Placement,
    Points,
    Controller,
    Port,
    Audio,
}

impl GuiSite {
    pub fn of(member: Option<GuiMember>, field: Option<GuiField>) -> Self {
        Self {
            object: None,
            member,
            field,
        }
    }

    /// The open object's `field`.
    pub fn object(field: GuiField) -> Self {
        Self::of(None, Some(field))
    }
}

impl From<&donder_model::Site> for GuiSite {
    fn from(site: &donder_model::Site) -> Self {
        Self {
            object: None,
            member: site.member.as_ref().map(GuiMember::from),
            field: site.field.as_ref().map(GuiField::from),
        }
    }
}

impl From<&donder_model::Member> for GuiMember {
    fn from(member: &donder_model::Member) -> Self {
        use donder_model::Member;
        match member {
            Member::Layer(id) => Self::Layer { id: id.0 },
            Member::Clip(id) => Self::Clip { id: id.0 },
            Member::Marks(id) => Self::Marks { id: id.0 },
            Member::Node(id) => Self::Node { id: id.0 },
            Member::Event(id) => Self::Event { id: id.0 },
            Member::Control(id) => Self::Control { id: id.0 },
            Member::Port(id) => Self::Port { id: id.0 },
            Member::Route(id) => Self::Route { id: id.0 },
            Member::LayoutItem(id) => Self::LayoutItem { id: id.0 },
            Member::Shape(id) => Self::Shape { id: id.0 },
        }
    }
}

impl From<&donder_model::Field> for GuiField {
    fn from(field: &donder_model::Field) -> Self {
        use donder_model::Field;
        let name = |name: &donder_runtime_types::Identifier| name.as_str().to_string();
        match field {
            Field::Name => Self::Name,
            Field::Description => Self::Description,
            Field::Color => Self::Color,
            Field::Start => Self::Start,
            Field::Duration => Self::Duration,
            Field::FrameRate => Self::FrameRate,
            Field::Layer => Self::Layer,
            Field::Target => Self::Target,
            Field::Scope => Self::Scope,
            Field::Definition => Self::Definition,
            Field::Param(param) => Self::Param { name: name(param) },
            Field::Played(param) => Self::Played { name: name(param) },
            Field::Binding(target) => Self::Binding {
                target: crate::gui::automation_target_to_gui(target),
            },
            Field::Set(param) => Self::Set { name: name(param) },
            Field::Always => Self::Always,
            Field::Input => Self::Input,
            Field::Copies => Self::Copies,
            Field::Length => Self::Length,
            Field::Pixels => Self::Pixels,
            Field::FirstChannel => Self::FirstChannel,
            Field::ColorOrder => Self::ColorOrder,
            Field::Gamma => Self::Gamma,
            Field::Brightness => Self::Brightness,
            Field::Destination => Self::Destination,
            Field::Interface => Self::Interface,
            Field::SenderName => Self::SenderName,
            Field::Priority => Self::Priority,
            Field::UniverseStart => Self::UniverseStart,
            Field::UniverseCount => Self::UniverseCount,
            Field::UniverseChannels => Self::UniverseChannels,
            Field::FseqChannel => Self::FseqChannel,
            Field::Device => Self::Device,
            Field::Channels => Self::Channels,
            Field::Placement => Self::Placement,
            Field::Points => Self::Points,
            Field::Controller => Self::Controller,
            Field::Port => Self::Port,
            Field::Audio => Self::Audio,
        }
    }
}
