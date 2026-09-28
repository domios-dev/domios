use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

use axum::{extract::State as AxumState, http::HeaderMap, Json};
use serde::{Deserialize, Serialize};

use crate::api::routes::AppState;
use crate::socket::server::protocol;

const CACHE_CAPACITY: usize = 512;

#[derive(Deserialize)]
pub struct MeshRpcRequest {
    method: String,
    params: Option<serde_json::Value>,
    request_id: String,
    /// Lenient by design: any non-integer value gates as the oldest protocol.
    #[serde(default)]
    protocol_version: Option<serde_json::Value>,
}

#[derive(Clone, Serialize)]
pub struct MeshRpcResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<crate::socket::server::JsonRpcError>,
}

#[derive(Default)]
struct ResponseCache {
    order: VecDeque<String>,
    values: HashMap<String, Arc<ResponseEntry>>,
}

impl ResponseCache {
    fn acquire(&mut self, request_id: String) -> (Arc<ResponseEntry>, bool) {
        if let Some(entry) = self.values.get(&request_id) {
            return (entry.clone(), false);
        }
        let entry = Arc::new(ResponseEntry::default());
        self.order.push_back(request_id.clone());
        self.values.insert(request_id, entry.clone());
        while self.order.len() > CACHE_CAPACITY {
            if let Some(expired) = self.order.pop_front() {
                if self
                    .values
                    .get(&expired)
                    .is_some_and(|entry| entry.is_complete())
                {
                    self.values.remove(&expired);
                } else {
                    self.order.push_back(expired);
                    break;
                }
            }
        }
        (entry, true)
    }
}

#[derive(Default)]
struct ResponseEntry {
    response: Mutex<Option<MeshRpcResponse>>,
    ready: tokio::sync::Notify,
}

impl ResponseEntry {
    fn is_complete(&self) -> bool {
        self.response
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some()
    }

    fn complete(&self, response: MeshRpcResponse) {
        *self
            .response
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(response);
        self.ready.notify_waiters();
    }

    async fn wait(&self) -> MeshRpcResponse {
        loop {
            let notified = self.ready.notified();
            if let Some(response) = self
                .response
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
            {
                return response;
            }
            notified.await;
        }
    }
}

fn response_cache() -> &'static Mutex<ResponseCache> {
    static CACHE: OnceLock<Mutex<ResponseCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(ResponseCache::default()))
}

fn method_allowed(method: &str) -> bool {
    matches!(
        method.split('.').next().unwrap_or_default(),
        "plan" | "spec" | "effort" | "task" | "team"
    )
}

pub async fn mesh_rpc(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(request): Json<MeshRpcRequest>,
) -> Json<MeshRpcResponse> {
    if request.request_id.is_empty() || request.request_id.len() > 128 {
        return Json(MeshRpcResponse {
            result: None,
            error: Some(crate::socket::server::JsonRpcError {
                code: -32602,
                message: "request_id must contain 1 to 128 characters".to_string(),
                data: None,
            }),
        });
    }
    if let Some(error) = protocol::version_skew_gate(
        protocol::client_protocol_version(request.protocol_version.as_ref()),
        protocol::PROTOCOL_VERSION,
        protocol::RECOVERY_COMMAND,
    ) {
        return Json(MeshRpcResponse {
            result: None,
            error: Some(error),
        });
    }
    let (entry, owns_dispatch) = response_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .acquire(request.request_id);
    if !owns_dispatch {
        return Json(entry.wait().await);
    }

    let authenticated_peer = if matches!(
        request.method.as_str(),
        "plan.task.remote_ack" | "plan.task.remote_report"
    ) {
        match super::relay::check_mesh_relay_auth(&state, &headers) {
            Ok(peer_id) if peer_has_full_access(&state, &peer_id) => Some(peer_id),
            Ok(_) => {
                entry.complete(MeshRpcResponse {
                    result: None,
                    error: Some(crate::socket::server::JsonRpcError {
                        code: -32003,
                        message: "remote task lifecycle mutation requires Full mesh access".into(),
                        data: None,
                    }),
                });
                return Json(entry.wait().await);
            }
            Err((_, message)) => {
                entry.complete(MeshRpcResponse {
                    result: None,
                    error: Some(crate::socket::server::JsonRpcError {
                        code: -32003,
                        message,
                        data: None,
                    }),
                });
                return Json(entry.wait().await);
            }
        }
    } else {
        None
    };
    let response = if method_allowed(&request.method) {
        let method = request.method.clone();
        let mut params = request.params;
        sanitize_remote_context(&mut params);
        let mesh_peer = authenticated_peer;
        let svc = state.svc.clone();
        match tokio::task::spawn_blocking(move || {
            if let Some(peer_id) = mesh_peer {
                crate::socket::server::dispatch_mesh_rpc(&method, params, &svc, &peer_id)
            } else {
                crate::socket::server::dispatch_service_rpc(&method, params, &svc)
            }
        })
        .await
        {
            Ok(Ok(result)) => MeshRpcResponse {
                result: Some(result),
                error: None,
            },
            Ok(Err(error)) => MeshRpcResponse {
                result: None,
                error: Some(error),
            },
            Err(error) => MeshRpcResponse {
                result: None,
                error: Some(crate::socket::server::JsonRpcError {
                    code: -32603,
                    message: format!("mesh RPC dispatch failed: {error}"),
                    data: None,
                }),
            },
        }
    } else {
        MeshRpcResponse {
            result: None,
            error: Some(crate::socket::server::JsonRpcError {
                code: -32601,
                message: format!("method '{}' is not available over mesh RPC", request.method),
                data: None,
            }),
        }
    };

    entry.complete(response.clone());
    Json(response)
}

