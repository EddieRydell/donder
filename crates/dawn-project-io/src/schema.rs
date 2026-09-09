//! YAML leaf shapes shared by loading and saving. Domain validation stays in
//! dawn-language; document identity and reference resolution stay in the loader.
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CurvePoint {
    pub position: f32,
    pub value: f32,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GradientStop {
    pub position: f32,
    pub color: String,
}
