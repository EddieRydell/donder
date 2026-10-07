//! The language server, on its own thread. The editor sends each JSON-RPC
//! message with `language_server_send`, and receives every reply and
//! notification as a `language_server_message` event.
use std::sync::{Mutex, mpsc};
use std::time::Duration;

use tauri::{AppHandle, Emitter};

pub(crate) const MESSAGE_EVENT: &str = "language_server_message";

pub(crate) struct LanguageServerHost {
    sender: Mutex<mpsc::Sender<serde_json::Value>>,
}

/// The desktop's working copies: the server sees unsaved buffers, not only
/// the open editor.
struct WorkingCopies(crate::desktop_state::DesktopState);

impl donder_language_server::DocumentSource for WorkingCopies {
    fn text(&self, relative: &camino::Utf8Path) -> Option<String> {
        self.0.working_text(relative)
    }

    fn overrides(&self) -> Vec<(camino::Utf8PathBuf, String)> {
        self.0.working_texts()
    }
}

impl LanguageServerHost {
    pub(crate) fn start(
        app: AppHandle,
        state: crate::desktop_state::DesktopState,
    ) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::channel::<serde_json::Value>();
        std::thread::Builder::new()
            .name("language-server".into())
            .spawn(move || {
                let mut server = donder_language_server::Server::with_source(std::sync::Arc::new(
                    WorkingCopies(state),
                ));
                let pause = Duration::from_millis(donder_language_server::IDLE_DELAY_MS);
                let emit = |messages: Vec<serde_json::Value>| {
                    for message in messages {
                        let _ = app.emit(MESSAGE_EVENT, message.to_string());
                    }
                };
                loop {
                    match receiver.recv_timeout(pause) {
                        Ok(message) => emit(server.handle(message)),
                        Err(mpsc::RecvTimeoutError::Timeout) => emit(server.idle()),
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            })?;
        Ok(Self {
            sender: Mutex::new(sender),
        })
    }

    pub(crate) fn send(&self, message: &str) -> Result<(), String> {
        let message = serde_json::from_str(message)
            .map_err(|error| format!("Invalid language server message: {error}"))?;
        self.sender
            .lock()
            .map_err(|_| "The language server is unavailable.".to_string())?
            .send(message)
            .map_err(|_| "The language server stopped.".to_string())
    }
}
