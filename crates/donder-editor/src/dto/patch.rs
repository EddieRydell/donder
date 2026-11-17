use crate::dto::*;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PatchGuiDocument {
    /// The open object's description, edited with `GuiEditCommand::Description`.
    pub(crate) description: Option<String>,
    pub(crate) path: String,
    pub(crate) object_key: String,
    pub(crate) routes: Vec<GuiPixelRoute>,
    /// Where each valid route's channels land.
    pub(crate) placements: Vec<GuiRoutePlacement>,
    pub(crate) layouts: Vec<PatchLayout>,
    pub(crate) controllers: Vec<SetupController>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PatchLayout {
    pub(crate) source_ref: GuiObjectRef,
    pub(crate) fixtures: Vec<PatchFixtureTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PatchFixtureTarget {
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) pixel_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiPixelRoute {
    pub id: u32,
    pub layout: GuiObjectRef,
    pub fixture: u32,
    pub pixels: Option<GuiPixelSpan>,
    pub controller: GuiObjectRef,
    /// A Donder controller's port; none for an E1.31 or Art-Net controller.
    pub port: Option<u32>,
    /// From 0: across an E1.31 or Art-Net controller's universes, or in the
    /// Donder port.
    pub start_channel: u32,
    pub encoding: GuiPixelEncoding,
    pub gamma: f32,
    pub brightness: f32,
    pub dither: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiRoutePlacement {
    pub(crate) route: u32,
    pub(crate) place: GuiChannelPlace,
}

/// Where a route's channels land, as people count, from 1.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiChannelPlace {
    Universes {
        first_universe: u16,
        first_channel: u16,
        last_universe: u16,
        last_channel: u16,
    },
    Port {
        port: String,
        first: u32,
        last: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiPixelSpan {
    pub(crate) start: u32,
    pub(crate) count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum GuiPixelEncoding {
    Rgb { order: [u8; 3] },
    Rgbw { order: [u8; 4] },
}
