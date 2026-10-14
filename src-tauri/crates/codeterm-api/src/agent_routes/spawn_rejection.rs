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

impl From<codeterm_service_core::service::spawn_launch_verdict::SpawnLaunchFailure>
    for SpawnRejection
{
    fn from(
        failure: codeterm_service_core::service::spawn_launch_verdict::SpawnLaunchFailure,
    ) -> Self {
        let mut error = failure.data();
        error["code"] = serde_json::json!(
            codeterm_service_core::service::spawn_launch_verdict::SpawnLaunchFailure::CODE
        );
        error["message"] = serde_json::json!(failure.message());
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: serde_json::json!({ "ok": false, "error": error }),
        }
    }
}

/// Answer for the launch this spawn started, off the async runtime.
pub(crate) async fn await_spawn_launch(
    svc: &codeterm_service_core::service::ServiceState,
    tab_id: &str,
) -> Result<codeterm_service_core::service::spawn_launch_verdict::SpawnLaunchVerdict, SpawnRejection>
{
    let svc = svc.clone();
    let tab_id = tab_id.to_string();
    tokio::task::spawn_blocking(move || {
        svc.await_spawn_launch_verdict(
            &tab_id,
            codeterm_service_core::service::spawn_launch_verdict::SPAWN_LAUNCH_VERDICT_DEADLINE,
        )
    })
    .await
    .map_err(|error| SpawnRejection::from(format!("spawn launch verdict task failed: {error}")))?
    .map_err(SpawnRejection::from)
}

impl From<codeterm_service_core::service::CwdRefusal> for SpawnRejection {
    fn from(error: codeterm_service_core::service::CwdRefusal) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            body: serde_json::json!({"ok": false, "error": error.data()}),
        }
    }
}

pub(crate) async fn validate_spawn_cwd(
    svc: &codeterm_service_core::service::ServiceState,
    workspace_id: Option<&str>,
    caller_tab_id: Option<&str>,
    cwd: Option<&str>,
) -> Result<(), SpawnRejection> {
    let svc = svc.clone();
    let workspace_id = workspace_id.map(str::to_string);
    let caller_tab_id = caller_tab_id.map(str::to_string);
    let cwd = cwd.map(str::to_string);
    tokio::task::spawn_blocking(move || {
        svc.validate_request_cwd(
            workspace_id.as_deref(),
            caller_tab_id.as_deref(),
            cwd.as_deref(),
        )
    })
    .await
    .map_err(|error| SpawnRejection::from(error.to_string()))?
    .map_err(SpawnRejection::from)
}
