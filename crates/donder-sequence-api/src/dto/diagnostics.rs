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
    /// The root import that brings an unreferenced document into the project.
    pub inclusion: Option<DocumentInclusion>,
}

/// Importing an unreferenced document from the project root.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocumentInclusion {
    pub document: String,
    pub alias: String,
    /// Sequences the document declares, added to the project's sequences.
    pub sequences: Vec<String>,
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
