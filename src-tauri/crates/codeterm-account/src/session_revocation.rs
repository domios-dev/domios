use crate::Session;
use async_trait::async_trait;
use serde_json::Value;

pub const SESSION_REVOKED: &str = "session_revoked";

pub fn is_session_revoked(body: &Value) -> bool {
    body.pointer("/error/status").and_then(Value::as_str) == Some("UNAUTHENTICATED")
        && body.pointer("/error/details/reason").and_then(Value::as_str)
            == Some(SESSION_REVOKED)
}

#[async_trait]
pub trait SessionRevocationObserver: Send + Sync {
    async fn revoked(&self, session: &Session);
}
