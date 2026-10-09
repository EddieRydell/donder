use crate::*;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceAudio {
    #[serde(rename = "import")]
    pub import_path: String,
    pub resolved_path: String,
    pub file_name: String,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceCurveLibraryItem {
    pub module_id: String,
    pub path: String,
    pub object_key: String,
    pub display_name: String,
    pub points: Vec<SequenceCurvePoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGradientLibraryItem {
    pub module_id: String,
    pub path: String,
    pub object_key: String,
    pub display_name: String,
    pub stops: Vec<SequenceGradientStop>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGuiDocument {
    /// The open object's description, edited with `GuiEditCommand::Description`.
    pub description: Option<String>,
    pub path: String,
    pub source_ref: GuiObjectRef,
    pub object_key: String,
    pub duration_seconds: f32,
    pub frame_rate: f32,
    pub audio: Option<SequenceAudio>,
    pub mark_collections: Vec<SequenceMarkCollection>,
    pub lanes: Vec<SequenceLane>,
    pub effect_definitions: Vec<SequenceEffectDefinition>,
    pub curve_library: Vec<SequenceCurveLibraryItem>,
    pub gradient_library: Vec<SequenceGradientLibraryItem>,
    pub layers: Vec<SequenceLayer>,
    pub effects: Vec<SequenceEffect>,
    pub composition_graph: SequenceCompositionGraph,
    pub automation_clips: Vec<SequenceAutomationClip>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceLayer {
    pub id: u32,
    pub name: String,
    pub description: Option<String>,
    pub color: String,
    pub enabled: bool,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceAutomationClip {
    pub id: u32,
    pub start_seconds: f32,
    pub duration_seconds: f32,
    pub row_target: FixtureTarget,
    pub curve: Vec<SequenceCurvePoint>,
    pub bindings: Vec<SequenceAutomationBinding>,
    pub detached_bindings: Vec<SequenceDetachedAutomationBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceAutomationBinding {
    pub target: SequenceAutomationTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceDetachedAutomationBinding {
    pub target: SequenceAutomationTarget,
    pub reason: SequenceAutomationDetachmentReason,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceAutomationDetachmentReason {
    DefinitionChanged,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipRasterRequest {
    #[serde(flatten)]
    pub document: GuiDocumentRequest,
    pub items: Vec<SequenceClipRasterRequestItem>,
    pub display_row_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipRasterRequestItem {
    pub effect_id: u32,
    pub signature: Option<String>,
    pub display_column_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipRasterResponse {
    pub project_revision: u32,
    pub request_id: u32,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipRasterResultBatch {
    pub project_revision: u32,
    pub request_id: u32,
    pub ready: Vec<SequenceClipRaster>,
    pub unavailable: Vec<SequenceClipRasterUnavailable>,
    pub errors: Vec<SequenceClipRasterError>,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipRaster {
    pub request_id: u32,
    pub effect_id: u32,
    pub signature: String,
    pub columns: u32,
    pub rows: u32,
    pub start_seconds: f32,
    pub duration_seconds: f32,
    pub pixels_rgba_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipRasterError {
    pub request_id: u32,
    pub effect_id: u32,
    pub signature: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipRasterUnavailable {
    pub request_id: u32,
    pub effect_id: u32,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffect {
    pub index: u32,
    pub id: u32,
    /// The clip's name, which automation bindings use.
    pub name: String,
    pub description: Option<String>,
    pub layer_id: u32,
    pub start_seconds: f32,
    pub duration_seconds: f32,
    pub target: FixtureTarget,
    pub target_label: String,
    pub scope: SequenceEffectScope,
    pub effect: String,
    pub effect_reference: SequenceEffectReference,
    pub params: Vec<SequenceEffectParam>,
    pub kind: SequenceTimelineClipKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceTimelineClipKind {
    Effect,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceCompositionGraph {
    pub id: u32,
    pub operator_catalog: Vec<SequenceGraphOperatorDefinition>,
    pub nodes: Vec<SequenceGraphNode>,
    pub edges: Vec<SequenceGraphEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGraphNode {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub inputs: Vec<SequenceGraphPortDefinition>,
    pub outputs: Vec<SequenceGraphPortDefinition>,
    pub kind: SequenceGraphNodeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceGraphNodeKind {
    Layer {
        layer_id: u32,
        layer_name: String,
        layer_color: String,
        enabled: bool,
    },
    Operator {
        /// The node's name, which edges and automation bindings use.
        name: String,
        operator: SequenceGraphOperator,
        params: Vec<SequenceEffectParam>,
    },
    Output,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGraphOperatorDefinition {
    pub operator: SequenceGraphOperator,
    pub source_name: String,
    pub display_name: String,
    /// The script's description of the operator.
    pub description: Option<String>,
    pub inputs: Vec<SequenceGraphPortDefinition>,
    pub outputs: Vec<SequenceGraphPortDefinition>,
    pub params: Vec<SequenceEffectDefinitionParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGraphPortDefinition {
    pub source_name: String,
    pub display_name: String,
    pub cardinality: SequenceGraphPortCardinality,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceGraphPortCardinality {
    One,
    Many,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectParam {
    pub supports_automation: bool,
    pub name: String,
    /// The script's description of the parameter.
    pub description: Option<String>,
    pub kind: SequenceEffectParamKind,
    pub options: Vec<String>,
    pub range: Option<SequenceParamRange>,
    pub editable: bool,
    pub value: SequenceEffectParamValue,
    pub automation: Option<SequenceParamAutomation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceParamAutomation {
    pub clip_id: u32,
}

/// Inclusive declared range of an `int`, `float`, or `curve` param.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceParamRange {
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectDefinition {
    pub name: String,
    /// The script's description of the effect.
    pub description: Option<String>,
    pub effect: SequenceEffectReference,
    #[serde(rename = "import")]
    pub import_path: Option<String>,
    pub params: Vec<SequenceEffectDefinitionParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectDefinitionParam {
    pub supports_automation: bool,
    pub name: String,
    /// The script's description of the parameter.
    pub description: Option<String>,
    pub kind: SequenceEffectParamKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceLane {
    pub target: FixtureTarget,
    pub label: String,
    pub kind: SequenceLaneKind,
    /** Nesting depth below the layout root; top-level fixtures and groups are 0. */
    pub depth: u32,
    /** How many lanes show this target. Every copy shows the same clips. */
    pub occurrences: u32,
}

/** Lanes walk the layout root depth-first; a group lane precedes its members, and
 * a member of several groups has a lane under each. */
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceLaneKind {
    Fixture,
    Group,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceMarkCollection {
    /// The collection's name, which effect parameters use.
    pub key: String,
    pub description: Option<String>,
    pub color: String,
    pub marks_seconds: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceSelectionEditResult {
    pub snapshot: AppSnapshot,
    pub document: GuiDocument,
    pub selection: Option<SequenceSelection>,
    pub copied_count: u32,
    pub skipped_count: u32,
}
