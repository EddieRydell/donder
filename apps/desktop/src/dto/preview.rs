use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PropDefinition {
    pub source_ref: GuiObjectRef,
    pub object_key: String,
    pub name: String,
    pub color_model: String,
    pub bulb_diameter_meters: f32,
    pub geometry: Geometry,
    pub geometry_summary: String,
    pub render_plan: GeometryRenderPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PropGuiDocument {
    pub path: String,
    pub fixture: PropDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PropGuiEdit {
    UpdateDefinition {
        geometry: Geometry,
        bulb_diameter_meters: f32,
    },
    MovePoint {
        point_index: u32,
        point: Point3Meters,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceCurvePoint {
    pub time: f32,
    pub value: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Geometry {
    Points {
        points: Vec<Point3Meters>,
    },
    Lines {
        points: Vec<Point3Meters>,
        pixels: u32,
    },
    Arc {
        center: Point3Meters,
        radius_meters: f32,
        start_degrees: f32,
        end_degrees: f32,
        pixels: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GeometryRenderBounds {
    pub min_x_meters: f32,
    pub min_y_meters: f32,
    pub max_x_meters: f32,
    pub max_y_meters: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GeometryRenderGuide {
    Line {
        from: GeometryRenderPoint,
        to: GeometryRenderPoint,
    },
    Arc {
        start: GeometryRenderPoint,
        end: GeometryRenderPoint,
        radius_x_meters: f32,
        radius_y_meters: f32,
        rotation: f32,
        large_arc: bool,
        sweep_positive: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GeometryRenderPlan {
    pub emitters: Vec<GeometryRenderPoint>,
    pub guides: Vec<GeometryRenderGuide>,
    pub bounds: GeometryRenderBounds,
    pub bulb_radius_meters: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GeometryRenderPoint {
    pub x_meters: f32,
    pub y_meters: f32,
    pub z_meters: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewGuiDocument {
    pub path: String,
    pub source_ref: GuiObjectRef,
    pub object_key: String,
    pub name: String,
    pub render_bounds: GeometryRenderBounds,
    pub fixtures: Vec<PreviewPropPlacement>,
    pub hierarchy: ElementTreeGuiDocument,
    pub available_fixtures: Vec<GuiObjectRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewPropPlacement {
    pub definition_ref: GuiObjectRef,
    pub bindings: Vec<SetupElementCell>,
    pub id: u32,
    pub name: String,
    pub transform: Transform,
    pub resolved_fixture: ResolvedPreviewProp,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PreviewGuiEdit {
    PlaceFixture {
        name: String,
        parent: Option<u32>,
        capability: GuiColorCapability,
        definition: GuiObjectRef,
        position: Point3Meters,
    },
    DuplicatePlacement {
        id: u32,
    },
    RemovePlacement {
        id: u32,
    },
    EditElements {
        edit: ElementTreeGuiEdit,
    },
    AddPixelLight {
        light: SetupPixelLight,
    },
    UpdatePlacementTransform {
        id: u32,
        transform: Transform,
    },
    SetPlacementBindings {
        id: u32,
        bindings: Vec<SetupElementCell>,
    },
    CopyPlacementDefinition {
        id: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ElementTarget {
    pub kind: ElementTargetKind,
    pub name: String,
}
