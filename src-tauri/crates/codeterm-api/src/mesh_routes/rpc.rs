use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

use axum::{extract::State as AxumState, http::HeaderMap, Json};
use serde::{Deserialize, Serialize};

use crate::routes::AppState;
use codeterm_service_core::ports::socket::{bridge, RpcError};

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
    error: Option<RpcError>,
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
    if method == "team.adopt_agent" {
        return false;
    }
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
    let auth_state = state.clone();
    Json(
        handle_mesh_rpc(&state, request, move || {
            super::relay::check_mesh_relay_auth(&auth_state, &headers)
        })
        .await,
    )
}

/// `authenticate` names the signed sender; a relay passes the sender it already verified,
/// because its loopback re-sign would otherwise present this instance as the caller.
pub(super) async fn handle_mesh_rpc(
    state: &AppState,
    request: MeshRpcRequest,
    authenticate: impl FnOnce() -> Result<String, (axum::http::StatusCode, String)>,
) -> MeshRpcResponse {
    if request.request_id.is_empty() || request.request_id.len() > 128 {
        return MeshRpcResponse {
            result: None,
            error: Some(RpcError {
                code: -32602,
                message: "request_id must contain 1 to 128 characters".to_string(),
                data: None,
            }),
        };
    }
    if let Some(error) = (bridge().version_skew_gate)(request.protocol_version.as_ref()) {
        return MeshRpcResponse {
            result: None,
            error: Some(error),
        };
    }
    let (entry, owns_dispatch) = response_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .acquire(request.request_id);
    if !owns_dispatch {
        return entry.wait().await;
    }

    let authenticated_peer = if matches!(
        request.method.as_str(),
        "plan.task.remote_ack" | "plan.task.remote_report"
    ) {
        match authenticate() {
            Ok(peer_id)
                if state
                    .mesh
                    .lock()
                    .paired_peers
                    .get(&peer_id)
                    .is_some_and(|peer| super::peer_has_full_access(Some(&peer.access_level))) =>
            {
                Some(peer_id)
            }
            Ok(_) => {
                entry.complete(MeshRpcResponse {
                    result: None,
                    error: Some(RpcError {
                        code: -32003,
                        message: "remote task lifecycle mutation requires Full mesh access".into(),
                        data: None,
                    }),
                });
                return entry.wait().await;
            }
            Err((_, message)) => {
                entry.complete(MeshRpcResponse {
                    result: None,
                    error: Some(RpcError {
                        code: -32003,
                        message,
                        data: None,
                    }),
                });
                return entry.wait().await;
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
                (bridge().dispatch_mesh_rpc)(&method, params, &svc, &peer_id)
            } else {
                (bridge().dispatch_service_rpc)(&method, params, &svc)
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
                error: Some(RpcError {
                    code: -32603,
                    message: format!("mesh RPC dispatch failed: {error}"),
                    data: None,
                }),
            },
        }
    } else {
        MeshRpcResponse {
            result: None,
            error: Some(RpcError {
                code: -32601,
                message: format!("method '{}' is not available over mesh RPC", request.method),
                data: None,
            }),
        }
    };

    entry.complete(response.clone());
    response
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
mod tests;
