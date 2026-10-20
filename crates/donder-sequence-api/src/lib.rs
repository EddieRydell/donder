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
pub use dto::app::{AppSnapshot, ProjectHealth};
pub use dto::audio::{
    AudioTransportSnapshot, AudioTransportState, PlaybackFrameTiming, PlaybackRange, PlaybackSpeed,
};
pub use dto::diagnostics::{
    DocumentInclusion, ProjectDiagnostic, RelatedDiagnosticLocation, Rotation3Degrees, Scale3,
};
pub use dto::live::{
    GuiButtonInput, GuiInputControl, GuiInputDriver, InputDeviceGuiDocument, ProjectGuiEdit,
    RigActionChoice, RigButtonRef, RigControlTarget, RigEffect, RigEffectEdit, RigEffectOwner,
    RigGuiDocument, RigGuiEdit, RigInput, RigInputButton, RigInputDevice, RigItem, RigLiveMarks,
    RigMapping, RigMappingAction, RigSwitch, RigTriggerLength, RigVoice, RigVoicePolicy, RigWhen,
    SetupInputDevice,
};
pub use dto::output::{
    ControllerOutputTest, DeviceCapabilities, DeviceFirmwareInfo, DeviceInstallProgress,
    DeviceOutputCapabilities, DevicePlaybackMode, DevicePlaybackStatus, DeviceSequenceStorage,
    DeviceSerialPort, DeviceTransportStatus, DonderDeviceClaim, DonderDeviceConnection,
    DonderDeviceNetwork, DonderDeviceNetworkRequest, DonderDeviceStatus,
    LiveOutputControllerSnapshot, LiveOutputControllerState, LiveOutputSnapshot, LiveOutputState,
    Point3Meters, SequenceExportOptions, SequenceExportPort, VideoExportProgress,
};
pub use dto::patch::{
    GuiPixelEncoding, GuiPixelRoute, GuiPixelSpan, PatchFixtureTarget, PatchGuiDocument,
    PatchLayout,
};
pub use dto::perform::{PerformButton, PerformRig, PerformSession, PerformSnapshot, PerformSwitch};
pub use dto::preview::{
    FixtureGuiDocument, FixtureGuiEdit, FixtureStorage, GeometryRenderBounds, GuiFixtureElement,
    GuiFixtureHandle, GuiFixtureShape, GuiFixtureSource, GuiGridAxis, GuiGridCorner,
    GuiLayoutFixture, GuiLayoutFixtureKind, LayoutGuiDocument, LayoutGuiEdit, PreviewAppearance,
    SpatialRenderPixel, SpatialRenderPlan,
};
pub use dto::sequence::{
    SequenceAudio, SequenceAutomationBinding, SequenceAutomationClip,
    SequenceAutomationDetachmentReason, SequenceClipMatchRequest, SequenceClipParamMatch,
    SequenceClipRaster, SequenceClipRasterError, SequenceClipRasterRequest,
    SequenceClipRasterResponse, SequenceClipRasterResultBatch, SequenceCompositionGraph,
    SequenceCurveLibraryItem, SequenceDetachedAutomationBinding, SequenceEffect,
    SequenceEffectDefinition, SequenceEffectDefinitionParam, SequenceEffectDetails,
    SequenceEffectDetailsResult, SequenceEffectParam, SequenceGradientLibraryItem,
    SequenceGraphNode, SequenceGraphNodeKind, SequenceGraphOperatorDefinition,
    SequenceGraphPortDefinition, SequenceGuiDocument, SequenceLane, SequenceLaneKind,
    SequenceLayer, SequenceMarkCollection, SequenceParamAutomation, SequenceParamRange,
    SequenceSelectionEditResult, SequenceTimelineClipKind,
};
pub use dto::setup::{
    BufferExternalState, ControllerGuiDocument, CurveGuiDocument, DiagnosticSeverity,
    DocumentViewId, GradientGuiDocument, GuiDocument, GuiDocumentChange, GuiDocumentRequest,
    GuiEditCommand, GuiEditResult, GuiEditUpdate, GuiObjectRef, GuiOwnedStep, GuiOwnershipEdit,
    GuiOwnershipSlot, ProjectGuiDocument, ReusableStorage, SetupController, SetupControllerConfig,
    SetupControllerPort, SetupGuiDocument, SetupGuiEdit,
};
pub use dto::synchronization::{
    DocumentSaveState, DocumentSaveStatus, DocumentText, DocumentTextEdit, DocumentTextEdits,
    DocumentUpdate, ExternalConflictDecision, GuiDocumentResult, TransitionDecision,
    TransitionRequest, TransitionResult, WorkspaceTransition,
};
pub use dto::view_state::{
    PersistedEditorViewState, PersistedEditorViewStateUpdate, PersistedGraphNodeSize,
    PersistedGraphViewState, PersistedGraphViewStateUpdate, PersistedGraphViewport,
    PersistedPreviewWindowState, PersistedSequenceViewportState,
    PersistedSequenceViewportStateUpdate, PersistedSpatialViewState,
    PersistedSpatialViewStateUpdate, PersistedWindowState, ProjectRestoreState, SpatialGuide,
    SpatialGuideAxis,
};
pub use dto::workspace::{
    AppCommandId, AppSettings, CommandId, DocumentDefaultObjectKey, DocumentDescriptor,
    DocumentObjectDescriptor, EditorBuffer, EditorCommandId, EditorTab, EditorViewMode,
    EffectRasterSettings, KeyBinding, NewSequenceRequest, NewSequenceResult, NewSequenceStorage,
    ObjectKind, ProjectSearchMatch, ProjectSearchMatchKind, ProjectSearchRequest,
    ProjectSearchResponse, SequenceClipMatch, SequenceClipTimeMatch, SequenceFollowMode,
    SequenceInitialZoomMode, Shortcut, SidebarView, SpatialSnapSettings, SpatialUnit,
    TextDocumentSyntax, TextPosition, TextRange, Transform, WorkspaceEntry, WorkspaceEntryKind,
    WorkspaceEntryRole, WorkspaceExplorerState, WorkspaceLayoutState, WorkspaceOperation,
    WorkspacePathChangeImpact, WorkspacePathChangePlan, WorkspacePathChangeRequest,
    workspace_role_for_source_object,
};

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
    Composition {
        edit: CompositionGuiEdit,
    },
    SetDuration {
        duration_seconds: f64,
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
        start_seconds: f64,
    },
    RenameClip {
        id: u32,
        name: String,
    },
    /// Set an item's description; empty text removes it.
    SetItemDescription {
        item: SequenceDescribedItem,
        description: Option<String>,
    },
    SetEffectLayer {
        id: u32,
        layer_id: u32,
    },
    MoveEffect {
        id: u32,
        start_seconds: f64,
        target: Option<FixtureTarget>,
    },
    ResizeEffect {
        id: u32,
        start_seconds: f64,
        duration_seconds: f64,
    },
    ChangeEffectDefinition {
        initial_color: String,
        id: u32,
        effect: SequenceEffectReference,
    },
    DeleteEffect {
        id: u32,
    },
    /// Cut each clip at `time_seconds` into two back-to-back clips. The right
    /// half shares the left half's automation, so automation continues across
    /// the cut.
    SplitEffects {
        ids: Vec<u32>,
        time_seconds: f64,
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
    AddAutomationClip {
        start_seconds: f64,
        duration_seconds: f64,
        row_target: FixtureTarget,
    },
    CreateAndBindAutomationClip {
        target: SequenceAutomationTarget,
    },
    MoveAutomationClip {
        id: u32,
        start_seconds: f64,
        row_target: FixtureTarget,
    },
    SplitAutomationClip {
        id: u32,
        time_seconds: f64,
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
    /// Each name becomes a unique `snake_case` name.
    CreateMarkCollections {
        collections: Vec<NewMarkCollection>,
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
    AddMarks {
        collection_key: String,
        times_seconds: Vec<f64>,
    },
    MoveMark {
        collection_key: String,
        index: u32,
        time_seconds: f64,
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
    /// Label a mark, or clear its label with `None` or a blank label.
    SetMarkLabel {
        collection_key: String,
        index: u32,
        label: Option<String>,
    },
}

/// Edits to the layers and composition graph that sequences and rigs share.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CompositionGuiEdit {
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
    RenameGraphNode {
        node_id: String,
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
    /// Empty text removes the description.
    SetLayerDescription {
        id: u32,
        description: Option<String>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewMarkCollection {
    pub name: String,
    pub color: String,
    pub marks_seconds: Vec<f64>,
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
    /// The timeline lane to paste at; a target may have several lanes.
    pub lane: Option<u32>,
    pub time_seconds: f64,
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
        start_seconds: f64,
    },
    Duration {
        duration_seconds: f64,
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
        time_delta_seconds: f64,
        /// The lane the move started on. Each clip moves from the lane of its
        /// target nearest this one.
        anchor_lane: u32,
        lane_delta: i32,
    },
    ResizeClips {
        effect_ids: Vec<u32>,
        automation_ids: Vec<u32>,
        edge: SequenceResizeEdge,
        automation: SequenceAutomationResize,
        time_delta_seconds: f64,
    },
    EditEffects {
        effect_ids: Vec<u32>,
        edit: SequenceEffectCommonEdit,
    },
    /// Retime the selected clips together; the selection stays the same.
    ArrangeClips {
        effect_ids: Vec<u32>,
        automation_ids: Vec<u32>,
        arrangement: SequenceClipArrangement,
    },
    MoveMarks {
        marks: Vec<SequenceMarkRef>,
        time_delta_seconds: f64,
    },
}

/// One clip of a sequence, effect or automation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceClipRef {
    Effect { id: u32 },
    Automation { id: u32 },
}

/// How "Align" and "Arrange" retime selected clips. A reference clip keeps
/// its timing; resized automation clips crop their curves. A row is one lane's
/// effects or one lane's automation clips.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceClipArrangement {
    /// Starts move to the reference's start; ends stay.
    AlignStarts { reference: SequenceClipRef },
    /// Ends move to the reference's end; starts stay.
    AlignEnds { reference: SequenceClipRef },
    /// Clips move, keeping their length, to start with the reference.
    MoveStarts { reference: SequenceClipRef },
    /// Clips move, keeping their length, to end with the reference.
    MoveEnds { reference: SequenceClipRef },
    /// Clips take the reference's start and end.
    AlignBoth { reference: SequenceClipRef },
    /// Clips move, keeping their length, to share the reference's midpoint.
    AlignCenters { reference: SequenceClipRef },
    /// Clips keep their start and take the reference's length.
    MatchDuration { reference: SequenceClipRef },
    /// Clips move, keeping their length, to start where the reference ends.
    StartAfter { reference: SequenceClipRef },
    /// Clips move, keeping their length, to end where the reference starts.
    EndBefore { reference: SequenceClipRef },
    /// Each start moves to the nearest mark of `mark_collections` and each end
    /// to the nearest later one; with no later mark, the clip keeps its length.
    QuantizeToMarks { mark_collections: Vec<String> },
    /// Each start moves back and each end forward to the nearest mark of
    /// `mark_collections`; an edge with no mark beyond it stays.
    StretchToMarks { mark_collections: Vec<String> },
    /// Each end extends to the next clip's start in its row.
    FillGaps,
    /// In each row, clips move, keeping their length, so each starts where
    /// the one before it ends.
    ButtTogether,
    /// In each row, the first and last clips stay and the clips between move
    /// so the gaps between them are equal.
    DistributeEvenly,
    /// Each lane's clips move by `step_seconds` more than the lane above's,
    /// or the lane below's when `reverse`.
    Cascade { step_seconds: f64, reverse: bool },
    /// Clips keep their start and take the average length.
    EqualizeDurations,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceEffectParamValue {
    Int {
        value: f32,
    },
    Float {
        value: f32,
    },
    Bool {
        value: bool,
    },
    Color {
        value: String,
    },
    Enum {
        value: String,
    },
    Curve {
        value: SequenceCurveValue,
    },
    Gradient {
        value: SequenceGradientValue,
    },
    IntArray {
        values: Vec<f32>,
    },
    FloatArray {
        values: Vec<f32>,
    },
    BoolArray {
        values: Vec<bool>,
    },
    ColorArray {
        values: Vec<String>,
    },
    CurveArray {
        values: Vec<SequenceCurveValue>,
    },
    GradientArray {
        values: Vec<SequenceGradientValue>,
    },
    /// `None` is no collection: the parameter has no marks.
    Marks {
        key: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceCurveValue {
    pub points: Vec<SequenceCurvePoint>,
    pub source: SequenceLibrarySource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGradientValue {
    pub stops: Vec<SequenceGradientStop>,
    pub source: SequenceLibrarySource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGradientStop {
    pub time: f32,
    pub value: String,
}

/// Beats and downbeats detected in a sequence's audio, in ascending seconds.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceBeatDetection {
    pub beats_seconds: Vec<f64>,
    pub downbeats_seconds: Vec<f64>,
}
