use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiFixtureDefinition {
    pub functions: Vec<GuiFixtureFunction>,
    pub channels: Vec<GuiFixtureChannel>,
    pub behavior_rules: Vec<GuiFixtureBehavior>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiFixtureFunction {
    pub id: u32,
    pub name: String,
    pub tag: Option<GuiFixtureFunctionTag>,
    pub kind: GuiFixtureFunctionKind,
    pub curve: GuiDimmingCurve,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum GuiFixtureFunctionTag {
    Pan,
    Tilt,
    Dimmer,
    Shutter,
    Zoom,
    Gobo,
    Frost,
    Prism,
    ColorWheel,
    ColorMixing,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiFixtureFunctionKind {
    Range,
    Indexed { entries: Vec<GuiFixtureEntry> },
    ColorWheel { entries: Vec<GuiFixtureEntry> },
    ColorMixing { model: GuiFixtureColorModel },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum GuiFixtureColorModel {
    Rgb,
    Rgbw,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiFixtureEntry {
    pub id: u32,
    pub name: String,
    pub dmx_min: u16,
    pub dmx_max: u16,
    pub curve_control: bool,
    pub color: Option<String>,
    pub tag: Option<GuiFixtureEntryTag>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum GuiFixtureEntryTag {
    ShutterOpen,
    ShutterClosed,
    Strobe,
    PrismOpen,
    PrismClosed,
    GoboOpen,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiFixtureChannel {
    pub slot: u16,
    pub role: GuiFixtureChannelRole,
    pub curve: GuiDimmingCurve,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiFixtureChannelRole {
    Coarse {
        function: u32,
    },
    Fine {
        function: u32,
    },
    ColorComponent {
        function: u32,
        component: GuiFixtureColorComponent,
    },
    Ignored,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum GuiFixtureColorComponent {
    Red,
    Green,
    Blue,
    White,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiFixtureBehavior {
    Shutter {
        function: u32,
        closed: u32,
        open: u32,
    },
    Dimmer {
        function: u32,
        off: f32,
        on: f32,
    },
    ColorWheel {
        function: u32,
        entries: Vec<GuiFixtureColorMapping>,
    },
    PrismGate {
        function: u32,
        disabled: u32,
        enabled: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiFixtureColorMapping {
    pub color: String,
    pub entry: u32,
}
