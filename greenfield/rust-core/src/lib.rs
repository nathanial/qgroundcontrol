use napi::bindgen_prelude::*;
use serde::Serialize;

/// Message returned to the Electron shell.
#[napi(object)]
#[derive(Debug, Serialize, Clone)]
pub struct StatusMessage {
    pub kind: String,
    pub message: String,
}

impl StatusMessage {
    fn new(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
        }
    }
}

#[napi]
pub fn health_check() -> Result<StatusMessage> {
    Ok(StatusMessage::new("health", "rust-core napi module loaded"))
}

#[napi]
pub fn version() -> Result<String> {
    Ok(env!("CARGO_PKG_VERSION").to_string())
}
