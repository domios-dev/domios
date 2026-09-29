use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

/// JSON error surface for `/api/agents/spawn`, so the mesh relay can parse it
/// instead of collapsing a bare `(StatusCode, String)` body to "HTTP 500".
pub(crate) struct SpawnRejection {
    pub(crate) status: StatusCode,
    pub(crate) body: serde_json::Value,
}

impl IntoResponse for SpawnRejection {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

impl SpawnRejection {
    pub(crate) fn new(status: StatusCode, code: &str, message: String) -> Self {
        Self {
            status,
            body: serde_json::json!({"ok": false, "error": {"code": code, "message": message}}),
        }
    }
}

/// The refusal reason was flattened to text upstream, so the daemon cannot promise a retry helps.
pub(crate) const SPAWN_REFUSAL_CLASS_UNCLASSIFIED: &str = "unknown";

impl From<String> for SpawnRejection {
    fn from(message: String) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: serde_json::json!({
                "ok": false,
                "error": {
                    "code": "spawn_failed",
                    "message": message,
                    "class": SPAWN_REFUSAL_CLASS_UNCLASSIFIED,
                    "retry_hint": "the spawn was refused and the daemon could not tell whether a retry can help — fix the spawn request (--role/--plan-task/--cwd) first, then check the target with `codeterm mesh peers`",
                },
            }),
        }
    }
}
