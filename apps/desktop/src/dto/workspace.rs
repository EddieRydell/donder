use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewSequenceRequest {
    pub file_path: String,
    pub object_key: String,
    pub duration_seconds: f32,
    pub frame_rate: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceLayoutState {
    pub sidebar_width_px: f32,
    pub inspector_width_px: f32,
    pub sidebar_collapsed: bool,
    pub inspector_collapsed: bool,
    pub active_sidebar_view: SidebarView,
}

impl<'de> Deserialize<'de> for WorkspaceLayoutState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct StoredLayout {
            sidebar_width_px: Option<f32>,
            project_tree_width_px: Option<f32>,
            inspector_width_px: f32,
            sidebar_collapsed: Option<bool>,
            project_tree_collapsed: Option<bool>,
            inspector_collapsed: bool,
            #[serde(default)]
            active_sidebar_view: SidebarView,
        }
        let stored = StoredLayout::deserialize(deserializer)?;
        Ok(Self {
            sidebar_width_px: stored
                .sidebar_width_px
                .or(stored.project_tree_width_px)
                .unwrap_or(288.0),
            inspector_width_px: stored.inspector_width_px,
            sidebar_collapsed: stored
                .sidebar_collapsed
                .or(stored.project_tree_collapsed)
                .unwrap_or(false),
            inspector_collapsed: stored.inspector_collapsed,
            active_sidebar_view: stored.active_sidebar_view,
        })
    }
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
    Packages,
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
    pub sequence_initial_px_per_second: f32,
    pub sequence_initial_lane_height_px: f32,
    pub effect_raster: EffectRasterSettings,
}

fn default_editor_view_mode() -> EditorViewMode {
    EditorViewMode::Gui
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            reopen_last_project: true,
            editor_view_mode: EditorViewMode::Gui,
            reopen_preview_window: true,
            autosave_project_edits: true,
            sequence_initial_zoom_mode: SequenceInitialZoomMode::FitToWidth,
            sequence_initial_px_per_second: 80.0,
            sequence_initial_lane_height_px: 42.0,
            effect_raster: EffectRasterSettings::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceInitialZoomMode {
    FitToWidth,
    FixedPxPerSecond,
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
            max_columns: 256,
            max_rows: 50,
            min_frame_stride: 4,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum EditorViewMode {
    Text,
    Gui,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
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
    pub(crate) fn document_view(&self) -> Option<DocumentViewId> {
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
pub enum SequenceEffectDefinitionKind {
    Sample,
    Generator,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceResizeEdge {
    Left,
    Right,
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
    Layout,
    Fixture,
    Patch,
    Curve,
    Gradient,
    Effect,
    Operator,
    Sequence,
    Manifest,
    Lockfile,
    Asset,
    PathDependency,
    File,
}

pub(crate) fn workspace_role_for_source_object(kind: &SourceObjectKind) -> WorkspaceEntryRole {
    match kind {
        SourceObjectKind::Project => WorkspaceEntryRole::Project,
        SourceObjectKind::Setup => WorkspaceEntryRole::Setup,
        SourceObjectKind::Layout | SourceObjectKind::FixtureDefinition => {
            WorkspaceEntryRole::Layout
        }
        SourceObjectKind::Patch => WorkspaceEntryRole::Patch,
        SourceObjectKind::Curve => WorkspaceEntryRole::Curve,
        SourceObjectKind::Gradient => WorkspaceEntryRole::Gradient,
        SourceObjectKind::Sequence => WorkspaceEntryRole::Sequence,
        SourceObjectKind::EffectDefinition | SourceObjectKind::EffectInstance => {
            WorkspaceEntryRole::Effect
        }
        SourceObjectKind::OperatorDefinition => WorkspaceEntryRole::Operator,
        SourceObjectKind::Controller => WorkspaceEntryRole::File,
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceEntryOwnership {
    Project,
    PathDependency,
    Registry,
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
pub enum WorkspacePathOwnership {
    Project,
    PathDependency {
        module_id: String,
        module_root: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePathChangeImpact {
    pub documents: Vec<String>,
    pub imports: Vec<String>,
    pub manifests: Vec<String>,
    pub assets: Vec<String>,
    pub modules: Vec<String>,
    pub open_files: Vec<String>,
    pub recent_files: Vec<String>,
    pub persisted_state: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePathChangePlan {
    pub request: WorkspacePathChangeRequest,
    pub structural: bool,
    pub ownership: WorkspacePathOwnership,
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
    pub text: String,
    pub dirty: bool,
    pub read_only: bool,
    pub document_revision: u32,
    pub saved_revision: u32,
    pub save_state: DocumentSaveState,
    pub external_state: BufferExternalState,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceEffectReference {
    Builtin {
        effect: SequenceBuiltinEffect,
    },
    Custom {
        module_id: String,
        path: String,
        effect_name: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceBuiltinEffect {
    Pulse,
    Chase,
    Spin,
    MarkPulse,
    MarkChase,
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
    pub ownership: WorkspaceEntryOwnership,
    pub operations: Vec<WorkspaceOperation>,
    pub operation_explanation: Option<String>,
}
