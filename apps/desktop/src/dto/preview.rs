use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FixtureGuiDocument {
    pub path: String,
    pub source_ref: GuiObjectRef,
    pub object_key: String,
    pub pixels: Vec<GuiPixel>,
    pub render_plan: SpatialRenderPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiPixel {
    pub id: u32,
    pub position: Point3Meters,
    pub diameter_meters: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FixtureGuiEdit {
    SetPixels { pixels: Vec<GuiPixel> },
    MovePixel { id: u32, delta: Point3Meters },
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
    AddDefinition { name: String, parent: Option<u32> },
    SetFixtures { fixtures: Vec<GuiLayoutFixture> },
    MoveFixture { id: u32, delta: Point3Meters },
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
