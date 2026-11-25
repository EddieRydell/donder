use crate::dto::*;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceAudio {
    #[serde(rename = "import")]
    pub import_path: String,
    pub resolved_path: String,
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
    pub gradient: SequenceGradient,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGuiDocument {
    /// The open object's description, edited with a description `GuiEdit::Put`.
    pub(crate) description: Option<String>,
    pub(crate) source_ref: GuiObjectRef,
    pub duration_seconds: f64,
    pub frame_rate: f32,
    pub audio: Option<SequenceAudio>,
    pub(crate) mark_collections: Vec<SequenceMarkCollection>,
    /// Whether the audio its effects read is analyzed.
    pub(crate) derived_data: DerivedDataStatus,
    pub targets: Vec<SequenceLane>,
    pub effect_definitions: Vec<SequenceEffectDefinition>,
    pub curve_library: Vec<SequenceCurveLibraryItem>,
    pub gradient_library: Vec<SequenceGradientLibraryItem>,
    pub(crate) layers: Vec<SequenceLayer>,
    pub clips: Vec<GuiClip>,
    pub composition_graph: SequenceCompositionGraph,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceLayer {
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) color: String,
    pub(crate) enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceAutomationBinding {
    pub(crate) target: SequenceAutomationTarget,
    pub(crate) window: GuiWindow,
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
pub struct GuiClip {
    pub id: u32,
    /// The clip's name, which envelopes and actions use.
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    /// What it plays over; an envelope's row.
    pub(crate) target: FixtureTarget,
    pub(crate) target_label: String,
    pub(crate) placement: GuiPlacement,
    pub content: GuiClipContent,
    /// The length a console clip repeats every when it plays from the
    /// session's start.
    pub(crate) always_length_seconds: Option<f64>,
}

/// What a clip plays: an effect on a layer, or an envelope.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiClipContent {
    Effect {
        layer_id: u32,
        scope: SequenceClipScope,
        effect: SequenceEffectReference,
        effect_name: String,
    },
    Envelope {
        curve: Vec<SequenceCurvePoint>,
        bindings: Vec<SequenceAutomationBinding>,
    },
}

/// When a clip plays: at a time, or when a console's actions play it, each
/// copy taking `values` from the press that started it.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiPlacement {
    Timed {
        start_seconds: f64,
        duration_seconds: f64,
    },
    Played {
        values: Vec<ConsolePlayedValue>,
    },
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
    pub(crate) ids: Vec<u32>,
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
    pub operator_catalog: Vec<SequenceGraphOperatorDefinition>,
    pub nodes: Vec<SequenceGraphNode>,
    pub edges: Vec<SequenceGraphEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGraphNode {
    pub id: u32,
    pub(crate) x: f32,
    pub(crate) y: f32,
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
    pub(crate) display_name: String,
    /// The script's description of the operator.
    pub(crate) description: Option<String>,
    /// The operator's inputs, in declaration order.
    pub(crate) inputs: Vec<String>,
    pub(crate) params: Vec<SequenceEffectDefinitionParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipParam {
    pub supports_automation: bool,
    pub name: String,
    /// The script's description of the parameter.
    pub(crate) description: Option<String>,
    pub(crate) kind: SequenceParamKind,
    /// An enum parameter's options, in order.
    pub(crate) options: Vec<SequenceParamOption>,
    pub(crate) range: Option<SequenceParamRange>,
    pub editable: bool,
    pub value: SequenceParamValue,
    pub(crate) automation: Option<SequenceParamAutomation>,
    /// The value a new item of an array parameter starts with.
    pub(crate) new_item: Option<SequenceParamValue>,
}

/// An enum option and the fields it carries, each valued at its default.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceParamOption {
    pub(crate) name: String,
    pub(crate) fields: Vec<SequenceClipParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceParamAutomation {
    /// The envelope that drives it.
    pub(crate) clip_id: u32,
    /// The part of the clip's `0..1` the binding uses.
    pub(crate) window: GuiWindow,
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
    /// The collection's id, which clip parameters and marks refer to.
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) color: String,
    pub(crate) marks_seconds: Vec<f64>,
    /// Each mark's duration, in the same order as `marks_seconds`; zero for an
    /// instantaneous mark.
    pub(crate) mark_durations_seconds: Vec<f64>,
    /// Each mark's label, in the same order as `marks_seconds`.
    pub(crate) mark_labels: Vec<Option<String>>,
}

/// Where the analysis of a sequence's audio its effects read stands.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DerivedDataStatus {
    /// No effect of the sequence reads its audio, a font or an image.
    Unused,
    /// The audio is being analyzed, fonts rasterized or images decoded;
    /// effects read silence, blank text and black images until then.
    Reading,
    Ready,
    Failed {
        /// The asset's project-relative path.
        file: String,
        reason: String,
    },
}
