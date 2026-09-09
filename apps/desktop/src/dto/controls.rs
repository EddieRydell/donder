use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceControlTarget {
    Scalar {
        node: u32,
        cells: Option<PatchGuiCellRange>,
    },
    Indexed {
        node: u32,
        cells: Option<PatchGuiCellRange>,
    },
    FixtureFunction {
        node: u32,
        cells: Option<PatchGuiCellRange>,
        function: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceControlValue {
    ConstantNormalized {
        value: f32,
    },
    NormalizedCurve {
        points: Vec<SequenceCurvePoint>,
    },
    Indexed {
        option: u32,
        range_curve: Option<Vec<SequenceCurvePoint>>,
    },
    FixtureIndexed {
        entry: u32,
        range_curve: Option<Vec<SequenceCurvePoint>>,
    },
    ConstantColor {
        value: String,
    },
    Gradient {
        stops: Vec<SequenceGradientStop>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceControlChannel {
    pub target: SequenceControlTarget,
    pub label: String,
    pub cell_count: Option<u32>,
    pub options: SequenceControlOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceControlOptions {
    Normalized,
    Indexed { options: Vec<SetupIndexedOption> },
    FixtureIndexed { entries: Vec<SequenceControlEntry> },
    Color,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceControlEntry {
    pub id: u32,
    pub name: String,
    pub range_control: bool,
}