fn peer_has_full_access(state: &AppState, peer_id: &str) -> bool {
    state
        .mesh
        .lock()
        .paired_peers
        .get(peer_id)
        .is_some_and(|peer| peer.access_level == codeterm_mesh::types::AccessLevel::Full)
}

fn sanitize_remote_context(params: &mut Option<serde_json::Value>) {
    let Some(object) = params.as_mut().and_then(serde_json::Value::as_object_mut) else {
        return;
    };
    object.remove("caller_tab_id");
    for key in ["owner", "from"] {
        if let Some(identity) = object
            .get_mut(key)
            .and_then(serde_json::Value::as_object_mut)
        {
            identity.remove("tab_id");
            identity.remove("session_id");
            identity.remove("title");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{method_allowed, sanitize_remote_context, ResponseCache, CACHE_CAPACITY};

    #[test]
    fn allowlist_is_plan_domain_only() {
        for method in [
            "plan.get",
            "spec.show",
            "effort.link",
            "task.show",
            "team.list",
        ] {
            assert!(method_allowed(method), "{method}");
        }
        for method in [
            "tab.send_text",
            "system.identify",
            "plugin.exec",
            "cron.list",
        ] {
            assert!(!method_allowed(method), "{method}");
        }
    }

    #[test]
    fn version_skew_error_serializes_untranslated_over_mesh_rpc() {
        use crate::socket::server::protocol;
        let error = protocol::version_skew_gate(2, 6, "recover --here")
            .expect("older mesh client is refused");
        let response = super::MeshRpcResponse {
            result: None,
            error: Some(error),
        };

        let wire = serde_json::to_value(&response).expect("mesh RPC response serializes");
        assert_eq!(wire["error"]["code"], protocol::VERSION_SKEW);
        assert_eq!(wire["error"]["data"]["type"], "version_skew");
        assert_eq!(wire["error"]["data"]["client_protocol"], 2);
        assert_eq!(wire["error"]["data"]["server_protocol"], 6);
        assert_eq!(wire["error"]["data"]["recovery"], "recover --here");
    }

    #[test]
    fn version_skew_mesh_request_parses_with_and_without_the_version_field() {
        use crate::socket::server::protocol;
        let versioned: super::MeshRpcRequest = serde_json::from_str(&format!(
            r#"{{"method":"plan.get","params":null,"request_id":"r1","protocol_version":{}}}"#,
            protocol::PROTOCOL_VERSION
        ))
        .unwrap();
        assert!(protocol::version_skew_gate(
            protocol::client_protocol_version(versioned.protocol_version.as_ref()),
            protocol::PROTOCOL_VERSION,
            protocol::RECOVERY_COMMAND,
        )
        .is_none());

        let legacy: super::MeshRpcRequest =
            serde_json::from_str(r#"{"method":"plan.get","params":null,"request_id":"r2"}"#)
                .expect("a peer without the version field must not fail parsing");
        assert_eq!(
            protocol::client_protocol_version(legacy.protocol_version.as_ref()),
            0
        );
    }

    #[test]
    fn response_cache_is_bounded() {
        let mut cache = ResponseCache::default();
        for index in 0..=CACHE_CAPACITY {
            let (entry, owns_dispatch) = cache.acquire(index.to_string());
            assert!(owns_dispatch);
            entry.complete(super::MeshRpcResponse {
                result: Some(serde_json::json!(index)),
                error: None,
            });
        }
        assert!(!cache.values.contains_key("0"));
        assert!(cache.values.contains_key(&CACHE_CAPACITY.to_string()));
    }

    #[test]
    fn remote_context_cannot_claim_a_local_tab() {
        let mut params = Some(serde_json::json!({
            "caller_tab_id": "foreign-tab",
            "owner": {"tab_id": "foreign-tab", "session_id": "foreign-session"},
            "body": "kept",
        }));
        sanitize_remote_context(&mut params);
        let params = params.unwrap();
        assert!(params.get("caller_tab_id").is_none());
        assert!(params["owner"].get("tab_id").is_none());
        assert_eq!(params["body"], "kept");
    }
}
