use serde::{Deserialize, Serialize};
use specta::Type;

/// Perform mode: the rigs the project can perform and the performance in progress.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PerformSnapshot {
    /// Increases with every change, so a client keeps the latest of several.
    pub revision: u32,
    pub rigs: Vec<PerformRig>,
    pub session: Option<PerformSession>,
}

/// A rig of the project, by its position in the project's rigs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PerformRig {
    pub index: u32,
    pub name: String,
    /// The document that declares it.
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PerformSession {
    pub rig: PerformRig,
    pub elapsed_seconds: f64,
    pub switches: Vec<PerformSwitch>,
    /// The buttons held down.
    pub held: Vec<PerformButton>,
    /// The `KeyboardEvent.code` of every key the rig plays from.
    pub keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PerformSwitch {
    pub name: String,
    pub on: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PerformButton {
    pub device: String,
    pub control: String,
}
