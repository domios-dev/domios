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
    let authenticated: Result<_, (axum::http::StatusCode, String)> = (|| {
        let peer = super::relay::check_mesh_signature(&state, &headers)?;
        let origin = crate::auth::authenticated_origin(&state, &headers)?;
        let agent = crate::auth::authenticated_agent(&state, &headers)?;
        Ok((peer, origin, agent))
    })();
    let (peer, origin, agent) = match authenticated {
        Ok(authenticated) => authenticated,
        Err((_, message)) => return Json(authority_refusal(message)),
    };
    Json(handle_mesh_rpc(&state, request, origin, agent.as_ref(), move || Ok(peer)).await)
}

fn authority_refusal(message: String) -> MeshRpcResponse {
    MeshRpcResponse {
        result: None,
        error: Some(RpcError {
            code: -32003,
            message,
            data: None,
        }),
    }
}

fn origin_admitted(origin: crate::auth::AuthenticatedOrigin, method: &str) -> bool {
    origin.owner_trusted()
        || (origin == crate::auth::AuthenticatedOrigin::Agent
            && matches!(method, "plan.task.remote_ack" | "plan.task.remote_report"))
}

fn agent_lifecycle_matches(
    agent: &codeterm_service_core::mesh::crown::AuthenticatedAgent,
    request: &MeshRpcRequest,
) -> bool {
    let claim = match request.method.as_str() {
        "plan.task.remote_ack" => request
            .params
            .as_ref()
            .and_then(|params| params.get("worker_tab_id")),
        "plan.task.remote_report" => request
            .params
            .as_ref()
            .and_then(|params| params.pointer("/report/reporter_tab_id")),
        _ => None,
    };
    claim.and_then(serde_json::Value::as_str) == Some(agent.tab_id.as_str())
}

/// `authenticate` names the signed sender; a relay passes the sender it already verified,
/// because its loopback re-sign would otherwise present this instance as the caller.
pub(super) async fn handle_mesh_rpc(
    state: &AppState,
    request: MeshRpcRequest,
    origin: crate::auth::AuthenticatedOrigin,
    agent: Option<&codeterm_service_core::mesh::crown::AuthenticatedAgent>,
    authenticate: impl FnOnce() -> Result<String, (axum::http::StatusCode, String)>,
) -> MeshRpcResponse {
    if !origin_admitted(origin, &request.method) {
        return authority_refusal(
            "Mesh RPC requires an owner credential or an explicit peer lifecycle capability".into(),
        );
    }
    if origin == crate::auth::AuthenticatedOrigin::Agent
        && !agent.is_some_and(|agent| agent_lifecycle_matches(agent, &request))
    {
        return authority_refusal(
            "Remote lifecycle request must match the signed originating agent".into(),
        );
    }
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
    let authenticated_peer = if matches!(
        request.method.as_str(),
        "plan.task.remote_ack" | "plan.task.remote_report"
    ) {
        let peer = match authenticate() {
            Ok(peer) => peer,
            Err((_, message)) => return authority_refusal(message),
        };
        let peer = if origin == crate::auth::AuthenticatedOrigin::Agent {
            agent.map(|agent| agent.instance_id.clone()).unwrap_or(peer)
        } else {
            peer
        };
        let full = {
            let mesh = state.mesh.lock();
            mesh.traffic_open(&peer)
                && mesh
                    .paired_peers
                    .get(&peer)
                    .is_some_and(|peer| super::peer_has_full_access(Some(&peer.access_level)))
        };
        if !full {
            return authority_refusal(
                "remote task lifecycle mutation requires Full mesh access".into(),
            );
        }
        Some(peer)
    } else {
        None
    };
    let actor = agent
        .map(|agent| format!("{}:{}", agent.instance_id, agent.tab_id))
        .unwrap_or_default();
    let (entry, owns_dispatch) = response_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .acquire(format!(
            "{origin:?}:{actor}:{}:{}:{}",
            authenticated_peer.as_deref().unwrap_or_default(),
            request.method,
            request.request_id
        ));
    if !owns_dispatch {
        return entry.wait().await;
    }
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
