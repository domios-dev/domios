use axum::{
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use std::net::SocketAddr;

use super::routes::AppState;

#[derive(Deserialize)]
pub(crate) struct RpcRequest {
    method: String,
    params: Option<serde_json::Value>,
    protocol_version: Option<serde_json::Value>,
}

fn local_caller(
    remote: SocketAddr,
    expected_token: Option<&str>,
    authorization: Option<&str>,
) -> bool {
    let Some(expected_token) = expected_token.filter(|value| !value.is_empty()) else {
        return false;
    };
    let presented = authorization
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("");
    remote.ip().is_loopback()
        && crate::service::tokens::constant_time_eq(presented.as_bytes(), expected_token.as_bytes())
}

fn identity_matches(method: &str, params: Option<&serde_json::Value>, headers: &HeaderMap) -> bool {
    let params = params.and_then(serde_json::Value::as_object);
    let field_matches = |field: &str| {
        let expected = headers
            .get("x-codeterm-tab-id")
            .and_then(|value| value.to_str().ok());
        expected.is_none_or(|expected| {
            params
                .and_then(|params| params.get(field))
                .and_then(serde_json::Value::as_str)
                == Some(expected)
        })
    };
    match method {
        "agent.startup_ack" => {
            field_matches("caller_tab_id")
                && optional_header_matches_param(
                    headers,
                    "x-codeterm-binding-nonce",
                    params,
                    "binding_nonce",
                )
        }
        "agent.ack" => field_matches("tab_id"),
        "agent.report" => {
            field_matches("tab_id")
                && field_matches("caller_tab_id")
                && optional_header_matches_param(
                    headers,
                    "x-codeterm-session-id",
                    params,
                    "caller_session_id",
                )
                && optional_header_matches_param(
                    headers,
                    "x-codeterm-binding-nonce",
                    params,
                    "session_binding_nonce",
                )
        }
        "tab.send_text" => {
            let Some(tab_id) = headers
                .get("x-codeterm-tab-id")
                .and_then(|value| value.to_str().ok())
            else {
                return true;
            };
            let source = params
                .and_then(|params| params.get("source_tab"))
                .and_then(serde_json::Value::as_str);
            source.map_or_else(
                || {
                    params
                        .and_then(|params| params.get("tab_id"))
                        .and_then(serde_json::Value::as_str)
                        == Some(tab_id)
                },
                |source| source == tab_id,
            )
        }
        _ => true,
    }
}

fn optional_header_matches_param(
    headers: &HeaderMap,
    header: &str,
    params: Option<&serde_json::Map<String, serde_json::Value>>,
    field: &str,
) -> bool {
    let expected = headers.get(header).and_then(|value| value.to_str().ok());
    let actual = params
        .and_then(|params| params.get(field))
        .and_then(serde_json::Value::as_str);
    expected == actual
}

pub(crate) async fn dispatch(
    State(state): State<AppState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<RpcRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let expected_token = state.config.lock().api.token.clone();
    if !local_caller(
        remote,
        expected_token.as_deref(),
        headers
            .get("authorization")
            .and_then(|value| value.to_str().ok()),
    ) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(
                serde_json::json!({"error":{"code":-32003,"message":"HTTP RPC fallback is restricted to authenticated loopback callers","data":null}}),
            ),
        ));
    }
    if let Some(error) = crate::socket::server::protocol::version_skew_gate(
        crate::socket::server::protocol::client_protocol_version(request.protocol_version.as_ref()),
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
    if matches!(
        request.method.as_str(),
        "agent.startup_ack" | "agent.ack" | "agent.report" | "tab.send_text"
    ) && !identity_matches(&request.method, request.params.as_ref(), &headers)
    {
        return Err((
            StatusCode::FORBIDDEN,
            Json(
                serde_json::json!({"error":{"code":-32003,"message":"HTTP RPC identity headers do not match request parameters","data":null}}),
            ),
        ));
    }
    let svc = state.svc.clone();
    let method = request.method;
    let params = request.params;
    let result = tokio::task::spawn_blocking(move || {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::socket::server::dispatch_service_rpc(&method, params, &svc)
        }))
    })
    .await;
    match result {
        Ok(Ok(Ok(value))) => Ok(Json(serde_json::json!({"result": value}))),
        Ok(Ok(Err(error))) => Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": {
                    "code": error.code,
                    "message": error.message,
                    "data": error.data,
                }
            })),
        )),
        Ok(Err(_)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                serde_json::json!({"error":{"code":-32603,"message":"HTTP RPC dispatch panicked","data":null}}),
            ),
        )),
        Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                serde_json::json!({"error":{"code":-32603,"message":"HTTP RPC dispatch task failed","data":null}}),
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{identity_matches, local_caller};
    use axum::http::HeaderMap;
    use std::net::SocketAddr;

    #[test]
    fn mesh_peer_identity_cannot_use_loopback_rpc_fallback() {
        let peer = "192.0.2.10:7000".parse::<SocketAddr>().unwrap();
        assert!(!local_caller(peer, Some("secret"), Some("Bearer secret")));
        assert!(!local_caller(
            "127.0.0.1:7000".parse().unwrap(),
            Some("secret"),
            Some("Bearer peer-token")
        ));
        assert!(local_caller(
            "127.0.0.1:7000".parse().unwrap(),
            Some("secret"),
            Some("Bearer secret")
        ));
    }

    #[test]
    fn rest_identity_headers_are_accepted_for_local_auth_and_refused_for_mesh_callers() {
        let local = "127.0.0.1:7000".parse().unwrap();
        let peer = "192.0.2.10:7000".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-codeterm-tab-id", "worker-1".parse().unwrap());
        let params = serde_json::json!({"tab_id": "worker-1"});
        let local_authenticated = local_caller(local, Some("secret"), Some("Bearer secret"));
        assert!(local_authenticated);
        assert!(local_authenticated && identity_matches("agent.ack", Some(&params), &headers));
        assert!(!local_caller(peer, Some("secret"), Some("Bearer secret")));
        assert!(!local_caller(
            local,
            Some("secret"),
            Some("Bearer peer-token")
        ));
    }

    #[test]
    fn agent_identity_headers_must_match_rpc_parameters() {
        let mut headers = HeaderMap::new();
        headers.insert("x-codeterm-tab-id", "worker-1".parse().unwrap());
        headers.insert("x-codeterm-binding-nonce", "nonce-1".parse().unwrap());
        let params = serde_json::json!({
            "caller_tab_id": "worker-1",
            "attempt_id": "attempt-1",
            "binding_nonce": "nonce-1"
        });
        assert!(identity_matches(
            "agent.startup_ack",
            Some(&params),
            &headers
        ));
        let forged = serde_json::json!({
            "caller_tab_id": "other-tab",
            "attempt_id": "attempt-1",
            "binding_nonce": "nonce-1"
        });
        assert!(!identity_matches(
            "agent.startup_ack",
            Some(&forged),
            &headers
        ));
    }
}
