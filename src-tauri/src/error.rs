//! The error shape that crosses the IPC boundary.
//!
//! The frontend switches on `code` -- a stable machine-readable discriminant --
//! never on message text, so wording can improve without breaking behaviour.
//! `retryable` drives whether the UI offers a "Try again" affordance.

use deck_domain::DeckError;
use serde::Serialize;

/// A `DeckError` flattened for the wire.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IpcError {
    /// Stable discriminant, e.g. `port_in_use`.
    pub code: String,
    /// Human-readable message, already actionable.
    pub message: String,
    /// Whether retrying the same operation could plausibly succeed.
    pub retryable: bool,
}

impl From<DeckError> for IpcError {
    fn from(e: DeckError) -> Self {
        Self {
            code: e.code().to_owned(),
            message: e.to_string(),
            retryable: e.is_retryable(),
        }
    }
}

/// Shorthand for command results.
pub type IpcResult<T> = Result<T, IpcError>;
