use super::*;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiDocument {
    ElementTree {
        document: ElementTreeGuiDocument,
    },
    FixtureProfile {
        document: FixtureProfileGuiDocument,
    },
    Patch {
        document: PatchGuiDocument,
    },
    Project {
        document: ProjectGuiDocument,
    },
    Setup {
        document: SetupGuiDocument,
    },
    Sequence {
        document: SequenceGuiDocument,
    },
    Preview {
        document: PreviewGuiDocument,
    },
    Prop {
        document: PropGuiDocument,
    },
    Curve {
        document: CurveGuiDocument,
    },
    Gradient {
        document: GradientGuiDocument,
    },
    Controller {
        document: ControllerGuiDocument,
    },
    Blocked {
        reason: String,
        diagnostics: Vec<ProjectDiagnostic>,
    },
}

#[derive(Debug, Clone, Eq, Hash, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiDocumentRequest {
    pub project_revision: u32,
    pub path: String,
    pub view: DocumentViewId,
    pub object_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiObjectRef {
    pub module_id: String,
    pub path: String,
    pub object_key: String,
    pub kind: ObjectKind,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiEditCommand {
    ElementTree {
        edit: ElementTreeGuiEdit,
    },
    FixtureProfile {
        definition: GuiFixtureDefinition,
    },
    Patch {
        nodes: Vec<PatchGuiNode>,
        edges: Vec<SetupPatchEdge>,
    },
    Setup {
        edit: SetupGuiEdit,
    },
    Sequence {
        edit: SequenceGuiEdit,
    },
    Preview {
        edit: PreviewGuiEdit,
    },
    Prop {
        edit: PropGuiEdit,
    },
    Curve {
        points: Vec<SequenceCurvePoint>,
    },
    Gradient {
        stops: Vec<SequenceGradientStop>,
    },
    Controller {
        config: SetupControllerConfig,
        ports: Vec<SetupControllerPort>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiEditResult {
    pub snapshot: AppSnapshot,
    pub document: GuiDocument,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum BufferExternalState {
    Current,
    ChangedOnDisk,
    DeletedOnDisk,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGradientStop {
    pub time: f32,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Eq, Hash, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DocumentViewId {
    Text,
    Project,
    Setup,
    ElementTree,
    Preview,
    Prop,
    FixtureProfile,
    Patch,
    Controller,
    Sequence,
    Curve,
    Gradient,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CurveGuiDocument {
    pub path: String,
    pub object_key: String,
    pub points: Vec<SequenceCurvePoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GradientGuiDocument {
    pub path: String,
    pub object_key: String,
    pub stops: Vec<SequenceGradientStop>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectGuiDocument {
    pub path: String,
    pub object_key: String,
    pub setup: GuiObjectRef,
    pub sequences: Vec<GuiObjectRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupGuiDocument {
    pub path: String,
    pub source_ref: GuiObjectRef,
    pub object_key: String,
    pub elements_ref: GuiObjectRef,
    pub preview_ref: GuiObjectRef,
    pub patch_ref: GuiObjectRef,
    pub elements_read_only: bool,
    pub preview_read_only: bool,
    pub patch_read_only: bool,
    pub root_ids: Vec<u32>,
    pub elements: Vec<SetupElementNode>,
    pub fixture_profiles: Vec<SetupFixtureProfile>,
    pub preview_links: Vec<SetupPreviewLink>,
    pub patch_nodes: Vec<SetupPatchNode>,
    pub patch_edges: Vec<SetupPatchEdge>,
    pub patch_definitions: Vec<PatchGuiNode>,
    pub patch_profiles: Vec<GuiObjectRef>,
    pub output_assignments: Vec<SetupOutputAssignment>,
    pub controllers: Vec<SetupController>,
    pub available_controllers: Vec<SetupController>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupElementNode {
    pub id: u32,
    pub name: String,
    pub kind: SetupElementKind,
    pub parent: Option<u32>,
    pub children: Vec<u32>,
    pub cell_count: Option<u32>,
    pub capability: Option<GuiColorCapability>,
    pub color_component_count: Option<u32>,
    pub profile: Option<String>,
    pub control_definition: Option<SetupControlElement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SetupControlElement {
    Scalar {
        cells: u32,
    },
    Indexed {
        cells: u32,
        options: Vec<SetupIndexedOption>,
    },
    Fixture {
        profile: GuiObjectRef,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupIndexedOption {
    pub id: u32,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SetupElementKind {
    Group,
    Color,
    Scalar,
    Indexed,
    Fixture,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupFixtureProfile {
    pub id: String,
    pub name: String,
    pub function_count: u32,
    pub channel_count: u32,
    pub behavior_rule_count: u32,
    pub source_ref: GuiObjectRef,
    pub read_only: bool,
    pub definition: GuiFixtureDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FixtureProfileGuiDocument {
    pub path: String,
    pub object_key: String,
    pub definition: GuiFixtureDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupPreviewLink {
    pub prop_id: u32,
    pub name: String,
    pub definition_ref: GuiObjectRef,
    pub point_count: u32,
    pub bindings: Vec<SetupElementCell>,
    pub geometry: Geometry,
    pub bulb_diameter_meters: f32,
    pub position: Point3Meters,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupElementCell {
    pub node: u32,
    pub cell: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupPatchNode {
    pub id: u32,
    pub kind: SetupPatchNodeKind,
    pub label: String,
    pub width: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SetupPatchNodeKind {
    Source,
    Filter,
    Sink,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupPatchEdge {
    pub from_node: u32,
    pub from_port: u16,
    pub to_node: u32,
    pub to_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupOutputAssignment {
    pub sink: u32,
    pub controller: String,
    pub port: u32,
    pub start_channel: u16,
    pub channel_count: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupController {
    pub label: String,
    pub source_ref: GuiObjectRef,
    pub read_only: bool,
    pub config: SetupControllerConfig,
    pub ports: Vec<SetupControllerPort>,
    pub assignments: Vec<SetupOutputAssignment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ControllerGuiDocument {
    pub path: String,
    pub object_key: String,
    pub controller: SetupController,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupControllerPort {
    pub id: u32,
    pub address: u16,
    pub slot_count: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SetupGuiEdit {
    AssignControlOutput {
        assignment: SetupControlOutputAssignment,
        mode: SetupOutputAssignmentMode,
    },
    CopyLayout,
    CopyController {
        controller: GuiObjectRef,
    },
    AssignFixtureOutput {
        node: u32,
        controller: GuiObjectRef,
        port: u32,
        start_slot: u16,
        mode: SetupOutputAssignmentMode,
    },
    CreateFixtureProfile {
        name: String,
        definition: GuiFixtureDefinition,
    },
    RemoveOutput {
        sink: u32,
    },
    AssignPixelOutput {
        node: u32,
        controller: GuiObjectRef,
        first_port: u32,
        start_slot: u16,
        component_order: Vec<u16>,
        mode: SetupOutputAssignmentMode,
    },
    AddController {
        config: SetupControllerConfig,
        ports: Vec<SetupControllerPort>,
    },
    AttachController {
        controller: GuiObjectRef,
    },
    DetachController {
        controller: GuiObjectRef,
        remove_outputs: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SetupOutputAssignmentMode {
    Add,
    Replace,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SetupControllerConfig {
    E131 {
        source_name: String,
        bind_address: String,
        priority: u8,
        destination: Option<String>,
    },
    ArtNet {
        bind_address: String,
        destination: String,
        broadcast: bool,
    },
}

impl From<&dawn_language::controller::ControllerProtocol> for SetupControllerConfig {
    fn from(protocol: &dawn_language::controller::ControllerProtocol) -> Self {
        use dawn_language::controller::{ArtNetMode, ControllerProtocol, E131Mode};
        match protocol {
            ControllerProtocol::E131(config) => Self::E131 {
                source_name: config.source_name.clone(),
                bind_address: config.bind_address.to_string(),
                priority: config.priority,
                destination: match &config.mode {
                    E131Mode::Multicast => None,
                    E131Mode::Unicast { destination } => Some(destination.to_string()),
                },
            },
            ControllerProtocol::ArtNet(config) => Self::ArtNet {
                bind_address: config.bind_address.to_string(),
                destination: config.destination.to_string(),
                broadcast: matches!(config.mode, ArtNetMode::Broadcast),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupControlOutputAssignment {
    pub node: u32,
    pub controller: GuiObjectRef,
    pub port: u32,
    pub start_slot: u16,
    pub mapping: SetupControlOutputMapping,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SetupControlOutputMapping {
    Scalar,
    Indexed { entries: Vec<SetupIndexedChannel> },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupIndexedChannel {
    pub id: u32,
    pub value: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupPixelLight {
    pub name: String,
    pub parent: Option<u32>,
    pub capability: GuiColorCapability,
    pub geometry: Geometry,
    pub bulb_diameter_meters: f32,
    pub position: Point3Meters,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ElementTreeGuiDocument {
    pub path: String,
    pub object_key: String,
    pub source_ref: GuiObjectRef,
    pub read_only: bool,
    pub root_ids: Vec<u32>,
    pub elements: Vec<SetupElementNode>,
    pub profiles: Vec<GuiObjectRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ElementTreeGuiEdit {
    AddControlElement {
        name: String,
        parent: Option<u32>,
        definition: SetupControlElement,
    },
    UpdateControlElement {
        id: u32,
        name: String,
        definition: SetupControlElement,
    },
    UpdateColorCapability {
        id: u32,
        capability: GuiColorCapability,
        component_order: Vec<u16>,
    },
    AddGroup {
        name: String,
        parent: Option<u32>,
    },
    MoveElement {
        id: u32,
        parent: Option<u32>,
    },
    DeleteElement {
        id: u32,
    },
    RenameElement {
        id: u32,
        name: String,
    },
    ReorderElements {
        parent: Option<u32>,
        ordered_ids: Vec<u32>,
    },
}
