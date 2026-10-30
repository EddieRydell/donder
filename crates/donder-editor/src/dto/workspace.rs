use crate::dto::*;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewSequenceRequest {
    pub storage: NewSequenceStorage,
    pub initial_color: String,
    pub duration_seconds: f64,
    pub frame_rate: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NewSequenceStorage {
    Inline,
    SameFile { name: String },
    NewFile { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewSequenceResult {
    pub snapshot: AppSnapshot,
    pub source: GuiObjectRef,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkspaceLayoutState {
    pub sidebar_width_px: f32,
    pub inspector_width_px: f32,
    pub sidebar_collapsed: bool,
    pub inspector_collapsed: bool,
    pub active_sidebar_view: SidebarView,
}

impl Default for WorkspaceLayoutState {
    fn default() -> Self {
        Self {
            sidebar_width_px: 288.0,
            inspector_width_px: 260.0,
            sidebar_collapsed: false,
            inspector_collapsed: false,
            active_sidebar_view: SidebarView::Explorer,
        }
    }
}

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SidebarView {
    #[default]
    Explorer,
    Search,
    Problems,
}

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceExplorerState {
    pub expanded_paths: Vec<String>,
    pub recent_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub reopen_last_project: bool,
    #[serde(default = "default_editor_view_mode")]
    pub editor_view_mode: EditorViewMode,
    pub reopen_preview_window: bool,
    pub autosave_project_edits: bool,
    sequence_initial_zoom_mode: SequenceInitialZoomMode,
    sequence_follow_mode: SequenceFollowMode,
    /// Timeline drags snap to marks and clip edges; holding Alt inverts this for the gesture.
    sequence_snap: bool,
    pub sequence_clip_match: SequenceClipMatch,
    pub sequence_initial_px_per_second: f32,
    pub sequence_initial_lane_height_px: f32,
    #[serde(default)]
    sequence_spectrogram_enabled: bool,
    #[serde(default = "default_sequence_spectrogram_time_resolution_ms")]
    pub sequence_spectrogram_time_resolution_ms: f32,
    #[serde(default = "default_sequence_spectrogram_fft_size")]
    pub sequence_spectrogram_fft_size: u32,
    /// Marks draw only in the Marks lane instead of also across the waveform and lanes.
    #[serde(default)]
    sequence_marks_lane_only: bool,
    pub clip_raster: ClipRasterSettings,
    pub spatial_snap: SpatialSnapSettings,
    /// Commands whose keys differ from their defaults.
    #[serde(default)]
    pub keybindings: Vec<KeyBinding>,
}

fn default_editor_view_mode() -> EditorViewMode {
    EditorViewMode::Gui
}

fn default_sequence_spectrogram_time_resolution_ms() -> f32 {
    10.0
}

fn default_sequence_spectrogram_fft_size() -> u32 {
    2048
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            reopen_last_project: true,
            editor_view_mode: EditorViewMode::Gui,
            reopen_preview_window: true,
            autosave_project_edits: true,
            sequence_initial_zoom_mode: SequenceInitialZoomMode::FitToWidth,
            sequence_follow_mode: SequenceFollowMode::Page,
            sequence_snap: false,
            sequence_clip_match: SequenceClipMatch::default(),
            sequence_initial_px_per_second: 80.0,
            sequence_initial_lane_height_px: 42.0,
            sequence_spectrogram_enabled: false,
            sequence_spectrogram_time_resolution_ms:
                default_sequence_spectrogram_time_resolution_ms(),
            sequence_spectrogram_fft_size: default_sequence_spectrogram_fft_size(),
            sequence_marks_lane_only: false,
            clip_raster: ClipRasterSettings::default(),
            spatial_snap: SpatialSnapSettings::default(),
            keybindings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceInitialZoomMode {
    FitToWidth,
    FixedPxPerSecond,
}

/// How the sequence view tracks the playhead during playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceFollowMode {
    /// The view never moves on its own.
    Off,
    /// The view pages forward near the edge and jumps when the playhead leaves it.
    Page,
    /// The playhead holds a fixed screen position while the timeline scrolls under it.
    Continuous,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClipRasterSettings {
    pub max_columns: u32,
    pub max_rows: u32,
    pub min_frame_stride: u32,
}

impl Default for ClipRasterSettings {
    fn default() -> Self {
        Self {
            max_columns: 1024,
            max_rows: 50,
            min_frame_stride: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum EditorViewMode {
    Text,
    Gui,
}

/// A key with the modifiers it needs. `primary` is Command on macOS and Control elsewhere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Shortcut {
    /// The key as the browser names it: a lowercase character or a named key such as `ArrowLeft`.
    pub key: String,
    primary: bool,
    shift: bool,
    alt: bool,
}

/// Keys chosen for a command in place of its defaults; an empty list leaves it unbound.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct KeyBinding {
    pub command: CommandId,
    pub shortcuts: Vec<Shortcut>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(untagged)]
pub enum CommandId {
    App(AppCommandId),
    Editor(EditorCommandId),
}

/// Commands the workbench runs, whatever editor is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum AppCommandId {
    #[serde(rename = "file.newProject")]
    FileNewProject,
    #[serde(rename = "file.copyProject")]
    FileCopyProject,
    #[serde(rename = "file.newSequence")]
    FileNewSequence,
    #[serde(rename = "file.openProject")]
    FileOpenProject,
    #[serde(rename = "file.save")]
    FileSave,
    #[serde(rename = "file.reloadFromDisk")]
    FileReloadFromDisk,
    #[serde(rename = "file.closeEditor")]
    FileCloseEditor,
    #[serde(rename = "file.closeWindow")]
    FileCloseWindow,
    #[serde(rename = "file.settings")]
    FileSettings,
    #[serde(rename = "edit.undo")]
    EditUndo,
    #[serde(rename = "edit.redo")]
    EditRedo,
    #[serde(rename = "view.toggleGuiMode")]
    ViewToggleGuiMode,
    #[serde(rename = "view.toggleSpectrogram")]
    ViewToggleSpectrogram,
    #[serde(rename = "view.toggleMarksLaneOnly")]
    ViewToggleMarksLaneOnly,
    #[serde(rename = "view.openLayerGraph")]
    ViewOpenLayerGraph,
    #[serde(rename = "view.toggleProjectTree")]
    ViewToggleProjectTree,
    #[serde(rename = "view.focusExplorer")]
    ViewFocusExplorer,
    #[serde(rename = "view.focusSearch")]
    ViewFocusSearch,
    #[serde(rename = "view.focusProblems")]
    ViewFocusProblems,
    #[serde(rename = "workbench.quickOpen")]
    WorkbenchQuickOpen,
    #[serde(rename = "workbench.commandPalette")]
    WorkbenchCommandPalette,
    #[serde(rename = "project.reload")]
    ProjectReload,
    #[serde(rename = "perform.stop")]
    PerformStop,
}

/// Commands the focused editor surface implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum EditorCommandId {
    #[serde(rename = "edit.cut")]
    EditCut,
    #[serde(rename = "edit.copy")]
    EditCopy,
    #[serde(rename = "edit.paste")]
    EditPaste,
    #[serde(rename = "edit.selectAll")]
    EditSelectAll,
    #[serde(rename = "edit.duplicate")]
    EditDuplicate,
    #[serde(rename = "edit.delete")]
    EditDelete,
    #[serde(rename = "edit.confirm")]
    EditConfirm,
    #[serde(rename = "edit.cancel")]
    EditCancel,
    #[serde(rename = "edit.moveLeft")]
    EditMoveLeft,
    #[serde(rename = "edit.moveRight")]
    EditMoveRight,
    #[serde(rename = "edit.moveUp")]
    EditMoveUp,
    #[serde(rename = "edit.moveDown")]
    EditMoveDown,
    #[serde(rename = "view.reset")]
    ViewReset,
    #[serde(rename = "transport.togglePlayback")]
    TransportTogglePlayback,
    #[serde(rename = "transport.stop")]
    TransportStop,
    #[serde(rename = "transport.toggleLooping")]
    TransportToggleLooping,
    #[serde(rename = "transport.cycleFollowMode")]
    TransportCycleFollowMode,
    #[serde(rename = "transport.tapMark")]
    TransportTapMark,
    #[serde(rename = "transport.rewind")]
    TransportRewind,
    #[serde(rename = "sequence.toggleSliceTool")]
    SequenceToggleSliceTool,
    #[serde(rename = "sequence.selectMatching")]
    SequenceSelectMatching,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ObjectKind {
    Project,
    Setup,
    Controller,
    InputDevice,
    Layout,
    Fixture,
    Patch,
    Sequence,
    Rig,
    Curve,
    Gradient,
    Effect,
    Operator,
}

impl From<donder_model::ObjectKind> for ObjectKind {
    fn from(kind: donder_model::ObjectKind) -> Self {
        use donder_model::ObjectKind as Kind;
        match kind {
            Kind::Project => Self::Project,
            Kind::Setup => Self::Setup,
            Kind::Controller => Self::Controller,
            Kind::InputDevice => Self::InputDevice,
            Kind::Layout => Self::Layout,
            Kind::Patch => Self::Patch,
            Kind::FixtureDefinition => Self::Fixture,
            Kind::Curve => Self::Curve,
            Kind::Gradient => Self::Gradient,
            Kind::Sequence => Self::Sequence,
            Kind::Rig => Self::Rig,
            Kind::EffectDefinition => Self::Effect,
            Kind::OperatorDefinition => Self::Operator,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceEntryKind {
    Directory,
    File,
}

/// What an explorer entry is: an object's document, by the kind of its first
/// object, or a plain entry.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(untagged)]
pub enum WorkspaceEntryRole {
    Object(ObjectKind),
    Entry(WorkspaceEntryPlainRole),
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceEntryPlainRole {
    Directory,
    Entrypoint,
    Asset,
    File,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceOperation {
    Open,
    Create,
    Rename,
    Delete,
    Move,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSearchRequest {
    pub request_id: u32,
    pub query: String,
    pub match_case: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSearchMatch {
    pub path: String,
    pub line: u32,
    pub column: u32,
    pub preview: String,
    pub kind: ProjectSearchMatchKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ProjectSearchMatchKind {
    Filename,
    Content,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSearchResponse {
    pub request_id: u32,
    pub matches: Vec<ProjectSearchMatch>,
    pub skipped_binary: u32,
    pub skipped_oversized: u32,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePathChangeRequest {
    pub source: String,
    pub destination: String,
    pub project_revision: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePathChangeImpact {
    pub documents: Vec<String>,
    pub imports: Vec<String>,
    pub assets: Vec<String>,
    pub open_files: Vec<String>,
    pub recent_files: Vec<String>,
    pub persisted_state: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePathChangePlan {
    pub request: WorkspacePathChangeRequest,
    pub structural: bool,
    pub impact: WorkspacePathChangeImpact,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocumentDescriptor {
    pub path: String,
    pub objects: Vec<DocumentObjectDescriptor>,
    /// The objects a GUI editor opens, in the order the editor offers them.
    pub gui_objects: Vec<DocumentObjectDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocumentObjectDescriptor {
    pub key: String,
    pub kind: ObjectKind,
}

/// An open document as the tab bar and editor see it, without its text.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EditorTab {
    pub path: String,
    name: String,
    syntax: TextDocumentSyntax,
    pub dirty: bool,
    pub document_revision: u32,
    pub saved_revision: u32,
    pub save_state: DocumentSaveState,
    pub external_state: BufferExternalState,
}

impl From<&EditorBuffer> for EditorTab {
    fn from(buffer: &EditorBuffer) -> Self {
        Self {
            path: buffer.path.clone(),
            name: buffer.name.clone(),
            syntax: buffer.syntax,
            dirty: buffer.dirty,
            document_revision: buffer.document_revision,
            saved_revision: buffer.saved_revision,
            save_state: buffer.save_state.clone(),
            external_state: buffer.external_state.clone(),
        }
    }
}

/// An open document's working copy.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EditorBuffer {
    pub path: String,
    pub name: String,
    pub syntax: TextDocumentSyntax,
    pub text: String,
    pub dirty: bool,
    pub document_revision: u32,
    pub saved_revision: u32,
    pub save_state: DocumentSaveState,
    pub external_state: BufferExternalState,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum TextDocumentSyntax {
    /// A `*.data.donder` document.
    Data,
    /// A `*.donder` script.
    Script,
    Plain,
}

impl From<donder_project_io::SourceDocumentFormat> for TextDocumentSyntax {
    fn from(format: donder_project_io::SourceDocumentFormat) -> Self {
        match format {
            donder_project_io::SourceDocumentFormat::Data => Self::Data,
            donder_project_io::SourceDocumentFormat::Script => Self::Script,
            donder_project_io::SourceDocumentFormat::Other => Self::Plain,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Transform {
    pub position: Point3Meters,
    pub rotation: Rotation3Degrees,
    pub scale: Scale3,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceEntry {
    pub path: String,
    pub kind: WorkspaceEntryKind,
    pub name: String,
    pub parent: String,
    pub role: WorkspaceEntryRole,
    pub operations: Vec<WorkspaceOperation>,
    pub operation_explanation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SpatialSnapSettings {
    enabled: bool,
    pub spacing_meters: f64,
    unit: SpatialUnit,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SpatialUnit {
    Meters,
    Centimeters,
    Millimeters,
    Inches,
    Feet,
}

impl Default for SpatialSnapSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            spacing_meters: 0.1,
            unit: SpatialUnit::Meters,
        }
    }
}

/// Which properties "Select Matching" compares between each seed clip and the
/// other clips. Effect type applies only between clips.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipMatch {
    pub(crate) effect_type: bool,
    pub start: SequenceClipTimeMatch,
    pub end: SequenceClipTimeMatch,
    pub duration: SequenceClipTimeMatch,
    pub(crate) same_lane: bool,
    pub(crate) automation_clips: bool,
}

/// A time property matches within `tolerance_seconds` of the seed's.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipTimeMatch {
    pub(crate) enabled: bool,
    pub tolerance_seconds: f64,
}

impl Default for SequenceClipMatch {
    fn default() -> Self {
        let time = |enabled| SequenceClipTimeMatch {
            enabled,
            tolerance_seconds: 0.005,
        };
        Self {
            effect_type: true,
            start: time(true),
            end: time(false),
            duration: time(false),
            same_lane: false,
            automation_clips: false,
        }
    }
}
