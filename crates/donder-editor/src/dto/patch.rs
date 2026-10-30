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
    pub port: u32,
    pub start_slot: u16,
    pub encoding: GuiPixelEncoding,
    pub gamma: f32,
    pub brightness: f32,
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
