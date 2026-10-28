use serde::{Deserialize, Serialize};
use specta::Type;

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
