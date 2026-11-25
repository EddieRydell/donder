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
mod problem;
mod sequence;
mod setup;
mod synchronization;
mod view_state;
mod workspace;
pub use app::{AppSnapshot, EditHistoryState, ProjectHealth};
pub use audio::{
    AudioTransportSnapshot, AudioTransportState, PlaybackFrameTiming, PlaybackRange, PlaybackSpeed,
};
pub use diagnostics::{Rotation3Degrees, Scale3};
pub use donder_project_io::{IoDiagnostic, TextPosition, TextRange};
pub use live::{
    ConsoleAction, ConsoleControlRef, ConsoleEventKind, ConsoleGuiDocument, ConsoleGuiEdit,
    ConsoleInput, ConsoleInputControl, ConsoleInputDevice, ConsoleLengthSource, ConsoleOn,
    ConsoleParamSource, ConsolePlayLength, ConsolePlayMode, ConsolePlayedQuantity,
    ConsolePlayedValue, GuiControlInput, GuiInputControl, GuiInputControlKind, GuiInputDriver,
    GuiInputLimits, InputDeviceGuiDocument, LearnedInput, ProjectGuiEdit, SetupInputDevice,
};
pub use output::{
    DonderDeviceConnection, DonderDeviceNetwork, DonderDeviceNetworkRequest, DonderDeviceStatus,
    ExportProgress, LiveOutputControllerSnapshot, LiveOutputControllerState, LiveOutputSnapshot,
    LiveOutputState, OutputTestPattern, OutputTestTarget, Point3Meters, SequenceExportOptions,
    SequenceExportPort,
};
pub use patch::{
    GuiChannelPlace, GuiPixelEncoding, GuiPixelRoute, GuiPixelSpan, GuiRoutePlacement,
    GuiRouteStart, PatchFixtureTarget, PatchGuiDocument,
};
pub use perform::{
    PerformButton, PerformConsole, PerformMidi, PerformOsc, PerformSession, PerformSnapshot,
};
pub use preview::{
    FixtureGuiDocument, FixtureGuiEdit, GeometryRenderBounds, GuiCaptureController,
    GuiFixtureCapture, GuiFixtureElement, GuiFixtureHandle, GuiFixtureShape, GuiGridAxis,
    GuiGridCorner, GuiGroupMember, GuiLayoutFixture, GuiLayoutFixtureKind, LayoutGuiDocument,
    LayoutGuiEdit, PreviewAppearance, SpatialRenderPixel, SpatialRenderPlan,
};
pub use problem::{GuiField, GuiMember, GuiProblem, GuiSite};
pub use sequence::{
    DerivedDataStatus, GuiClip, GuiClipContent, GuiPlacement, SequenceAudio,
    SequenceAutomationBinding, SequenceClipDetails, SequenceClipDetailsResult,
    SequenceClipMatchRequest, SequenceClipParam, SequenceClipParamMatch, SequenceClipRaster,
    SequenceClipRasterRequest, SequenceClipRasterResponse, SequenceClipRasterResultBatch,
    SequenceCompositionGraph, SequenceCurveLibraryItem, SequenceEffectDefinition,
    SequenceEffectDefinitionParam, SequenceGradientLibraryItem, SequenceGraphNode,
    SequenceGraphNodeKind, SequenceGraphOperatorDefinition, SequenceGuiDocument, SequenceLane,
    SequenceLaneKind, SequenceLayer, SequenceMarkCollection, SequenceParamAutomation,
    SequenceParamOption, SequenceParamRange,
};
pub use setup::{
    BufferExternalState, ControllerGuiDocument, CurveGuiDocument, GradientGuiDocument, GuiDocument,
    GuiDocumentChange, GuiDocumentRequest, GuiEdit, GuiEditUpdate, GuiMark, GuiObjectRef,
    GuiUniverses, GuiValue, ProjectGuiDocument, SetupController, SetupControllerConfig,
    SetupControllerPort, SetupDonderOutput,
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
    ObjectKind, ProjectSearchMatch, ProjectSearchMatchKind, ProjectSearchRequest,
    ProjectSearchResponse, SequenceClipMatch, SequenceClipTimeMatch, SequenceFollowMode,
    SequenceInitialZoomMode, Shortcut, SidebarView, SpatialSnapSettings, SpatialUnit,
    TextDocumentSyntax, Transform, WorkspaceEntry, WorkspaceEntryKind, WorkspaceEntryPlainRole,
    WorkspaceEntryRole, WorkspaceExplorerState, WorkspaceLayoutState, WorkspaceOperation,
    WorkspacePathChangeImpact, WorkspacePathChangePlan, WorkspacePathChangeRequest,
};

