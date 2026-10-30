//! The serialized editor contract shared with the frontend, one module per
//! workflow. Tauri-Specta exports its TypeScript representation in bindings.ts.
use serde::{Deserialize, Serialize};
use specta::Type;

mod app;
mod audio;
mod diagnostics;
mod live;
mod output;
mod patch;
mod perform;
mod preview;
mod sequence;
mod setup;
mod synchronization;
mod view_state;
mod workspace;
pub use app::{AppSnapshot, ProjectHealth};
pub use audio::{
    AudioTransportSnapshot, AudioTransportState, PlaybackFrameTiming, PlaybackRange, PlaybackSpeed,
};
pub use diagnostics::{Rotation3Degrees, Scale3};
pub use donder_project_io::{IoDiagnostic, TextPosition, TextRange};
pub use live::{
    ConsoleAction, ConsoleAutomation, ConsoleClip, ConsoleClipEdit, ConsoleControlRef,
    ConsoleEventKind, ConsoleGuiDocument, ConsoleGuiEdit, ConsoleInput, ConsoleInputControl,
    ConsoleInputDevice, ConsoleLengthSource, ConsoleLiveMarks, ConsoleOn, ConsolePlayLength,
    ConsolePlayedQuantity, ConsolePlayedValue, GuiControlInput, GuiInputControl,
    GuiInputControlKind, GuiInputDriver, GuiInputLimits, InputDeviceGuiDocument, ProjectGuiEdit,
    SetupInputDevice,
};
pub use output::{
    ControllerOutputTest, DonderDeviceClaim, DonderDeviceConnection, DonderDeviceNetwork,
    DonderDeviceNetworkRequest, DonderDeviceStatus, LiveOutputControllerSnapshot,
    LiveOutputControllerState, LiveOutputSnapshot, LiveOutputState, Point3Meters,
    SequenceExportOptions, SequenceExportPort, VideoExportProgress,
};
pub use patch::{
    GuiPixelEncoding, GuiPixelRoute, GuiPixelSpan, PatchFixtureTarget, PatchGuiDocument,
    PatchLayout,
};
pub use perform::{
    PerformButton, PerformConsole, PerformMidi, PerformOsc, PerformSession, PerformSnapshot,
};
pub use preview::{
    FixtureGuiDocument, FixtureGuiEdit, FixtureStorage, GeometryRenderBounds, GuiFixtureElement,
    GuiFixtureHandle, GuiFixtureShape, GuiGridAxis, GuiGridCorner, GuiLayoutFixture,
    GuiLayoutFixtureKind, GuiOwnedFixture, LayoutGuiDocument, LayoutGuiEdit, PreviewAppearance,
    SpatialRenderPixel, SpatialRenderPlan,
};
pub use sequence::{
    SequenceAudio, SequenceAutomationBinding, SequenceAutomationClip, SequenceClip,
    SequenceClipDetails, SequenceClipDetailsResult, SequenceClipMatchRequest, SequenceClipParam,
    SequenceClipParamMatch, SequenceClipRaster, SequenceClipRasterError, SequenceClipRasterRequest,
    SequenceClipRasterResponse, SequenceClipRasterResultBatch, SequenceCompositionGraph,
    SequenceCurveLibraryItem, SequenceDetachedAutomationBinding, SequenceEffectDefinition,
    SequenceEffectDefinitionParam, SequenceGradientLibraryItem, SequenceGraphNode,
    SequenceGraphNodeKind, SequenceGraphOperatorDefinition, SequenceGraphPortDefinition,
    SequenceGuiDocument, SequenceLane, SequenceLaneKind, SequenceLayer, SequenceMarkCollection,
    SequenceParamAutomation, SequenceParamRange,
};
pub use setup::{
    BufferExternalState, ControllerGuiDocument, CurveGuiDocument, GradientGuiDocument, GuiDocument,
    GuiDocumentChange, GuiDocumentRequest, GuiEditCommand, GuiEditUpdate, GuiItem, GuiObjectRef,
    GuiOwnershipEdit, OwnedObjectSlot, OwnershipMember, ProjectGuiDocument, ReusableStorage,
    SetupController, SetupControllerConfig, SetupControllerPort, SetupGuiDocument, SetupGuiEdit,
};
pub use synchronization::{
    DocumentSaveState, DocumentSaveStatus, DocumentText, DocumentTextEdit, DocumentTextEdits,
    DocumentUpdate, ExternalConflictDecision, GuiDocumentResult, TransitionDecision,
    TransitionRequest, TransitionResult, WorkspaceTransition,
};
pub use view_state::{
    PersistedEditorViewState, PersistedEditorViewStateUpdate, PersistedGraphNodeSize,
    PersistedGraphViewState, PersistedGraphViewStateUpdate, PersistedGraphViewport,
    PersistedPreviewWindowState, PersistedSequenceViewportState,
    PersistedSequenceViewportStateUpdate, PersistedSpatialViewState,
    PersistedSpatialViewStateUpdate, PersistedWindowState, ProjectRestoreState, SpatialGuide,
    SpatialGuideAxis, ViewStateUpdate,
};
pub use workspace::{
    AppCommandId, AppSettings, ClipRasterSettings, CommandId, DocumentDescriptor,
    DocumentObjectDescriptor, EditorBuffer, EditorCommandId, EditorTab, EditorViewMode, KeyBinding,
    NewSequenceRequest, NewSequenceResult, NewSequenceStorage, ObjectKind, ProjectSearchMatch,
    ProjectSearchMatchKind, ProjectSearchRequest, ProjectSearchResponse, SequenceClipMatch,
    SequenceClipTimeMatch, SequenceFollowMode, SequenceInitialZoomMode, Shortcut, SidebarView,
    SpatialSnapSettings, SpatialUnit, TextDocumentSyntax, Transform, WorkspaceEntry,
    WorkspaceEntryKind, WorkspaceEntryPlainRole, WorkspaceEntryRole, WorkspaceExplorerState,
    WorkspaceLayoutState, WorkspaceOperation, WorkspacePathChangeImpact, WorkspacePathChangePlan,
    WorkspacePathChangeRequest,
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
    ClipParam { clip_id: u32, param: String },
    CompositionNodeParam { node_id: String, param: String },
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
    AddClip {
        initial_color: String,
        effect: SequenceEffectReference,
        target: FixtureTarget,
        scope: SequenceClipScope,
        start_seconds: f64,
    },
    ChangeEffectDefinition {
        initial_color: String,
        id: u32,
        effect: SequenceEffectReference,
    },
    /// Cut each clip at `time_seconds` into two back-to-back clips. The right
    /// half shares the left half's automation, so automation continues across
    /// the cut.
    SplitClips {
        ids: Vec<u32>,
        time_seconds: f64,
    },
    RetargetClip {
        id: u32,
        target: FixtureTarget,
    },
    AddAutomationClip {
        start_seconds: f64,
        duration_seconds: f64,
        row_target: FixtureTarget,
    },
    CreateAndBindAutomationClip {
        target: SequenceAutomationTarget,
    },
    SplitAutomationClip {
        id: u32,
        time_seconds: f64,
    },
    UpdateAutomationCurve {
        id: u32,
        curve: Vec<SequenceCurvePoint>,
    },
    BindAutomationParam {
        automation_clip_id: u32,
        target: SequenceAutomationTarget,
    },
    UnbindAutomationParam {
        automation_clip_id: u32,
        target: SequenceAutomationTarget,
    },
    RebindDetachedAutomation {
        automation_clip_id: u32,
        detached_index: u32,
        target: SequenceAutomationTarget,
    },
    DiscardDetachedAutomation {
        automation_clip_id: u32,
        detached_index: u32,
    },
    /// Each name becomes a unique `snake_case` name; the edit reports the
    /// names made, in order.
    CreateMarkCollections {
        collections: Vec<NewMarkCollection>,
    },
    AddMarks {
        collection_key: String,
        times_seconds: Vec<f64>,
    },
    ReassignMarkCollection {
        collection_key: String,
        index: u32,
        target_collection_key: String,
    },
    /// Label a mark, or clear its label with `None` or a blank label.
    SetMarkLabel {
        collection_key: String,
        index: u32,
        label: Option<String>,
    },
    /// Remove the selection after copying it to the clipboard.
    CutSelection {
        selection: SequenceSelection,
    },
    DeleteSelection {
        selection: SequenceSelection,
    },
    /// Paste the clipboard; the pasted items become the selection.
    Paste {
        anchor: SequencePasteAnchor,
    },
    MoveClips {
        clips: Vec<SequenceClipRef>,
        time_delta_seconds: f64,
        /// The lane the move started on. Each clip moves from the lane of its
        /// target nearest this one.
        anchor_lane: u32,
        lane_delta: i32,
    },
    ResizeClips {
        clips: Vec<SequenceClipRef>,
        edge: SequenceResizeEdge,
        automation: SequenceAutomationResize,
        time_delta_seconds: f64,
    },
    /// Set a property of clips.
    EditClips {
        clips: Vec<SequenceClipRef>,
        edit: SequenceClipCommonEdit,
    },
    /// Retime the clips together; the selection stays the same.
    ArrangeClips {
        clips: Vec<SequenceClipRef>,
        arrangement: SequenceClipArrangement,
    },
    MoveMarks {
        marks: Vec<SequenceMarkRef>,
        time_delta_seconds: f64,
    },
}

