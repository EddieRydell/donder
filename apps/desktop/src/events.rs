//! What the backend tells the window, unasked. Each event's name and payload
//! are generated into the frontend bindings.
use donder_editor::dto::{
    AppSnapshot, AudioTransportSnapshot, LiveOutputSnapshot, PerformSnapshot,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::{Event, Events, collect_events};

#[derive(Clone, Debug, Serialize, Deserialize, Type, Event)]
pub(crate) struct AppSnapshotChanged {
    pub snapshot: AppSnapshot,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, Event)]
pub(crate) struct AudioTransportChanged {
    pub transport: AudioTransportSnapshot,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, Event)]
pub(crate) struct LiveOutputChanged {
    pub output: LiveOutputSnapshot,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, Event)]
pub(crate) struct PerformChanged {
    pub perform: PerformSnapshot,
}

/// Whether the Preview window is open.
#[derive(Clone, Debug, Serialize, Deserialize, Type, Event)]
pub(crate) struct PreviewWindowChanged {
    pub open: bool,
}

/// The window, the Preview or the OS asked to quit; the window confirms
/// unsaved changes first.
#[derive(Clone, Debug, Serialize, Deserialize, Type, Event)]
pub(crate) struct CloseRequested;

/// A JSON-RPC reply or notification from the language server.
#[derive(Clone, Debug, Serialize, Deserialize, Type, Event)]
pub(crate) struct LanguageServerMessage {
    pub message: String,
}

pub(crate) fn events() -> Events {
    collect_events![
        AppSnapshotChanged,
        AudioTransportChanged,
        LiveOutputChanged,
        PerformChanged,
        PreviewWindowChanged,
        CloseRequested,
        LanguageServerMessage,
    ]
}

/// Send `event` to the window. A window that is gone has nothing to update.
pub(crate) fn send<R: tauri::Runtime>(
    app: &(impl tauri::Emitter<R> + tauri::Manager<R>),
    event: impl Event + Serialize + Clone,
) {
    let _ = event.emit(app);
}
