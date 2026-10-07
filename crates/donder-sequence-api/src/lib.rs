#![deny(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unwrap_used
    )
)]

use serde::{Deserialize, Serialize};
use specta::Type;

mod dto;
pub use dto::*;

// Shared serialized edit contract for desktop and browser clients.
// Tauri-Specta exports the TypeScript representation in bindings.ts.

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceAutomationTarget {
    EffectParam { effect_id: u32, param: String },
    CompositionNodeParam { node_id: String, param: String },
}

/// A sequence item that carries a description.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceDescribedItem {
    Layer { id: u32 },
    MarkCollection { key: String },
    Clip { id: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceGuiEdit {
    SetDuration {
        duration_seconds: f32,
    },
    SetAudio {
        #[serde(rename = "import")]
        import_path: Option<String>,
    },
    AddEffect {
        initial_color: String,
        effect: SequenceEffectReference,
        target: FixtureTarget,
        scope: SequenceEffectScope,
        start_seconds: f32,
        mark_collection_key: Option<String>,
    },
    CreateLayer {
        name: String,
        color: String,
    },
    CreateLayerAt {
        name: String,
        color: String,
        x: f32,
        y: f32,
    },
    RenameLayer {
        id: u32,
        name: String,
    },
    RenameClip {
        id: u32,
        name: String,
    },
    RenameGraphNode {
        node_id: String,
        name: String,
    },
    /// Set an item's description; empty text removes it.
    SetItemDescription {
        item: SequenceDescribedItem,
        description: Option<String>,
    },
    SetLayerColor {
        id: u32,
        color: String,
    },
    SetLayerEnabled {
        id: u32,
        enabled: bool,
    },
    SetEffectLayer {
        id: u32,
        layer_id: u32,
    },
    MoveEffect {
        id: u32,
        start_seconds: f32,
        target: Option<FixtureTarget>,
    },
    ResizeEffect {
        id: u32,
        start_seconds: f32,
        duration_seconds: f32,
    },
    ChangeEffectDefinition {
        initial_color: String,
        id: u32,
        effect: SequenceEffectReference,
    },
    DeleteEffect {
        id: u32,
    },
    RetargetEffect {
        id: u32,
        target: FixtureTarget,
    },
    SetEffectScope {
        id: u32,
        scope: SequenceEffectScope,
    },
    UpdateEffectParam {
        id: u32,
        name: String,
        value: SequenceEffectParamValue,
    },
    AddGraphOperatorNode {
        initial_color: String,
        operator: SequenceGraphOperator,
        x: f32,
        y: f32,
    },
    MoveGraphNodes {
        positions: Vec<SequenceGraphNodePosition>,
    },
    DeleteGraphItems {
        node_ids: Vec<String>,
        layer_ids: Vec<u32>,
        edges: Vec<SequenceGraphEdge>,
        migrate_to_layer_id: Option<u32>,
    },
    ConnectGraphNodes {
        from_node: String,
        from_port: String,
        to_node: String,
        to_port: String,
    },
    ReconnectGraphEdge {
        previous: SequenceGraphEdge,
        connection: SequenceGraphEdge,
    },
    UpdateGraphOperatorParam {
        node_id: String,
        name: String,
        value: SequenceEffectParamValue,
    },
    AddAutomationClip {
        start_seconds: f32,
        duration_seconds: f32,
        row_target: FixtureTarget,
    },
    CreateAndBindAutomationClip {
        target: SequenceAutomationTarget,
    },
    MoveAutomationClip {
        id: u32,
        start_seconds: f32,
        row_target: FixtureTarget,
    },
    SplitAutomationClip {
        id: u32,
        time_seconds: f32,
    },
    UpdateAutomationCurve {
        id: u32,
        curve: Vec<SequenceCurvePoint>,
    },
    DeleteAutomationClip {
        id: u32,
    },
    BindAutomationParam {
        clip_id: u32,
        target: SequenceAutomationTarget,
    },
    UnbindAutomationParam {
        clip_id: u32,
        target: SequenceAutomationTarget,
    },
    RebindDetachedAutomation {
        clip_id: u32,
        detached_index: u32,
        target: SequenceAutomationTarget,
    },
    DiscardDetachedAutomation {
        clip_id: u32,
        detached_index: u32,
    },
    /// The name becomes a unique `snake_case` name.
    CreateMarkCollection {
        name: String,
        color: String,
    },
    RenameMarkCollection {
        key: String,
        name: String,
    },
    DeleteMarkCollection {
        key: String,
    },
    SetMarkCollectionColor {
        key: String,
        color: String,
    },
    AddMark {
        collection_key: String,
        time_seconds: f32,
    },
    MoveMark {
        collection_key: String,
        index: u32,
        time_seconds: f32,
    },
    ReassignMarkCollection {
        collection_key: String,
        index: u32,
        target_collection_key: String,
    },
    DeleteMark {
        collection_key: String,
        index: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceMarkRef {
    pub collection_key: String,
    pub index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequencePasteAnchor {
    pub target: Option<FixtureTarget>,
    pub time_seconds: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceSelection {
    Clips {
        effect_ids: Vec<u32>,
        automation_ids: Vec<u32>,
    },
    Marks {
        marks: Vec<SequenceMarkRef>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceEffectCommonEdit {
    Layer {
        layer_id: u32,
    },
    Scope {
        scope: SequenceEffectScope,
    },
    Start {
        start_seconds: f32,
    },
    Duration {
        duration_seconds: f32,
    },
    Param {
        name: String,
        value: SequenceEffectParamValue,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceSelectionEdit {
    Copy {
        selection: SequenceSelection,
    },
    Cut {
        selection: SequenceSelection,
    },
    Delete {
        selection: SequenceSelection,
    },
    Paste {
        anchor: SequencePasteAnchor,
    },
    MoveClips {
        effect_ids: Vec<u32>,
        automation_ids: Vec<u32>,
        time_delta_seconds: f32,
        lane_delta: i32,
    },
    ResizeClips {
        effect_ids: Vec<u32>,
        automation_ids: Vec<u32>,
        edge: SequenceResizeEdge,
        automation: SequenceAutomationResize,
        time_delta_seconds: f32,
    },
    EditEffects {
        effect_ids: Vec<u32>,
        edit: SequenceEffectCommonEdit,
    },
    MoveMarks {
        marks: Vec<SequenceMarkRef>,
        time_delta_seconds: f32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceEffectParamValue {
    Int { value: f32 },
    Float { value: f32 },
    Bool { value: bool },
    Color { value: String },
    Enum { value: String },
    Curve { value: SequenceCurveValue },
    Gradient { value: SequenceGradientValue },
    IntArray { values: Vec<f32> },
    FloatArray { values: Vec<f32> },
    BoolArray { values: Vec<bool> },
    ColorArray { values: Vec<String> },
    CurveArray { values: Vec<SequenceCurveValue> },
    GradientArray { values: Vec<SequenceGradientValue> },
    Marks { key: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceCurveValue {
    pub points: Vec<SequenceCurvePoint>,
    pub source: SequenceLibrarySource,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGradientValue {
    pub stops: Vec<SequenceGradientStop>,
    pub source: SequenceLibrarySource,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceLibrarySource {
    Inline,
    Library {
        module_id: String,
        path: String,
        object_key: String,
        display_name: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceGraphOperator {
    Custom {
        module_id: String,
        path: String,
        object_key: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGraphNodePosition {
    pub node_id: String,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGraphEdge {
    pub from_node: String,
    pub from_port: String,
    pub to_node: String,
    pub to_port: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceEffectParamKind {
    Int,
    Float,
    Bool,
    Color,
    Enum,
    Curve,
    Gradient,
    IntArray,
    FloatArray,
    BoolArray,
    ColorArray,
    CurveArray,
    GradientArray,
    Marks,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceEffectScope {
    PerFixture,
    WholeTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceResizeEdge {
    Left,
    Right,
}

/// How a resize treats an automation clip's curve.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceAutomationResize {
    /// The edge moves over fixed content.
    Crop,
    /// The content scales with the clip.
    Stretch,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceEffectReference {
    Custom {
        module_id: String,
        path: String,
        effect_name: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceCurvePoint {
    pub time: f32,
    pub value: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FixtureTarget {
    pub fixture: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGradientStop {
    pub time: f32,
    pub value: String,
}
