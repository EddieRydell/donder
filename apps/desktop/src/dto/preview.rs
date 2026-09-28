use super::*;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewAppearance {
    pub background_rgb: [u8; 3],
    pub unlit_rgb: [u8; 3],
    pub window_width: u32,
    pub window_height: u32,
    pub window_min_width: u32,
    pub window_min_height: u32,
    pub canvas_fill_ratio: f32,
    pub minimum_radius_pixels: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FixtureGuiDocument {
    pub path: String,
    pub source_ref: GuiObjectRef,
    pub object_key: String,
    pub elements: Vec<GuiFixtureElement>,
    pub handles: Vec<GuiFixtureHandle>,
    pub render_plan: SpatialRenderPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiFixtureElement {
    pub id: u32,
    pub name: String,
    pub transform: Transform,
    pub diameter_meters: f32,
    pub reverse: bool,
    pub shape: GuiFixtureShape,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiFixtureHandle {
    pub element: u32,
    pub index: u32,
    pub position: Point3Meters,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum GuiGridAxis {
    Rows,
    Columns,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum GuiGridCorner {
    BottomLeft,
    BottomRight,
    TopLeft,
    TopRight,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiFixtureShape {
    Pixel,
    Line {
        length: f32,
        count: u32,
    },
    Polyline {
        points: Vec<Point3Meters>,
        count: u32,
    },
    Arc {
        radius: f32,
        start_degrees: f32,
        sweep_degrees: f32,
        count: u32,
        closed: bool,
    },
    Grid {
        columns: u32,
        rows: u32,
        width: f32,
        height: f32,
        axis: GuiGridAxis,
        corner: GuiGridCorner,
        serpentine: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FixtureGuiEdit {
    SetElements {
        elements: Vec<GuiFixtureElement>,
    },
    MoveElement {
        id: u32,
        delta: Point3Meters,
    },
    MoveHandle {
        id: u32,
        index: u32,
        position: Point3Meters,
    },
    ConvertToPixels {
        id: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LayoutGuiDocument {
    pub path: String,
    pub source_ref: GuiObjectRef,
    pub object_key: String,
    pub fixtures: Vec<GuiLayoutFixture>,
    pub available_fixtures: Vec<GuiObjectRef>,
    pub render_plan: SpatialRenderPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiLayoutFixture {
    pub id: u32,
    pub name: String,
    pub kind: GuiLayoutFixtureKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiLayoutFixtureKind {
    Fixture {
        definition: GuiObjectRef,
        transform: Transform,
    },
    Group {
        children: Vec<GuiLayoutFixture>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LayoutGuiEdit {
    AddDefinition {
        name: String,
        storage: FixtureStorage,
        parent: Option<u32>,
        transform: Transform,
    },
    SetFixtures {
        fixtures: Vec<GuiLayoutFixture>,
    },
    MoveFixture {
        id: u32,
        delta: Point3Meters,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum FixtureStorage {
    Inline,
    NewFile,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SpatialRenderPlan {
    pub pixels: Vec<SpatialRenderPixel>,
    pub bounds: GeometryRenderBounds,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SpatialRenderPixel {
    /// Pixel ID in a fixture editor; instance ID in a layout editor.
    pub owner: u32,
    pub index: u32,
    pub position: Point3Meters,
    pub diameter_meters: f32,
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
