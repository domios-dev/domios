use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;

use super::routes::AppState;

#[derive(Deserialize)]
pub(crate) struct RpcRequest {
    method: String,
    params: Option<serde_json::Value>,
    protocol_version: Option<serde_json::Value>,
}

fn supported_method(method: &str) -> bool {
    matches!(
        method,
        "system.identify"
            | "tab.list"
            | "tab.send_text"
            | "agent.startup_ack"
            | "agent.ack"
            | "agent.report"
    )
}

pub(crate) async fn dispatch(
    State(state): State<AppState>,
    Json(request): Json<RpcRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !supported_method(&request.method) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": {
                    "code": -32601,
                    "message": "Method not found on HTTP fallback",
                    "data": null,
                }
            })),
        ));
    }
    if let Some(error) = crate::socket::server::protocol::version_skew_gate(
        crate::socket::server::protocol::client_protocol_version(
            request.protocol_version.as_ref(),
        ),
        crate::socket::server::protocol::PROTOCOL_VERSION,
        crate::socket::server::protocol::RECOVERY_COMMAND,
    ) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": { "code": error.code, "message": error.message, "data": error.data }
            })),
        ));
    }
    crate::socket::server::dispatch_service_rpc(&request.method, request.params, &state.svc)
        .map(Json)
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": {
                        "code": error.code,
                        "message": error.message,
                        "data": error.data,
                    }
                })),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::supported_method;

    #[test]
    fn http_rpc_fallback_only_exposes_the_required_cli_methods() {
        for method in [
            "system.identify",
            "tab.list",
            "tab.send_text",
            "agent.startup_ack",
            "agent.ack",
            "agent.report",
        ] {
            assert!(supported_method(method), "{method}");
        }
        assert!(!supported_method("agent.spawn"));
        assert!(!supported_method("plan.delete"));
    }
}
