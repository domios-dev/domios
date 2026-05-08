//! Error types for codeterm-mesh-client.

use thiserror::Error;

/// Errors that can occur in mesh client operations.
#[derive(Error, Debug)]
pub enum MeshError {
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Invalid key format: {0}")]
    InvalidKey(String),

    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    #[error("Peer not found: {0}")]
    PeerNotFound(String),

    #[error("Request expired")]
    RequestExpired,

    #[error("Signature verification failed")]
    InvalidSignature,

    #[error("No addresses available for peer")]
    NoAddresses,

    #[error("HTTP error: {0}")]
    Http(String),
}

impl From<MeshError> for String {
    fn from(e: MeshError) -> String {
        e.to_string()
    }
}