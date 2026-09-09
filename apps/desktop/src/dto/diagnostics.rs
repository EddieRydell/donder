use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDiagnostic {
    pub path: String,
    pub range: Option<TextRange>,
    pub severity: DiagnosticSeverity,
    pub code: String,
    pub message: String,
    pub detail: Option<String>,
    pub related: Vec<RelatedDiagnosticLocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RelatedDiagnosticLocation {
    pub path: String,
    pub range: Option<TextRange>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedPreviewProp {
    pub name: String,
    pub color_model: String,
    pub bulb_diameter_meters: f32,
    pub geometry_summary: String,
    pub render_plan: GeometryRenderPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Rotation3Degrees {
    pub x_degrees: f32,
    pub y_degrees: f32,
    pub z_degrees: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Scale3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}
