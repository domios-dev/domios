//! Error types for codeterm-relay.

use thiserror::Error;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

#[derive(Error, Debug)]
pub enum RelayError {
    #[error("Configuration error: {0}")]
    Config(#[from] anyhow::Error),

    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Peer not found: {0}")]
    PeerNotFound(String),

    #[error("Pane not found: {0}")]
    PaneNotFound(String),

    #[error("Route not found: {0}")]
    RouteNotFound(String),

    #[error("Template error: {0}")]
    TemplateError(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Webhook verification failed: {0}")]
    WebhookVerificationFailed(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for RelayError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            RelayError::AuthFailed(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            RelayError::InvalidRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            RelayError::PeerNotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            RelayError::PaneNotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            RelayError::RouteNotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            RelayError::WebhookVerificationFailed(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            RelayError::Config(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            RelayError::Network(e) => (StatusCode::BAD_GATEWAY, e.clone()),
            RelayError::TemplateError(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.clone()),
            RelayError::Internal(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.clone()),
        };

        let body = serde_json::json!({
            "error": message,
            "status": status.as_u16()
        });

        (status, Json(body)).into_response()
    }
}

impl From<reqwest::Error> for RelayError {
    fn from(e: reqwest::Error) -> Self {
        RelayError::Network(e.to_string())
    }
}

impl From<serde_json::Error> for RelayError {
    fn from(e: serde_json::Error) -> Self {
        RelayError::InvalidRequest(e.to_string())
    }
}