// Shared serialized edit contract for desktop and browser clients.
// Tauri-Specta exports the TypeScript representation in bindings.ts.

/// The part of a normalized `0..1` value a binding uses, as fractions: 0 lands
/// on `low` and 1 on `high`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiWindow {
    pub(crate) low: f32,
    pub(crate) high: f32,
}

impl From<donder_model::Window> for GuiWindow {
    fn from(window: donder_model::Window) -> Self {
        Self {
            low: window.low,
            high: window.high,
        }
    }
}

impl From<GuiWindow> for donder_model::Window {
    fn from(window: GuiWindow) -> Self {
        Self {
            low: window.low,
            high: window.high,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceAutomationTarget {
    ClipParam { clip_id: u32, param: String },
    CompositionNodeParam { node_id: u32, param: String },
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
    AddClip {
        target: FixtureTarget,
        start_seconds: f64,
        duration_seconds: f64,
        content: NewClipContent,
    },
    /// Cut each clip at `time_seconds` into two back-to-back clips. An
    /// envelope's curve is cut; the right half of an effect clip shares the
    /// left half's envelopes, so they continue across the cut.
    SplitClips {
        ids: Vec<u32>,
        time_seconds: f64,
    },
    /// A new envelope over `target`'s clip or the whole sequence, bound to it.
    AddEnvelopeFor {
        target: SequenceAutomationTarget,
    },
    /// Envelope `clip` also drives `target`.
    Bind {
        clip: u32,
        target: SequenceAutomationTarget,
    },
    AddMarks {
        collection_id: u32,
        times_seconds: Vec<f64>,
    },
    ReassignMarkCollection {
        collection_id: u32,
        index: u32,
        target_collection_id: u32,
    },
    DeleteSelection {
        selection: SequenceSelection,
    },
    MoveClips {
        ids: Vec<u32>,
        time_delta_seconds: f64,
        /// The lane the move started on. Each clip moves from the lane of its
        /// target nearest this one.
        anchor_lane: u32,
        lane_delta: i32,
    },
    ResizeClips {
        ids: Vec<u32>,
        edge: SequenceResizeEdge,
        automation: SequenceAutomationResize,
        time_delta_seconds: f64,
    },
    /// Retime the clips together; the selection stays the same.
    ArrangeClips {
        ids: Vec<u32>,
        arrangement: SequenceClipArrangement,
    },
    MoveMarks {
        marks: Vec<SequenceMarkRef>,
        time_delta_seconds: f64,
    },
}

/// What a new clip plays.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NewClipContent {
    /// An effect on the first layer, named after it.
    Effect {
        initial_color: String,
        effect: SequenceEffectReference,
        scope: SequenceClipScope,
    },
    /// An envelope that binds nothing yet.
    Envelope,
}

/// Edits to the layers and composition graph that sequences and consoles share.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CompositionGuiEdit {
    /// Each name becomes a unique `snake_case` name; the edit reports the
    /// names made, in order. A console's collections start empty.
    AddMarkCollections {
        collections: Vec<NewMarkCollection>,
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
        node_ids: Vec<u32>,
        layer_ids: Vec<u32>,
        edges: Vec<SequenceGraphEdge>,
    },
    ConnectGraphNodes {
        connection: SequenceGraphEdge,
    },
    ReconnectGraphEdge {
        previous: SequenceGraphEdge,
        connection: SequenceGraphEdge,
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
    pub(crate) collection_id: u32,
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
    Clips { ids: Vec<u32> },
    Marks { marks: Vec<SequenceMarkRef> },
}

/// How "Align" and "Arrange" retime selected clips. A reference clip keeps
/// its timing; resized envelopes crop their curves. A row is one lane's
/// effect clips or one lane's envelopes.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceClipArrangement {
    /// Starts move to the reference's start; ends stay.
    AlignStarts { reference: u32 },
    /// Ends move to the reference's end; starts stay.
    AlignEnds { reference: u32 },
    /// Clips move, keeping their length, to start with the reference.
    MoveStarts { reference: u32 },
    /// Clips move, keeping their length, to end with the reference.
    MoveEnds { reference: u32 },
    /// Clips take the reference's start and end.
    AlignBoth { reference: u32 },
    /// Clips move, keeping their length, to share the reference's midpoint.
    AlignCenters { reference: u32 },
    /// Clips keep their start and take the reference's length.
    MatchDuration { reference: u32 },
    /// Clips move, keeping their length, to start where the reference ends.
    StartAfter { reference: u32 },
    /// Clips move, keeping their length, to end where the reference starts.
    EndBefore { reference: u32 },
    /// Each start moves to the nearest mark of `mark_collections` and each end
    /// to the nearest later one; with no later mark, the clip keeps its length.
    QuantizeToMarks { mark_collections: Vec<u32> },
    /// Each start moves back and each end forward to the nearest mark of
    /// `mark_collections`; an edge with no mark beyond it stays.
    StretchToMarks { mark_collections: Vec<u32> },
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
    /// An option and, in order, every field it carries.
    Enum {
        value: String,
        fields: Vec<SequenceParamField>,
    },
    Curve {
        value: SequenceCurveValue,
    },
    Gradient {
        value: SequenceGradientValue,
    },
    /// Each item valued as the array's element kind.
    Array {
        values: Vec<SequenceParamValue>,
    },
    /// A mark collection by id; `None` is no collection: the parameter has no
    /// marks.
    Marks {
        id: Option<u32>,
    },
    Text {
        value: String,
    },
    /// `None` is no font: every character is blank.
    Font {
        value: Option<SequenceFont>,
    },
    /// `None` is no image: every point is black.
    Image {
        value: Option<SequenceImage>,
    },
}

/// An image file, by its project-relative path, scaled so its longer side
/// is at most `size` pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceImage {
    pub file: String,
    pub size: u32,
}

/// A font file, by its project-relative path, drawn at a line height in
/// pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceFont {
    pub file: String,
    pub height: u32,
}

/// An enum option's field and its value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceParamField {
    pub name: String,
    pub value: SequenceParamValue,
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
    pub gradient: SequenceGradient,
    pub link: SequenceLibraryLink,
}

/// A gradient's value: the color space it interpolates in and its stops.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGradient {
    pub space: SequenceGradientSpace,
    pub stops: Vec<SequenceGradientStop>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceGradientSpace {
    LinearRgb,
    Hsv,
    Oklab,
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
    pub(crate) node_id: u32,
    pub(crate) x: f32,
    pub(crate) y: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGraphEdge {
    pub from_node: u32,
    pub to_node: u32,
    /// The operator input the edge feeds; `None` for the root's one input.
    pub input: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceParamKind {
    Int,
    Float,
    Bool,
    Color,
    Enum,
    Curve,
    Gradient,
    Array { element: Box<SequenceParamKind> },
    Marks,
    Text,
    Font,
    Image,
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

/// How a resize treats an envelope's curve.
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
