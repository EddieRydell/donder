use serde::{Deserialize, Serialize};
use specta::Type;

/// Perform mode: the consoles the project can perform and the performance in progress.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PerformSnapshot {
    /// Increases with every change, so a client keeps the latest of several.
    pub revision: u32,
    pub consoles: Vec<PerformConsole>,
    pub session: Option<PerformSession>,
}

/// A console of the project, by its position in the project's consoles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PerformConsole {
    pub index: u32,
    pub name: String,
    /// The document that declares it.
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PerformSession {
    pub console: PerformConsole,
    pub elapsed_seconds: f64,
    /// The buttons held down.
    pub held: Vec<PerformButton>,
    /// The `KeyboardEvent.code` of every key the console plays from.
    pub keys: Vec<String>,
    /// The console's MIDI connections, when it plays from MIDI.
    pub midi: Option<PerformMidi>,
    /// The console's OSC listeners, when it plays from OSC.
    pub osc: Option<PerformOsc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PerformMidi {
    /// The ports connected now.
    pub ports: Vec<String>,
    /// Why the last look for ports failed to list or connect one.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PerformOsc {
    /// The UDP ports listening now.
    pub ports: Vec<u16>,
    /// Why a port that is not listening failed to bind.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PerformButton {
    pub device: String,
    pub control: String,
}
