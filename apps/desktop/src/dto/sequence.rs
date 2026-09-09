use super::*;

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
    pub control_clips: Vec<SequenceControlClip>,
    pub control_channels: Vec<SequenceControlChannel>,
    pub composition_graph: SequenceCompositionGraph,
    pub automation_clips: Vec<SequenceAutomationClip>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceLayer {
    pub id: u32,
    pub name: String,
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
    pub anchor_lane_index: u32,
    pub lane_index: u32,
    pub curve: Vec<SequenceCurvePoint>,
    pub bindings: Vec<SequenceAutomationBinding>,
    pub detached_bindings: Vec<SequenceDetachedAutomationBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceAutomationBinding {
    pub target: SequenceAutomationTarget,
    pub mapping: SequenceAutomationMapping,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceDetachedAutomationBinding {
    pub target: SequenceAutomationTarget,
    pub mapping: SequenceAutomationMapping,
    pub reason: SequenceAutomationDetachmentReason,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceAutomationDetachmentReason {
    TargetDeleted,
    DefinitionChanged,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceAutomationMapping {
    Float { min: f32, max: f32 },
    Int { min: f32, max: f32 },
    Bool,
    Enum { values: Vec<String> },
    Curve { min: f32, max: f32 },
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
    pub layer_id: u32,
    pub start_seconds: f32,
    pub duration_seconds: f32,
    pub target: ElementTarget,
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
pub struct SequenceControlClip {
    pub id: u32,
    pub start_seconds: f32,
    pub duration_seconds: f32,
    pub target: SequenceControlTarget,
    pub target_label: String,
    pub value: SequenceControlValue,
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
        operator: SequenceGraphOperator,
        params: Vec<SequenceEffectParam>,
    },
    Output,
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
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceGraphOperator {
    Builtin {
        operator: SequenceBuiltinOperator,
    },
    Custom {
        module_id: String,
        path: String,
        object_key: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceBuiltinOperator {
    Max,
    Add,
    Multiply,
    IntensityModulate,
    Dim,
    Invert,
    Colorize,
    Delay,
    Echo,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGraphOperatorDefinition {
    pub operator: SequenceGraphOperator,
    pub source_name: String,
    pub display_name: String,
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
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectParam {
    pub fixed: bool,
    pub supports_automation: bool,
    pub name: String,
    pub kind: SequenceEffectParamKind,
    pub options: Vec<String>,
    pub editable: bool,
    pub value: SequenceEffectParamValue,
    pub automation: Option<SequenceParamAutomation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceParamAutomation {
    pub clip_id: u32,
    pub mapping: SequenceAutomationMapping,
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
pub struct SequenceEffectDefinition {
    pub name: String,
    pub kind: SequenceEffectDefinitionKind,
    pub effect: SequenceEffectReference,
    #[serde(rename = "import")]
    pub import_path: Option<String>,
    pub params: Vec<SequenceEffectDefinitionParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectDefinitionParam {
    pub fixed: bool,
    pub supports_automation: bool,
    pub name: String,
    pub kind: SequenceEffectParamKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceGuiEdit {
    UpsertControlClip {
        id: Option<u32>,
        start_seconds: f32,
        duration_seconds: f32,
        target: SequenceControlTarget,
        value: SequenceControlValue,
    },
    SetDuration {
        duration_seconds: f32,
    },
    SetAudio {
        #[serde(rename = "import")]
        import_path: Option<String>,
    },
    DeleteControlClip {
        id: u32,
    },
    AddEffect {
        initial_color: String,
        effect: SequenceEffectReference,
        target: ElementTarget,
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
    SetLayerColor {
        id: u32,
        color: String,
    },
    SetLayerEnabled {
        id: u32,
        enabled: bool,
    },
    DeleteLayer {
        id: u32,
        migrate_to_layer_id: u32,
    },
    SetEffectLayer {
        id: u32,
        layer_id: u32,
    },
    MoveEffect {
        id: u32,
        start_seconds: f32,
        target: Option<ElementTarget>,
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
        target: ElementTarget,
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
    MoveGraphNode {
        node_id: String,
        x: f32,
        y: f32,
    },
    DeleteGraphNode {
        node_id: String,
    },
    ConnectGraphNodes {
        from_node: String,
        from_port: String,
        to_node: String,
        to_port: String,
    },
    DisconnectGraphNodes {
        from_node: String,
        from_port: String,
        to_node: String,
        to_port: String,
    },
    UpdateGraphOperatorParam {
        node_id: String,
        name: String,
        value: SequenceEffectParamValue,
    },
    AddAutomationClip {
        start_seconds: f32,
        duration_seconds: f32,
        anchor_lane_index: u32,
        lane_index: u32,
    },
    CreateAndBindAutomationClip {
        target: SequenceAutomationTarget,
        mapping: SequenceAutomationMapping,
    },
    MoveAutomationClip {
        id: u32,
        start_seconds: f32,
        anchor_lane_index: u32,
        lane_index: u32,
    },
    ResizeAutomationClip {
        id: u32,
        start_seconds: f32,
        duration_seconds: f32,
    },
    UpdateAutomationCurve {
        id: u32,
        curve: Vec<SequenceCurvePoint>,
    },
    UpdateAutomationParamMapping {
        clip_id: u32,
        target: SequenceAutomationTarget,
        mapping: SequenceAutomationMapping,
    },
    DeleteAutomationClip {
        id: u32,
    },
    BindAutomationParam {
        clip_id: u32,
        target: SequenceAutomationTarget,
        mapping: SequenceAutomationMapping,
    },
    UnbindAutomationParam {
        clip_id: u32,
        target: SequenceAutomationTarget,
    },
    RebindDetachedAutomation {
        clip_id: u32,
        detached_index: u32,
        target: SequenceAutomationTarget,
        mapping: SequenceAutomationMapping,
    },
    DiscardDetachedAutomation {
        clip_id: u32,
        detached_index: u32,
    },
    CreateMarkCollection {
        key: String,
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
pub struct SequenceLane {
    pub target: ElementTarget,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceMarkCollection {
    pub key: String,
    pub name: String,
    pub color: String,
    pub marks_seconds: Vec<f32>,
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
    pub lane_index: u32,
    pub time_seconds: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceSelection {
    Effects { ids: Vec<u32> },
    Marks { marks: Vec<SequenceMarkRef> },
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
    MoveEffects {
        ids: Vec<u32>,
        time_delta_seconds: f32,
        lane_delta: i32,
    },
    ResizeEffects {
        ids: Vec<u32>,
        edge: SequenceResizeEdge,
        time_delta_seconds: f32,
    },
    MoveMarks {
        marks: Vec<SequenceMarkRef>,
        time_delta_seconds: f32,
    },
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
