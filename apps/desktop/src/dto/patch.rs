use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PatchGuiDocument {
    pub path: String,
    pub object_key: String,
    pub nodes: Vec<PatchGuiNode>,
    pub edges: Vec<SetupPatchEdge>,
    pub element_trees: Vec<PatchElementTree>,
    pub controllers: Vec<SetupController>,
    pub profiles: Vec<GuiObjectRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PatchElementTree {
    pub source_ref: GuiObjectRef,
    pub elements: Vec<ElementCellOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ElementCellOption {
    pub id: u32,
    pub name: String,
    pub cell_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PatchGuiNode {
    pub id: u32,
    pub definition: PatchGuiNodeDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PatchGuiNodeDefinition {
    Source {
        tree: GuiObjectRef,
        node: u32,
        cells: Option<PatchGuiCellRange>,
        output: PatchGuiValueType,
    },
    Filter {
        filter: PatchGuiFilter,
    },
    Sink {
        controller: GuiObjectRef,
        port: u32,
        start_slot: u16,
        slot_count: u16,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PatchGuiCellRange {
    pub start: u32,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PatchGuiValueType {
    Color { width: u32 },
    Scalar { width: u32 },
    Indexed { width: u32 },
    FixtureState { width: u32, profile: GuiObjectRef },
    Components { width: u32 },
    Slots { width: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PatchGuiFilter {
    ScalarToComponents {
        width: u32,
    },
    ColorBreakdown {
        capability: GuiColorCapability,
        cell_count: u32,
    },
    DimmingCurve {
        curve: GuiDimmingCurve,
        width: u32,
    },
    ScaleInvert {
        scale: f32,
        invert: bool,
        width: u32,
    },
    FanOut {
        width: u32,
        outputs: u16,
    },
    ComponentReorder {
        components_per_cell: u16,
        order: Vec<u16>,
        cell_count: u32,
    },
    IndexedValueMapping {
        entries: Vec<PatchGuiIndexedEntry>,
        width: u32,
    },
    Quantize8 {
        width: u32,
    },
    Quantize16 {
        width: u32,
        byte_order: GuiByteOrder,
    },
    FixtureProfileEncoding {
        profile: GuiObjectRef,
        fixture_count: u32,
        slot_count: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum GuiByteOrder {
    CoarseFine,
    FineCoarse,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PatchGuiIndexedEntry {
    pub id: u32,
    pub value: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiDimmingCurve {
    Linear,
    Gamma { exponent: f32 },
    Custom { points: Vec<SequenceCurvePoint> },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiColorCapability {
    Rgb,
    Rgbw,
    Discrete {
        emitters: Vec<GuiDiscreteEmitter>,
        mappings: Vec<GuiDiscreteColorMapping>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiDiscreteEmitter {
    pub id: u32,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiDiscreteColorMapping {
    pub color: String,
    pub levels: Vec<PatchGuiIndexedEntry>,
}
