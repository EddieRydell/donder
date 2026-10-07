use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewSequenceRequest {
    pub storage: NewSequenceStorage,
    pub initial_color: String,
    pub duration_seconds: f32,
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
    pub sequence_initial_zoom_mode: SequenceInitialZoomMode,
    pub sequence_follow_mode: SequenceFollowMode,
    pub sequence_initial_px_per_second: f32,
    pub sequence_initial_lane_height_px: f32,
    #[serde(default)]
    pub sequence_spectrogram_enabled: bool,
    #[serde(default = "default_sequence_spectrogram_time_resolution_ms")]
    pub sequence_spectrogram_time_resolution_ms: f32,
    #[serde(default = "default_sequence_spectrogram_fft_size")]
    pub sequence_spectrogram_fft_size: u32,
    /// Marks draw only in the Marks lane instead of also across the waveform and lanes.
    #[serde(default)]
    pub sequence_marks_lane_only: bool,
    pub effect_raster: EffectRasterSettings,
    pub spatial_snap: SpatialSnapSettings,
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
            sequence_initial_px_per_second: 80.0,
            sequence_initial_lane_height_px: 42.0,
            sequence_spectrogram_enabled: false,
            sequence_spectrogram_time_resolution_ms:
                default_sequence_spectrogram_time_resolution_ms(),
            sequence_spectrogram_fft_size: default_sequence_spectrogram_fft_size(),
            sequence_marks_lane_only: false,
            effect_raster: EffectRasterSettings::default(),
            spatial_snap: SpatialSnapSettings::default(),
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
pub struct EffectRasterSettings {
    pub render_scale: f32,
    pub max_columns: u32,
    pub max_rows: u32,
    pub min_frame_stride: u32,
}

impl Default for EffectRasterSettings {
    fn default() -> Self {
        Self {
            render_scale: 1.0,
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

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ObjectKind {
    Project,
    Setup,
    Controller,
    Layout,
    Fixture,
    Patch,
    Sequence,
    Curve,
    Gradient,
    Effect,
    Operator,
}

impl From<&SourceObjectKind> for ObjectKind {
    fn from(kind: &SourceObjectKind) -> Self {
        match kind {
            SourceObjectKind::Project => Self::Project,
            SourceObjectKind::Setup => Self::Setup,
            SourceObjectKind::Controller => Self::Controller,
            SourceObjectKind::Layout => Self::Layout,
            SourceObjectKind::Patch => Self::Patch,
            SourceObjectKind::FixtureDefinition => Self::Fixture,
            SourceObjectKind::Curve => Self::Curve,
            SourceObjectKind::Gradient => Self::Gradient,
            SourceObjectKind::Sequence => Self::Sequence,
            SourceObjectKind::EffectDefinition | SourceObjectKind::EffectInstance => Self::Effect,
            SourceObjectKind::OperatorDefinition => Self::Operator,
        }
    }
}

impl ObjectKind {
    pub fn document_view(&self) -> Option<DocumentViewId> {
        match self {
            Self::Project => Some(DocumentViewId::Project),
            Self::Setup => Some(DocumentViewId::Setup),
            Self::Layout => Some(DocumentViewId::Layout),
            Self::Fixture => Some(DocumentViewId::Fixture),
            Self::Patch => Some(DocumentViewId::Patch),
            Self::Controller => Some(DocumentViewId::Controller),
            Self::Sequence => Some(DocumentViewId::Sequence),
            Self::Curve => Some(DocumentViewId::Curve),
            Self::Gradient => Some(DocumentViewId::Gradient),
            Self::Effect | Self::Operator => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceEntryKind {
    Directory,
    File,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceEntryRole {
    Directory,
    Project,
    Entrypoint,
    Setup,
    Controller,
    Layout,
    Fixture,
    Patch,
    Curve,
    Gradient,
    Effect,
    Operator,
    Sequence,
    Asset,
    File,
}

pub fn workspace_role_for_source_object(kind: &SourceObjectKind) -> WorkspaceEntryRole {
    match kind {
        SourceObjectKind::Project => WorkspaceEntryRole::Project,
        SourceObjectKind::Setup => WorkspaceEntryRole::Setup,
        SourceObjectKind::Controller => WorkspaceEntryRole::Controller,
        SourceObjectKind::Layout => WorkspaceEntryRole::Layout,
        SourceObjectKind::FixtureDefinition => WorkspaceEntryRole::Fixture,
        SourceObjectKind::Patch => WorkspaceEntryRole::Patch,
        SourceObjectKind::Curve => WorkspaceEntryRole::Curve,
        SourceObjectKind::Gradient => WorkspaceEntryRole::Gradient,
        SourceObjectKind::Sequence => WorkspaceEntryRole::Sequence,
        SourceObjectKind::EffectDefinition | SourceObjectKind::EffectInstance => {
            WorkspaceEntryRole::Effect
        }
        SourceObjectKind::OperatorDefinition => WorkspaceEntryRole::Operator,
    }
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
pub struct DocumentDefaultObjectKey {
    pub view: DocumentViewId,
    pub object_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocumentDescriptor {
    pub path: String,
    pub objects: Vec<DocumentObjectDescriptor>,
    pub available_views: Vec<DocumentViewId>,
    pub default_object_keys: Vec<DocumentDefaultObjectKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocumentObjectDescriptor {
    pub key: String,
    pub kind: ObjectKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EditorBuffer {
    pub path: String,
    pub name: String,
    pub syntax: TextDocumentSyntax,
    pub text: String,
    pub dirty: bool,
    pub read_only: bool,
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
pub struct TextPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TextRange {
    pub start: TextPosition,
    pub end: TextPosition,
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
    pub enabled: bool,
    pub spacing_meters: f64,
    pub unit: SpatialUnit,
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
