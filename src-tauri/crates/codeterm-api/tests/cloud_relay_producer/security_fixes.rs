use codeterm_mesh::account_mesh::{AccountMeshState, AccountPresence};
use codeterm_service_core::{
    ports::rest::LocalApiRouter,
    service::mesh_attachment::{AttachmentKind, IngestedAttachment},
};

use super::*;

async fn post_mesh_relay(
    app: axum::Router,
    signed_token: &str,
    cmd: &RelayCommand,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("POST")
        .uri("/api/mesh/relay")
        .header("authorization", format!("Bearer {signed_token}"))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(cmd).unwrap()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned())),
    )
}

#[tokio::test]
async fn cloud_mode_router_without_listener_serves_forwarded_tabs_and_send() {
    codeterm_service_core::ports::socket::register_test_stub();
    let (mux, target_tab) = MultiplexerState::new();
    let fixture = cloud_fixture(mux);
    let deliveries =
        fixture.bind_recorded_agent_recipient(&target_tab, "cloud-session", "cloud-nonce");
    let (cloud, _) = configure_peer_state(&fixture, "cloud-inst");
    assert_eq!(
        cloud.api_port, 0,
        "cloud-mode test runs with no TCP HTTP listener"
    );

    let (home_key, home_pub) = crown::generate_peer_keypair();
    {
        let mut mesh = cloud.mesh.lock();
        mesh.set_inbound_only_for_test(true);
        let mut home_peer = test_paired_peer("home-peer", "HomeMac");
        home_peer.public_key = home_pub;
        mesh.insert_account_peer_with_installation_for_test("home-inst".into(), home_peer);
    }

    let app = routes::create_router_with_frontend(cloud.clone(), None);
    assert!(cloud.svc.set_ext(LocalApiRouter(app.clone())).is_ok());

    // 1. Forwarded GET /api/tabs succeeds in-process without any TCP listener.
    let list_token =
        crown::sign_request_with_agent(&home_key, "home-peer", AuthenticatedOrigin::Owner, None)
            .unwrap();
    let (status, list_resp) = post_mesh_relay(
        app.clone(),
        &list_token,
        &RelayCommand {
            method: "GET".into(),
            path: "/api/tabs".into(),
            body: Value::Null,
            delivery_id: None,
        },
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list_resp}");
    assert_eq!(list_resp["ok"], true, "{list_resp}");
    assert_eq!(list_resp["status"], 200, "{list_resp}");
    assert!(
        serde_json::to_string(&list_resp["data"])
            .unwrap()
            .contains(&target_tab),
        "forwarded GET /api/tabs must include target tab: {list_resp}"
    );

    // 2. Forwarded POST /api/tabs/{id}/send succeeds in-process and delivers to the tab.
    let send_token =
        crown::sign_request_with_agent(&home_key, "home-peer", AuthenticatedOrigin::Owner, None)
            .unwrap();
    let (status, send_resp) = post_mesh_relay(
        app,
        &send_token,
        &RelayCommand {
            method: "POST".into(),
            path: format!("/api/tabs/{target_tab}/send"),
            body: json!({ "text": "hello cloud tab" }),
            delivery_id: Some("cloud-in-process-send-1".into()),
        },
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{send_resp}");
    assert_eq!(send_resp["ok"], true, "{send_resp}");
    assert_eq!(send_resp["status"], 200, "{send_resp}");
    assert_eq!(send_resp["data"]["delivery"], "sent", "{send_resp}");
    assert_eq!(send_resp["data"]["bytes_written"], true, "{send_resp}");
    // Target is already BootStage::AgentReady, so gateway_deliver submits synchronously at
    // the recorded PTY boundary without entering the deferred outbox flush loop.
    let texts = deliveries.texts.lock();
    assert_eq!(texts.len(), 1, "expected exactly one delivered message");
    assert!(
        texts[0].contains("hello cloud tab"),
        "forwarded POST /api/tabs/{{id}}/send must reach the target tab: {:?}",
        *texts
    );
}

#[tokio::test]
async fn live_drain_policy_defers_missing_peer_refuses_unpaired_or_revoked_and_closed_agent_tab() {
    let (mux, worker_tab) = MultiplexerState::new();
    let fixture = cloud_fixture(mux);
    let (cloud, _) = configure_peer_state(&fixture, "cloud-pol");
    let policy = drain_policy(&cloud.svc, cloud.mesh.clone());

    let req = OutboundPeerRequest {
        account_id: "test-account".into(),
        source_installation_id: "cloud-pol".into(),
        target_installation_id: "home-inst".into(),
        operation: OutboundOperation::WorkerAck,
        caller_context: CallerContext {
            origin: AuthenticatedOrigin::Agent,
            agent: Some(AuthenticatedAgent {
                instance_id: "cloud-pol".into(),
                tab_id: worker_tab.clone(),
                can_spawn: false,
            }),
            expires_at_ms: None,
        },
        body: serde_json::to_vec(&json!({
            "method": WORKER_ACK_RPC_METHOD,
            "request_id": "ack-policy-1",
            "protocol_version": 1,
            "params": {
                "plan_id": "pl_1",
                "task_id": "task_1",
                "attempt_id": "att_1",
                "worker_tab_id": worker_tab,
                "session_binding_nonce": "nonce-1"
            }
        }))
        .unwrap(),
    };
    let frame = OutboxFrame {
        id: OutboxId::generate(),
        sequence: 1,
        enqueued_at_ms: 1_000,
        payload_digest: req.payload_digest(),
        request: req,
    };

    // 1. Signed in with no paired peer yet -> Defer (never Allow).
    cloud.mesh.lock().apply_account_state(AccountMeshState {
        presence: AccountPresence::SignedIn {
            account_id: "test-account".into(),
        },
        ..Default::default()
    });
    assert!(
        matches!(
            policy.authorize_drain(&frame, 1_500),
            DrainPolicyDecision::Defer { .. }
        ),
        "missing paired peer must Defer, not Allow"
    );

    // 2. Registry explicitly omits target installation (unpaired) -> Refuse.
    cloud.mesh.lock().apply_account_state(AccountMeshState {
        presence: AccountPresence::SignedIn {
            account_id: "test-account".into(),
        },
        registry_members: Some(BTreeMap::from([("cloud-pol".into(), ("k".into(), 1, 1))])),
        ..Default::default()
    });
    assert!(
        matches!(
            policy.authorize_drain(&frame, 1_500),
            DrainPolicyDecision::Refuse { .. }
        ),
        "unpaired target absent from registry must Refuse"
    );

    // 3. Paired and traffic-open with live agent tab -> Allow.
    {
        let mut mesh = cloud.mesh.lock();
        mesh.apply_account_state(AccountMeshState::default());
        mesh.insert_account_peer_with_installation_for_test(
            "home-inst".into(),
            test_paired_peer("home-peer", "HomeMac"),
        );
    }
    assert_eq!(
        policy.authorize_drain(&frame, 1_500),
        DrainPolicyDecision::Allow
    );

    // 4. Target revoked -> Refuse.
    let base_account = cloud.mesh.lock().account_state().clone();
    let mut revoked_account = base_account.clone();
    revoked_account
        .revoked_installations
        .insert("home-inst".into());
    cloud.mesh.lock().apply_account_state(revoked_account);
    assert!(
        matches!(
            policy.authorize_drain(&frame, 1_500),
            DrainPolicyDecision::Refuse { .. }
        ),
        "revoked target must Refuse"
    );

    // 5. Restore target admission, then close the originating agent tab -> Refuse.
    cloud.mesh.lock().apply_account_state(base_account);
    assert_eq!(
        policy.authorize_drain(&frame, 1_500),
        DrainPolicyDecision::Allow
    );
    assert!(cloud.svc.mux.write().remove_tab(&worker_tab).is_some());
    assert_eq!(
        policy.authorize_drain(&frame, 1_500),
        DrainPolicyDecision::Refuse {
            reason: "caller_tab_closed".into(),
        }
    );
}

#[tokio::test]
async fn cloud_producer_mints_relay_token_after_transfer_artifacts() {
    let fixture = cloud_fixture(MultiplexerState::new().0);
    let (home, original_pub) = configure_peer_state(&fixture, "home-artifact");
    let cloud_addr = SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 30), 7687);
    enroll_cloud_peer(
        &home,
        "cloud-peer-art",
        "cloud-art",
        "cloud-inst-art",
        cloud_addr,
    );

    let channel = FakeCloudChannel::new();
    assert!(home
        .svc
        .set_ext(CloudIngressConnectorOverride(channel.clone()))
        .is_ok());

    let temp = tempfile::tempdir().unwrap();
    let artifact_path = temp.path().canonicalize().unwrap().join("note.md");
    std::fs::write(&artifact_path, b"cloud artifact body").unwrap();
    let artifact_display =
        codeterm_service_core::paths::strip_verbatim_prefix(&artifact_path.to_string_lossy())
            .replace('\\', "/");

    let (rotated_priv, rotated_pub) = crown::generate_peer_keypair();
    let mesh_for_rotate = home.mesh.clone();
    let rotated_priv_clone = rotated_priv.clone();
    let rotated_pub_clone = rotated_pub.clone();
    // Rotate the local signing key during artifact ingest: only a token minted AFTER
    // transfer_artifacts completes will verify under `rotated_pub`.
    channel.push_reply(move |cmd| {
        assert_eq!(cmd.method, "POST");
        assert_eq!(cmd.path, "/api/tabs/remote-tab/attachments");
        let mut mesh = mesh_for_rotate.lock();
        mesh.ident.identity.private_key = Some(rotated_priv_clone.clone());
        mesh.ident.identity.public_key = Some(rotated_pub_clone.clone());
        Ok(RelayResponse {
            ok: true,
            status: 200,
            data: Some(
                serde_json::to_value(IngestedAttachment {
                    path: "/remote/ingested/note.md".into(),
                    kind: AttachmentKind::File,
                    size: 19,
                    reference: None,
                })
                .unwrap(),
            ),
            error: None,
        })
    });

    let app = routes::create_router_with_frontend(home, None);
    let (status, resp) = post_relay_to(
        app,
        "owner-secret",
        &json!({
            "target_peer_id": "cloud-art",
            "method": "POST",
            "path": "/api/tabs/remote-tab/send",
            "body": { "text": format!("See\n<ds-document src=\"{artifact_display}\" />\n") },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert_eq!(resp["ok"], true, "{resp}");

    let recorded = channel.recorded();
    assert_eq!(
        recorded.len(),
        2,
        "expected attachment upload then relay send"
    );
    assert_eq!(recorded[0].command.path, "/api/tabs/remote-tab/attachments");
    assert_eq!(recorded[1].command.path, "/api/tabs/remote-tab/send");
    assert!(
        recorded[1].command.body["text"]
            .as_str()
            .unwrap()
            .contains("/remote/ingested/note.md"),
        "artifact path must be rewritten before send_relay"
    );

    let original_keys = [("home-artifact".to_owned(), original_pub)].into();
    let rotated_keys = [("home-artifact".to_owned(), rotated_pub)].into();
    assert!(
        crown::verify_request_claims(&recorded[0].signed_token, &original_keys).is_ok(),
        "attachment ingest uses pre-rotation signing key"
    );
    assert!(
        crown::verify_request_claims(&recorded[1].signed_token, &rotated_keys).is_ok(),
        "send_relay token must be minted after transfer_artifacts completes"
    );
    assert!(
        crown::verify_request_claims(&recorded[1].signed_token, &original_keys).is_err(),
        "send_relay must not reuse a token pre-minted before transfer_artifacts"
    );
}

#[tokio::test]
async fn home_to_cloud_relay_routes_provider_proxy_from_gossip_and_persisted_endpoint() {
    let fixture = cloud_fixture(MultiplexerState::new().0);
    let (home, _home_pub) = configure_peer_state(&fixture, "home-proxy");
    let gossip_endpoint = CloudEndpoint::ProviderProxy {
        host: "domios-gossip.fly.dev".into(),
    };
    let persisted_endpoint = CloudEndpoint::ProviderProxy {
        host: "domios-persisted.fly.dev".into(),
    };

    {
        let mut mesh = home.mesh.lock();
        mesh.insert_account_peer_with_installation_for_test(
            "cloud-inst-proxy-persisted".into(),
            test_paired_peer("cloud-peer-proxy-persisted", "fly-persisted"),
        );
        mesh.insert_account_peer_with_installation_for_test(
            "cloud-inst-proxy-gossip".into(),
            test_paired_peer("cloud-peer-proxy-gossip", "fly-gossip"),
        );
        mesh.insert_cloud_endpoint_for_test(
            "cloud-inst-proxy-gossip".into(),
            gossip_endpoint.clone(),
        );
    }

    let dir = tempfile::tempdir().unwrap();
    let store = CloudMachineStore::new(dir.path().join("cloud_machines.json"));
    store
        .save(&[CloudMachineRecord {
            machine_id: "cloud-fly-1".into(),
            name: "fly-persisted".into(),
            provider: "fly_machines".into(),
            size: "small".into(),
            state: CloudLifecycleState::Awake,
            sleep_mode: CloudSleepMode::StopBilled,
            installation_id: Some("cloud-inst-proxy-persisted".into()),
            peer_instance_id: Some("cloud-peer-proxy-persisted".into()),
            endpoint: Some(persisted_endpoint.clone()),
            rate_per_hour: None,
            created_at_ms: 1_000,
            last_awake_at_ms: Some(1_000),
            last_owner_interaction_at_ms: Some(1_000),
            accumulated_awake_secs: 0,
            period_start_ms: 1_000,
            lease_id: None,
            lease_expires_at_ms: None,
            exhaustion_pending: false,
            idle_since_ms: None,
            apply_unconfirmed: false,
            pending_request: None,
            last_error: None,
            ownership: CloudOwnershipReceipt::default(),
        }])
        .unwrap();
    home.svc
        .install_cloud_machines_for_test(Arc::new(CloudMachineController::new(
            store,
            "home-proxy".into(),
            Arc::new(SystemClock),
        )));

    let channel = FakeCloudChannel::new();
    assert!(home
        .svc
        .set_ext(CloudIngressConnectorOverride(channel.clone()))
        .is_ok());

    let app = routes::create_router_with_frontend(home, None);
    let (status, resp) = post_relay_to(
        app.clone(),
        "owner-secret",
        &json!({
            "target_peer_id": "fly-gossip",
            "method": "POST",
            "path": "/api/tabs/remote-tab/send",
            "body": { "text": "hello gossip proxy" },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert_eq!(resp["ok"], true, "{resp}");

    let (status, resp) = post_relay_to(
        app,
        "owner-secret",
        &json!({
            "target_peer_id": "fly-persisted",
            "method": "POST",
            "path": "/api/tabs/remote-tab/send",
            "body": { "text": "hello persisted proxy" },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert_eq!(resp["ok"], true, "{resp}");

    let recorded = channel.recorded();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0].route, gossip_endpoint);
    assert_eq!(recorded[0].installation_id, "cloud-inst-proxy-gossip");
    assert_eq!(recorded[1].route, persisted_endpoint);
    assert_eq!(recorded[1].installation_id, "cloud-inst-proxy-persisted");
}

#[tokio::test]
async fn managed_cloud_machine_prefers_provisioned_endpoint_over_conflicting_gossip() {
    let fixture = cloud_fixture(MultiplexerState::new().0);
    let (home, _home_pub) = configure_peer_state(&fixture, "home-auth-ep");
    let authoritative = CloudEndpoint::ProviderProxy {
        host: "domios-authoritative.fly.dev".into(),
    };
    let spoofed_direct = SocketAddrV4::new(Ipv4Addr::new(8, 8, 8, 8), 7687);
    let spoofed_host = CloudEndpoint::ProviderProxy {
        host: "domios-spoofed.fly.dev".into(),
    };

    {
        let mut mesh = home.mesh.lock();
        mesh.insert_account_peer_with_installation_for_test(
            "cloud-inst-managed-direct-gossip".into(),
            test_paired_peer("cloud-peer-managed-a", "fly-auth-a"),
        );
        mesh.insert_direct_ingress_endpoint_for_test(
            "cloud-inst-managed-direct-gossip".into(),
            spoofed_direct,
        );
        mesh.insert_account_peer_with_installation_for_test(
            "cloud-inst-managed-host-gossip".into(),
            test_paired_peer("cloud-peer-managed-b", "fly-auth-b"),
        );
        mesh.insert_cloud_endpoint_for_test("cloud-inst-managed-host-gossip".into(), spoofed_host);
    }

    let make_record = |id: &str, name: &str, inst: &str, peer: &str| CloudMachineRecord {
        machine_id: id.into(),
        name: name.into(),
        provider: "fly_machines".into(),
        size: "small".into(),
        state: CloudLifecycleState::Awake,
        sleep_mode: CloudSleepMode::StopBilled,
        installation_id: Some(inst.into()),
        peer_instance_id: Some(peer.into()),
        endpoint: Some(authoritative.clone()),
        rate_per_hour: None,
        created_at_ms: 1_000,
        last_awake_at_ms: Some(1_000),
        last_owner_interaction_at_ms: Some(1_000),
        accumulated_awake_secs: 0,
        period_start_ms: 1_000,
        lease_id: None,
        lease_expires_at_ms: None,
        exhaustion_pending: false,
        idle_since_ms: None,
        apply_unconfirmed: false,
        pending_request: None,
        last_error: None,
        ownership: CloudOwnershipReceipt::default(),
    };

    let dir = tempfile::tempdir().unwrap();
    let store = CloudMachineStore::new(dir.path().join("cloud_machines.json"));
    store
        .save(&[
            make_record(
                "cloud-fly-auth-a",
                "fly-auth-a",
                "cloud-inst-managed-direct-gossip",
                "cloud-peer-managed-a",
            ),
            make_record(
                "cloud-fly-auth-b",
                "fly-auth-b",
                "cloud-inst-managed-host-gossip",
                "cloud-peer-managed-b",
            ),
        ])
        .unwrap();
    home.svc
        .install_cloud_machines_for_test(Arc::new(CloudMachineController::new(
            store,
            "home-auth-ep".into(),
            Arc::new(SystemClock),
        )));

    let channel = FakeCloudChannel::new();
    assert!(home
        .svc
        .set_ext(CloudIngressConnectorOverride(channel.clone()))
        .is_ok());

    let app = routes::create_router_with_frontend(home, None);
    for target in ["fly-auth-a", "fly-auth-b"] {
        let (status, resp) = post_relay_to(
            app.clone(),
            "owner-secret",
            &json!({
                "target_peer_id": target,
                "method": "POST",
                "path": "/api/tabs/remote-tab/send",
                "body": { "text": format!("hello {target}") },
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{resp}");
        assert_eq!(resp["ok"], true, "{resp}");
    }

    let recorded = channel.recorded();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0].route, authoritative);
    assert_eq!(recorded[1].route, authoritative);
}
