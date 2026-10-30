use crate::dto::*;
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
    pub path: String,
    pub object_key: String,
    pub display_name: String,
    pub points: Vec<SequenceCurvePoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGradientLibraryItem {
    pub path: String,
    pub object_key: String,
    pub display_name: String,
    pub stops: Vec<SequenceGradientStop>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGuiDocument {
    /// The open object's description, edited with `GuiEditCommand::Description`.
    pub(crate) description: Option<String>,
    pub(crate) path: String,
    pub(crate) source_ref: GuiObjectRef,
    pub(crate) object_key: String,
    pub duration_seconds: f64,
    pub frame_rate: f32,
    pub audio: Option<SequenceAudio>,
    pub(crate) mark_collections: Vec<SequenceMarkCollection>,
    pub lanes: Vec<SequenceLane>,
    pub effect_definitions: Vec<SequenceEffectDefinition>,
    pub curve_library: Vec<SequenceCurveLibraryItem>,
    pub gradient_library: Vec<SequenceGradientLibraryItem>,
    pub(crate) layers: Vec<SequenceLayer>,
    pub clips: Vec<SequenceClip>,
    pub composition_graph: SequenceCompositionGraph,
    pub(crate) automation_clips: Vec<SequenceAutomationClip>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceLayer {
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) color: String,
    pub(crate) enabled: bool,
    pub(crate) is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceAutomationClip {
    pub(crate) id: u32,
    pub(crate) start_seconds: f64,
    pub(crate) duration_seconds: f64,
    pub(crate) row_target: FixtureTarget,
    pub(crate) curve: Vec<SequenceCurvePoint>,
    pub(crate) bindings: Vec<SequenceAutomationBinding>,
    pub(crate) detached_bindings: Vec<SequenceDetachedAutomationBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceAutomationBinding {
    pub(crate) target: SequenceAutomationTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceDetachedAutomationBinding {
    pub(crate) target: SequenceAutomationTarget,
}

/// The clips the editor shows, in the order their rasters should render.
/// Every clip of the sequence gets a raster; visible clips render first.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipRasterRequest {
    #[serde(flatten)]
    pub document: GuiDocumentRequest,
    pub visible_clip_ids: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipRasterResponse {
    /// Clips whose rasters are not rendered yet.
    pub pending: u32,
}

/// The rasters that changed after `since` in a results request.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipRasterResultBatch {
    /// The newest raster revision; the next request asks for later ones.
    pub revision: u32,
    pub rasters: Vec<SequenceClipRaster>,
    pub errors: Vec<SequenceClipRasterError>,
    pub pending: u32,
}

/// A clip's raster: one column per sampled time across the clip, one row per
/// sampled pixel. A raster is rendered once at this resolution and scaled to
/// the clip's on-screen size.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipRaster {
    pub clip_id: u32,
    /// Changes whenever the clip's raster is rendered again.
    pub revision: u32,
    pub columns: u32,
    pub rows: u32,
    pub pixels_rgba_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipRasterError {
    pub clip_id: u32,
    pub revision: u32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClip {
    pub(crate) index: u32,
    pub id: u32,
    /// The clip's name, which automation bindings use.
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) layer_id: u32,
    pub(crate) start_seconds: f64,
    pub(crate) duration_seconds: f64,
    pub(crate) target: FixtureTarget,
    pub(crate) target_label: String,
    pub(crate) scope: SequenceClipScope,
    pub(crate) effect: String,
}

/// What the inspector edits on one clip, fetched for the selected clips only:
/// a sequence document carries every clip's summary but no parameters.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipDetails {
    pub(crate) id: u32,
    pub(crate) effect_reference: SequenceEffectReference,
    pub params: Vec<SequenceClipParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipDetailsResult {
    pub project_revision: u32,
    pub details: Vec<SequenceClipDetails>,
}

/// "Select Matching": the clips matching any seed clip under `criteria` and
/// every `params` entry the seed clip has.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipMatchRequest {
    /// The seed clips.
    pub(crate) clips: Vec<SequenceClipRef>,
    pub(crate) criteria: SequenceClipMatch,
    pub(crate) params: Vec<SequenceClipParamMatch>,
}

/// A parameter matches when its value equals the seed's, with numbers within
/// `tolerance` of the seed's.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipParamMatch {
    pub(crate) name: String,
    pub(crate) tolerance: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceCompositionGraph {
    pub(crate) id: u32,
    pub operator_catalog: Vec<SequenceGraphOperatorDefinition>,
    pub nodes: Vec<SequenceGraphNode>,
    pub edges: Vec<SequenceGraphEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGraphNode {
    pub id: String,
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) inputs: Vec<SequenceGraphPortDefinition>,
    pub(crate) outputs: Vec<SequenceGraphPortDefinition>,
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
        params: Vec<SequenceClipParam>,
    },
    Root,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGraphOperatorDefinition {
    pub operator: SequenceGraphOperator,
    pub(crate) source_name: String,
    pub(crate) display_name: String,
    /// The script's description of the operator.
    pub(crate) description: Option<String>,
    pub(crate) inputs: Vec<SequenceGraphPortDefinition>,
    pub(crate) outputs: Vec<SequenceGraphPortDefinition>,
    pub(crate) params: Vec<SequenceEffectDefinitionParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGraphPortDefinition {
    pub(crate) source_name: String,
    pub(crate) display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipParam {
    pub supports_automation: bool,
    pub name: String,
    /// The script's description of the parameter.
    pub(crate) description: Option<String>,
    pub(crate) kind: SequenceParamKind,
    pub(crate) options: Vec<String>,
    pub(crate) range: Option<SequenceParamRange>,
    pub editable: bool,
    pub value: SequenceParamValue,
    pub(crate) automation: Option<SequenceParamAutomation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceParamAutomation {
    pub(crate) automation_clip_id: u32,
}

/// Inclusive declared range of an `int`, `float`, or `curve` param.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceParamRange {
    pub(crate) min: f64,
    pub(crate) max: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectDefinition {
    pub(crate) name: String,
    /// The script's description of the effect.
    pub(crate) description: Option<String>,
    pub effect: SequenceEffectReference,
    #[serde(rename = "import")]
    pub(crate) import_path: Option<String>,
    pub(crate) params: Vec<SequenceEffectDefinitionParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectDefinitionParam {
    pub(crate) supports_automation: bool,
    pub(crate) name: String,
    /// The script's description of the parameter.
    pub(crate) description: Option<String>,
    pub(crate) kind: SequenceParamKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceLane {
    pub target: FixtureTarget,
    pub(crate) label: String,
    pub(crate) kind: SequenceLaneKind,
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
    /// The collection's name, which clip parameters use.
    pub(crate) key: String,
    pub(crate) description: Option<String>,
    pub(crate) color: String,
    pub(crate) marks_seconds: Vec<f64>,
    /// Each mark's label, in the same order as `marks_seconds`.
    pub(crate) mark_labels: Vec<Option<String>>,
}