/// Edits to the layers and composition graph that sequences and consoles share.
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
    SetLayerEnabled {
        id: u32,
        enabled: bool,
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
        value: SequenceParamValue,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewMarkCollection {
    pub(crate) name: String,
    pub(crate) color: String,
    pub(crate) marks_seconds: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceMarkRef {
    pub(crate) collection_key: String,
    pub(crate) index: u32,
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
    Clips { clips: Vec<SequenceClipRef> },
    Marks { marks: Vec<SequenceMarkRef> },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceClipCommonEdit {
    Layer {
        layer_id: u32,
    },
    Scope {
        scope: SequenceClipScope,
    },
    Start {
        start_seconds: f64,
    },
    Duration {
        duration_seconds: f64,
    },
    Param {
        name: String,
        value: SequenceParamValue,
    },
}

/// A clip or an automation clip of a sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceClipRef {
    Clip { id: u32 },
    Automation { id: u32 },
}

/// How "Align" and "Arrange" retime selected clips. A reference clip keeps
/// its timing; resized automation clips crop their curves. A row is one lane's
/// clips or one lane's automation clips.
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
pub enum SequenceParamValue {
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
    pub link: SequenceLibraryLink,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGradientValue {
    pub stops: Vec<SequenceGradientStop>,
    pub link: SequenceLibraryLink,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceLibraryLink {
    Inline,
    Library {
        path: String,
        object_key: String,
        display_name: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGraphOperator {
    pub path: String,
    pub object_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGraphNodePosition {
    pub(crate) node_id: String,
    pub(crate) x: f32,
    pub(crate) y: f32,
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
pub enum SequenceParamKind {
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
pub enum SequenceClipScope {
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
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectReference {
    pub path: String,
    pub effect_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceCurvePoint {
    pub(crate) time: f32,
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
    pub(crate) time: f32,
    pub value: String,
}

/// Beats and downbeats detected in a sequence's audio, in ascending seconds.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceBeatDetection {
    pub beats_seconds: Vec<f64>,
    pub downbeats_seconds: Vec<f64>,
}
