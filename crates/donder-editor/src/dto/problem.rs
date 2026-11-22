use serde::{Deserialize, Serialize};
use specta::Type;

/// Why an edit, or a draft of one, of the open object is refused: the plain
/// message, and the field it concerns when the editor shows one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiProblem {
    pub(crate) message: String,
    pub(crate) site: Option<GuiProblemSite>,
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

/// A field of the open object, or of one of its members; no field for the
/// member as a whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiProblemSite {
    pub(crate) member: GuiMember,
    pub(crate) field: Option<GuiField>,
}

/// The open object, or a member of it a problem concerns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiMember {
    Object,
    /// A sequence's or console's clip.
    Clip {
        id: u32,
    },
    AutomationClip {
        id: u32,
    },
    /// A sequence's mark collection, or a console's live marks.
    MarkCollection {
        key: String,
    },
    Layer {
        id: u32,
    },
    GraphNode {
        node_id: u32,
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
    LayoutItem {
        id: u32,
    },
    Route {
        id: u32,
    },
    Shape {
        id: u32,
    },
}

/// A field of an editor, as its member names it.
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
    Effect,
    Param {
        name: String,
    },
    /// A value each copy of a console clip takes from its press.
    PlayedValue {
        name: String,
    },
    /// The control an event reads, or what a control reads.
    Input,
    Copies,
    Length,
    Layout,
    Fixture,
    Pixels,
    Controller,
    /// A route's controller port, or the port an input device reads.
    Port,
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
    /// A fixture definition the layout places.
    Definition,
    /// A curve's or gradient's points.
    Points,
    /// The declaration an owned object is replaced with.
    Declaration,
}

impl GuiProblemSite {
    pub(crate) fn of(member: GuiMember, field: Option<GuiField>) -> Self {
        Self { member, field }
    }

    /// The open object's `field`.
    pub(crate) fn object(field: GuiField) -> Self {
        Self::of(GuiMember::Object, Some(field))
    }
}
