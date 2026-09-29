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
            error: Some(crate::socket::server::JsonRpcError {
                code: -32602,
                message: "request_id must contain 1 to 128 characters".to_string(),
                data: None,
            }),
        };
    }
    if let Some(error) = protocol::version_skew_gate(
        protocol::client_protocol_version(request.protocol_version.as_ref()),
        protocol::PROTOCOL_VERSION,
        protocol::RECOVERY_COMMAND,
    ) {
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
                    error: Some(crate::socket::server::JsonRpcError {
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
                    error: Some(crate::socket::server::JsonRpcError {
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
mod tests {
    use super::{method_allowed, sanitize_remote_context, ResponseCache, CACHE_CAPACITY};

    fn app_state(
        fixture: &crate::service::test_fixture::ServiceFixture,
        peer: Option<crate::mesh::types::PairedPeer>,
    ) -> crate::api::routes::AppState {
        let config = crate::config::schema::AppConfig::default();
        let mut mesh = crate::mesh::MeshManager::new("rpc-origin".into(), 0);
        if let Some(peer) = peer {
            mesh.paired_peers.insert(peer.instance_id.clone(), peer);
        }
        crate::api::routes::AppState {
            svc: std::sync::Arc::new(fixture.service.clone()),
            cron: std::sync::Arc::new(parking_lot::Mutex::new(
                crate::cron::store::CronStore::new_with_path(
                    fixture.temp.path().join("mesh-rpc-crons.json"),
                ),
            )),
            cron_wake: std::sync::Arc::new(std::sync::Condvar::new()),
            api_port: 0,
            update_state: std::sync::Arc::new(parking_lot::Mutex::new(
                crate::update::UpdateState {
                    install_method: crate::update::InstallMethod::Unknown,
                    last_check: None,
                    last_check_time: None,
                    channel: config.updater.channel,
                    phase: crate::update::UpdatePhase::Idle,
                    last_error: None,
                    phase_changed_at: None,
                    downloaded_bytes: 0,
                    download_total_bytes: None,
                },
            )),
            config: std::sync::Arc::new(parking_lot::Mutex::new(config)),
            mesh: std::sync::Arc::new(parking_lot::Mutex::new(mesh)),
            keepawake: std::sync::Arc::new(parking_lot::Mutex::new(None)),
            resize_owner: std::sync::Arc::new(parking_lot::Mutex::new(Default::default())),
            rest_api: std::sync::Arc::new(crate::startup::RestApiControl::new()),
            test_clock: None,
            events: fixture.service.events.clone(),
            web_dist: None,
            snapshot_bound: crate::api::snapshot_bound::SnapshotBound::default(),
        }
    }

    fn remote_peer(
        access_level: crate::mesh::types::AccessLevel,
    ) -> (crate::mesh::types::PairedPeer, String) {
        let (private_key, public_key) = crate::mesh::crown::generate_peer_keypair();
        (
            crate::mesh::types::PairedPeer {
                instance_id: "peer-rpc-test".into(),
                instance_name: "Peer".into(),
                addresses: Vec::new(),
                api_port: 0,
                public_key,
                paired_at: "now".into(),
                last_seen: None,
                online: true,
                is_primary: false,
                access_level,
                version: None,
                web_ui: None,
                relay_protocol_version: 0,
                consecutive_misses: 0,
                last_active_ms: None,
            },
            private_key,
        )
    }

    #[cfg_attr(not(windows), ignore = "requires the desktop Tauri test runtime")]
    #[tokio::test]
    async fn mesh_rpc_remote_lifecycle_rejects_unauthenticated_unpaired_and_read_only_peers() {
        let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        for method in ["plan.task.remote_ack", "plan.task.remote_report"] {
            for (case, paired_peer, authorization, expected) in [
                (
                    "unauthenticated",
                    None,
                    None,
                    "Invalid or unknown signature",
                ),
                ("unpaired", None, Some(true), "Invalid or unknown signature"),
                (
                    "read_only",
                    Some(crate::mesh::types::AccessLevel::ReadOnly),
                    Some(true),
                    "requires Full mesh access",
                ),
            ] {
                let signed =
                    remote_peer(paired_peer.unwrap_or(crate::mesh::types::AccessLevel::Full));
                let peer = if case == "read_only" {
                    Some(signed.0.clone())
                } else {
                    None
                };
                let state = app_state(&fixture, peer);
                let headers = if authorization == Some(true) {
                    let token =
                        crate::mesh::crown::sign_request(&signed.1, &signed.0.instance_id).unwrap();
                    let mut headers = axum::http::HeaderMap::new();
                    headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
                    headers
                } else {
                    axum::http::HeaderMap::new()
                };
                let response = super::mesh_rpc(
                    axum::extract::State(state),
                    headers,
                    axum::Json(super::MeshRpcRequest {
                        method: method.into(),
                        params: Some(serde_json::json!({})),
                        request_id: format!("{method}-{case}"),
                        protocol_version: Some(serde_json::json!(
                            crate::socket::server::protocol::PROTOCOL_VERSION
                        )),
                    }),
                )
                .await
                .0;
                let error = response
                    .error
                    .expect("unauthorized remote lifecycle mutation");
                assert!(
                    error.message.contains(expected),
                    "{}: {}",
                    case,
                    error.message
                );
            }
        }
    }

    #[cfg_attr(not(windows), ignore = "requires the desktop Tauri test runtime")]
    #[tokio::test]
    async fn relayed_remote_lifecycle_uses_the_verified_sender_access() {
        let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        for (access, full) in [
            (crate::mesh::types::AccessLevel::Full, true),
            (crate::mesh::types::AccessLevel::ReadOnly, false),
        ] {
            let (peer, _) = remote_peer(access);
            let peer_id = peer.instance_id.clone();
            let state = app_state(&fixture, Some(peer));
            let response = super::handle_mesh_rpc(
                &state,
                super::MeshRpcRequest {
                    method: "plan.task.remote_ack".into(),
                    params: Some(serde_json::json!({})),
                    request_id: format!("relayed-{full}"),
                    protocol_version: Some(serde_json::json!(
                        crate::socket::server::protocol::PROTOCOL_VERSION
                    )),
                },
                move || Ok(peer_id),
            )
            .await;
            let message = response.error.expect("empty params still refuse").message;
            assert_eq!(
                message.contains("requires Full mesh access"),
                !full,
                "{message}"
            );
        }
    }

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
