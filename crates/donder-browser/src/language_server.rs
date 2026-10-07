//! The language server for the website's editor, run in a worker: the host
//! passes each JSON-RPC message as text and sends back what it returns.
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct LanguageServer(donder_language_server::Server);

#[wasm_bindgen]
impl LanguageServer {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self(donder_language_server::Server::new())
    }

    /// Handle one message; returns the messages to send, as a JSON array.
    pub fn handle(&mut self, message: &str) -> Result<String, JsValue> {
        let message = serde_json::from_str(message)
            .map_err(|error| JsValue::from_str(&format!("Invalid message: {error}")))?;
        Ok(serde_json::Value::Array(self.0.handle(message)).to_string())
    }

    /// Recheck after edits; call when messages pause. Returns a JSON array.
    pub fn idle(&mut self) -> String {
        serde_json::Value::Array(self.0.idle()).to_string()
    }

    /// Milliseconds to wait after the last message before calling `idle`.
    #[wasm_bindgen(js_name = idleDelayMs)]
    pub fn idle_delay_ms() -> u32 {
        donder_language_server::IDLE_DELAY_MS as u32
    }
}

impl Default for LanguageServer {
    fn default() -> Self {
        Self::new()
    }
}
