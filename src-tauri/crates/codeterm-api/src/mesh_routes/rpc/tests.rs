use super::{method_allowed, sanitize_remote_context, ResponseCache, CACHE_CAPACITY};

fn app_state(
    fixture: &codeterm_service_core::service::test_fixture::ServiceFixture,
    peer: Option<codeterm_service_core::mesh::types::PairedPeer>,
) -> crate::routes::AppState {
    let config = codeterm_service_core::config::schema::AppConfig::default();
    let mut mesh = codeterm_service_core::mesh::MeshManager::new("rpc-origin".into(), 0);
    if let Some(peer) = peer {
        mesh.insert_account_peer_for_test(peer);
    }
    crate::routes::AppState {
        frontend_dev_url: None,
        svc: std::sync::Arc::new(fixture.service.clone()),
        cron: std::sync::Arc::new(parking_lot::Mutex::new(
            codeterm_service_core::cron::store::CronStore::new_with_path(
                fixture.temp.path().join("mesh-rpc-crons.json"),
            ),
        )),
        cron_wake: std::sync::Arc::new(std::sync::Condvar::new()),
        api_port: 0,
        update_state: std::sync::Arc::new(parking_lot::Mutex::new(codeterm_update::UpdateState {
            install_method: codeterm_update::InstallMethod::Unknown,
            last_check: None,
            last_check_time: None,
            channel: config.updater.channel,
            phase: codeterm_update::UpdatePhase::Idle,
            last_error: None,
            phase_changed_at: None,
            downloaded_bytes: 0,
            download_total_bytes: None,
        })),
        config: std::sync::Arc::new(parking_lot::Mutex::new(config)),
        mesh: std::sync::Arc::new(parking_lot::Mutex::new(mesh)),
        keepawake: std::sync::Arc::new(parking_lot::Mutex::new(None)),
        resize_owner: std::sync::Arc::new(parking_lot::Mutex::new(Default::default())),
        rest_api: std::sync::Arc::new(codeterm_service_core::startup::RestApiControl::new()),
        test_clock: None,
        events: fixture.service.events.clone(),
        web_dist: None,
    }
}

fn remote_peer(
    access_level: codeterm_service_core::mesh::types::AccessLevel,
) -> (codeterm_service_core::mesh::types::PairedPeer, String) {
    let (private_key, public_key) = codeterm_service_core::mesh::crown::generate_peer_keypair();
    (
        codeterm_service_core::mesh::types::PairedPeer {
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

#[tokio::test]
async fn mesh_rpc_remote_lifecycle_rejects_unauthenticated_unpaired_and_read_only_peers() {
    codeterm_service_core::ports::socket::register_test_stub();
    let (mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
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
                Some(codeterm_service_core::mesh::types::AccessLevel::ReadOnly),
                Some(true),
                "requires Full mesh access",
            ),
        ] {
            let signed = remote_peer(
                paired_peer.unwrap_or(codeterm_service_core::mesh::types::AccessLevel::Full),
            );
            let peer = if case == "read_only" {
                Some(signed.0.clone())
            } else {
                None
            };
            let state = app_state(&fixture, peer);
            let headers = if authorization == Some(true) {
                let token = codeterm_service_core::mesh::crown::sign_request(
                    &signed.1,
                    &signed.0.instance_id,
                )
                .unwrap();
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
                        (codeterm_service_core::ports::socket::bridge().protocol_version)()
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

#[tokio::test]
async fn relayed_remote_lifecycle_uses_the_verified_sender_access() {
    codeterm_service_core::ports::socket::register_test_stub();
    let (mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
    for (access, full) in [
        (codeterm_service_core::mesh::types::AccessLevel::Full, true),
        (
            codeterm_service_core::mesh::types::AccessLevel::ReadOnly,
            false,
        ),
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
                    (codeterm_service_core::ports::socket::bridge().protocol_version)()
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
        "team.adopt_agent",
        "tab.send_text",
        "system.identify",
        "plugin.exec",
        "cron.list",
    ] {
        assert!(!method_allowed(method), "{method}");
    }
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
