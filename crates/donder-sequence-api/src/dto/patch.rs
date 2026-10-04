use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PatchGuiDocument {
    pub path: String,
    pub object_key: String,
    pub routes: Vec<GuiPixelRoute>,
    pub layouts: Vec<PatchLayout>,
    pub controllers: Vec<SetupController>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PatchLayout {
    pub source_ref: GuiObjectRef,
    pub fixtures: Vec<PatchFixtureTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PatchFixtureTarget {
    pub id: u32,
    pub name: String,
    pub pixel_count: u32,
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
    pub start: u32,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum GuiPixelEncoding {
    Rgb { order: [u8; 3] },
    Rgbw { order: [u8; 4] },
}
