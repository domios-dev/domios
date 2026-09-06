use super::*;
use crate::mesh::types::RelayResponse;

#[test]
fn authorized_workspace_managers_receive_typed_close_provenance() {
    let method = Method::POST;
    let path = "/api/workspaces/ws_remote/close";
    for access in [
        crate::mesh::types::AccessLevel::Full,
        crate::mesh::types::AccessLevel::Custom(vec![
            crate::mesh::types::Permission::ManageWorkspaces,
        ]),
    ] {
        assert_eq!(
            authorize_peer_relay(
                "signed-peer",
                &access,
                &method,
                path,
                &serde_json::Value::Null
            ),
            PeerRelayAuthorization::WorkspaceClose {
                peer_id: "signed-peer",
                workspace_id: "ws_remote",
                force: false,
            }
        );
    }
}

#[test]
fn readonly_and_agent_isolated_peers_are_rejected_before_close_dispatch() {
    let method = Method::POST;
    let path = "/api/workspaces/ws_remote/close";
    for access in [
        crate::mesh::types::AccessLevel::ReadOnly,
        crate::mesh::types::AccessLevel::AgentIsolated,
    ] {
        assert_eq!(
            authorize_peer_relay(
                "signed-peer",
                &access,
                &method,
                path,
                &serde_json::Value::Null
            ),
            PeerRelayAuthorization::Rejected
        );
    }
}

#[test]
fn caller_fields_in_relay_body_cannot_forge_mesh_provenance() {
    let command = crate::mesh::types::RelayCommand {
        method: "POST".into(),
        path: "/api/workspaces/ws_remote/close".into(),
        body: serde_json::json!({
            "caller_tab_id": "forged-tab"
        }),
        delivery_id: None,
    };
    let method = Method::from_bytes(command.method.as_bytes()).unwrap();

    let authorization = authorize_peer_relay(
        "cryptographically-verified-peer",
        &crate::mesh::types::AccessLevel::Full,
        &method,
        &command.path,
        &command.body,
    );

    assert_eq!(
        authorization,
        PeerRelayAuthorization::WorkspaceClose {
            peer_id: "cryptographically-verified-peer",
            workspace_id: "ws_remote",
            force: false,
        }
    );
}

#[test]
fn only_the_exact_workspace_close_route_gets_typed_provenance() {
    let access = crate::mesh::types::AccessLevel::Full;
    for (method, path) in [
        (Method::GET, "/api/workspaces/ws_remote/close"),
        (Method::POST, "/api/workspaces/ws_remote/close/extra"),
        (
            Method::POST,
            "/api/workspaces/ws_remote/close?caller_tab_id=x",
        ),
        (Method::POST, "/api/workspaces/ws_remote/tabs/tab_1/close"),
    ] {
        assert_eq!(
            authorize_peer_relay(
                "signed-peer",
                &access,
                &method,
                path,
                &serde_json::Value::Null
            ),
            PeerRelayAuthorization::Forward,
            "{method} {path}"
        );
    }
}

fn paired_peer_fixture(
    id: &str,
    name: &str,
    online: bool,
    protocol: u16,
) -> crate::mesh::types::PairedPeer {
    crate::mesh::types::PairedPeer {
        instance_id: id.into(),
        instance_name: name.into(),
        addresses: vec![crate::mesh::types::PeerAddress::from_ip("100.65.0.30")],
        api_port: 7685,
        public_key: String::new(),
        paired_at: "now".into(),
        last_seen: None,
        online,
        is_primary: false,
        access_level: crate::mesh::types::AccessLevel::Full,
        version: None,
        web_ui: None,
        relay_protocol_version: protocol,
        consecutive_misses: 0,
        last_active_ms: None,
    }
}

/// codeterm#434: two paired records share an instance_name — one online at
/// the current relay protocol, one dead at an old one. Name-based relay
/// resolution must return the live record on every call, never flip with
/// hashmap iteration order.
#[test]
fn peer_relay_resolves_online_record_deterministically_over_duplicate_names() {
    let mut mesh = crate::mesh::MeshManager::new("test-host".into(), 7685);
    mesh.disk_persist = false;
    // `MeshManager::new` loads real disk state — start from empty so this
    // test is never polluted by the running machine's own paired peers.
    mesh.paired_peers.clear();
    mesh.paired_peers.insert(
        "a4db6f3d-dead".into(),
        paired_peer_fixture("a4db6f3d-dead", "BLACKSTAR", false, 0),
    );
    mesh.paired_peers.insert(
        "2556b3f9-live".into(),
        paired_peer_fixture(
            "2556b3f9-live",
            "BLACKSTAR",
            true,
            crate::mesh::types::RELAY_PROTOCOL_VERSION,
        ),
    );

    for _ in 0..50 {
        let (_, name) =
            resolve_relay_peer(&mesh, "BLACKSTAR").expect("resolves to the live record");
        assert_eq!(name, "BLACKSTAR");
    }
    // Case-insensitive name resolution still lands on the live record.
    assert!(resolve_relay_peer(&mesh, "blackstar").is_ok());
    // Id lookup returns the EXACT record for that id, bypassing the name
    // ambiguity — but not the protocol gate: the dead id is still protocol 0.
    let (_, live_id_name) =
        resolve_relay_peer(&mesh, "2556b3f9-live").expect("the live id resolves");
    assert_eq!(live_id_name, "BLACKSTAR");
    match resolve_relay_peer(&mesh, "a4db6f3d-dead") {
        Ok(_) => panic!("the dead id's own record is protocol 0 — must still refuse"),
        Err((status, _)) => assert_eq!(status, StatusCode::UPGRADE_REQUIRED),
    }
}

/// When every record sharing a name is offline, the refusal is structured
/// JSON naming both records and the id-routed form of the same call —
/// never a prose sentence the caller must pattern-match.
#[test]
fn peer_relay_refusal_names_both_dead_records_and_the_id_route() {
    let mut mesh = crate::mesh::MeshManager::new("test-host".into(), 7685);
    mesh.disk_persist = false;
    mesh.paired_peers.clear();
    mesh.paired_peers.insert(
        "a4db6f3d-dead".into(),
        paired_peer_fixture("a4db6f3d-dead", "BLACKSTAR", false, 0),
    );
    mesh.paired_peers.insert(
        "c9112abc-alsodead".into(),
        paired_peer_fixture("c9112abc-alsodead", "BLACKSTAR", false, 1),
    );

    let (status, body) = match resolve_relay_peer(&mesh, "BLACKSTAR") {
        Ok(_) => panic!("no online record exists for this name — must not resolve"),
        Err(error) => error,
    };
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("structured JSON body");
    assert_eq!(parsed["reason"], PEER_NAME_NO_ONLINE_RECORD_REASON);
    assert_eq!(parsed["name"], "BLACKSTAR");
    let ids: Vec<String> = parsed["candidates"]
        .as_array()
        .expect("candidates array")
        .iter()
        .map(|c| c["instance_id"].as_str().unwrap().to_string())
        .collect();
    assert!(ids.contains(&"a4db6f3d-dead".to_string()), "got: {ids:?}");
    assert!(
        ids.contains(&"c9112abc-alsodead".to_string()),
        "got: {ids:?}"
    );
    let route_ids: Vec<String> = parsed["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            c["route_by_id"]["target_peer_id"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert!(route_ids.contains(&"a4db6f3d-dead".to_string()));
    assert!(route_ids.contains(&"c9112abc-alsodead".to_string()));
}

#[test]
fn peer_relay_stamps_delivery_id_only_on_sends() {
    let id = relay_send_delivery_id("POST", "/api/tabs/abc/send")
        .expect("a forwarded Tab send carries a delivery id");
    assert!(uuid::Uuid::parse_str(&id).is_ok(), "a v4 uuid: {id}");
    assert!(
        relay_send_delivery_id("POST", "/api/tabs/abc/send?x=1").is_some(),
        "query string does not hide a send"
    );
    assert!(
        relay_send_delivery_id("GET", "/api/tabs/abc/send").is_none(),
        "a GET is never a logical send"
    );
    assert!(relay_send_delivery_id("POST", "/api/tabs").is_none());
    assert!(relay_send_delivery_id("POST", "/api/agents/spawn").is_none());
}

#[test]
fn dedupe_holds_success_and_in_doubt_but_releases_undelivered() {
    let ok = RelayResponse {
        ok: true,
        status: 200,
        data: None,
        error: None,
    };
    assert!(outcome_holds_dedupe(&ok), "an optimistic success dedupes");

    let in_doubt = RelayResponse {
        ok: false,
        status: 502,
        data: Some(serde_json::json!({"error": {"bytes_written": true}})),
        error: None,
    };
    assert!(
        outcome_holds_dedupe(&in_doubt),
        "bytes reached the PTY — retrying risks a duplicate, so keep it deduped"
    );

    let not_delivered = RelayResponse {
        ok: false,
        status: 404,
        data: Some(serde_json::json!({"error": {"bytes_written": false}})),
        error: None,
    };
    assert!(
        !outcome_holds_dedupe(&not_delivered),
        "nothing was written — safe to retry, so the window must release it"
    );

    let transport = RelayResponse {
        ok: false,
        status: 502,
        data: None,
        error: Some("Local relay failed: connection refused".into()),
    };
    assert!(
        !outcome_holds_dedupe(&transport),
        "an unknown transport failure delivered nothing — allow the retry"
    );
}

#[test]
fn relay_forwards_structured_delivery_code_not_bare_status() {
    let body = crate::service::delivery_error::DeliveryError::SubmitUnconfirmed {
        tab: "53854ac8".into(),
        detail: "submit not confirmed after 3s".into(),
    }
    .to_json();
    let summary = relay_error_summary(502, &body);
    assert!(summary.contains("submit_unconfirmed"), "got: {summary}");
    assert!(
        summary.contains("bytes reached the target PTY"),
        "caller must learn the payload landed; got: {summary}"
    );
    assert!(!summary.starts_with("HTTP "));
}

#[test]
fn relay_marks_zero_byte_refusals_explicitly() {
    let body = crate::service::delivery_error::DeliveryError::TabBusyRefused {
        tab: "53854ac8".into(),
        detail: "still booting".into(),
    }
    .to_json();
    let summary = relay_error_summary(409, &body);
    assert!(summary.contains("tab_busy_refused"), "got: {summary}");
    assert!(summary.contains("nothing written"), "got: {summary}");
}

#[test]
fn relay_unreachable_tab_is_named_not_bare_404() {
    let body = crate::service::delivery_error::DeliveryError::TabNotReachableOnPeer {
        tab: "ghost".into(),
    }
    .to_json();
    let summary = relay_error_summary(404, &body);
    assert!(
        summary.contains("tab_not_reachable_on_peer"),
        "got: {summary}"
    );
    assert!(summary.contains("nothing written"), "got: {summary}");
}

#[test]
fn relay_falls_back_to_status_for_unstructured_bodies() {
    assert_eq!(
        relay_error_summary(500, &serde_json::json!(null)),
        "HTTP 500"
    );
    assert_eq!(
        relay_error_summary(403, &serde_json::json!({"error": "plain string"})),
        "HTTP 403"
    );
}

fn ok_resp(data: serde_json::Value) -> RelayResponse {
    RelayResponse {
        ok: true,
        status: 200,
        data: Some(data),
        error: None,
    }
}

#[test]
fn spawn_relay_registration_extracts_worker_fields() {
    let body = serde_json::json!({"agent":"claude","report_to":"4d479b6c","model":"opus"});
    let resp = ok_resp(serde_json::json!({"tab_id":"53854ac8","title":"Canvas"}));
    let (report_to, remote_tab, title, model) =
        spawn_registration_from_relay("POST", "/api/agents/spawn", &body, &resp)
            .expect("spawn relay registers");
    assert_eq!(report_to, "4d479b6c");
    assert_eq!(remote_tab, "53854ac8");
    assert_eq!(title, "Canvas");
    assert_eq!(model.as_deref(), Some("opus"));
}

#[test]
fn spawn_relay_registration_skips_non_spawn_and_unregisterable() {
    let body = serde_json::json!({"report_to":"main"});
    let resp = ok_resp(serde_json::json!({"tab_id":"p1"}));
    // Wrong path / method.
    assert!(spawn_registration_from_relay("POST", "/api/tabs/x/send", &body, &resp).is_none());
    assert!(spawn_registration_from_relay("GET", "/api/agents/spawn", &body, &resp).is_none());
    // No report target.
    let no_report = serde_json::json!({"agent":"claude"});
    assert!(
        spawn_registration_from_relay("POST", "/api/agents/spawn", &no_report, &resp).is_none()
    );
    // Failed relay (ok:false) must not register.
    let failed = RelayResponse {
        ok: false,
        status: 500,
        data: Some(serde_json::json!({"tab_id":"p1"})),
        error: Some("boom".into()),
    };
    assert!(spawn_registration_from_relay("POST", "/api/agents/spawn", &body, &failed).is_none());
}

#[test]
fn relay_spawn_tags_origin_peer() {
    let mut body = serde_json::json!({"agent": "claude", "report_to": "main"});
    tag_relay_spawn_source(
        &mut body,
        "POST",
        "/api/agents/spawn",
        "peer-mac",
        "Mac-Air",
    );
    assert_eq!(body["origin_peer"], serde_json::json!("Mac-Air"));
    assert_eq!(body["report_to"], serde_json::json!("main"));
}

#[test]
fn relay_spawn_stamps_ownership_peer_from_signed_sender() {
    let mut body = serde_json::json!({
        "agent": "claude",
        "role": "Worker",
        "report_to": "af5919cb",
        "ownership": {
            "controller_title": "Puccini",
            "controller_tab_id": "af5919cb",
            "peer_id": "SPOOF",
            "peer_name": "SPOOF"
        }
    });
    tag_relay_spawn_source(
        &mut body,
        "POST",
        "/api/agents/spawn",
        "peer-mac",
        "Mac-Air",
    );
    assert_eq!(body["ownership"]["peer_id"], serde_json::json!("peer-mac"));
    assert_eq!(body["ownership"]["peer_name"], serde_json::json!("Mac-Air"));
    assert_eq!(
        body["ownership"]["controller_title"],
        serde_json::json!("Puccini")
    );
}

#[test]
fn relay_spawn_origin_overrides_spoofed_body() {
    let mut body = serde_json::json!({"agent": "claude", "origin_peer": "I-AM-ROOT"});
    tag_relay_spawn_source(
        &mut body,
        "post",
        "/api/agents/spawn",
        "peer-mac",
        "Mac-Air",
    );
    assert_eq!(body["origin_peer"], serde_json::json!("Mac-Air"));
}

#[test]
fn relay_spawn_strips_origin_local_parent_and_promotes_to_report() {
    // Regression: forwarding an origin-local --parent-tab made the remote spawn
    // validate it as a target-local hierarchy parent and orphan a tab.
    let mut body = serde_json::json!({
        "agent": "codex",
        "role": "Worker",
        "parent_tab_id": "af5919cb"
    });
    tag_relay_spawn_source(
        &mut body,
        "POST",
        "/api/agents/spawn",
        "peer-mac",
        "Mac-Air",
    );
    assert_eq!(
        body.get("parent_tab_id"),
        None,
        "origin-local parent stripped"
    );
    assert_eq!(body["origin_peer"], serde_json::json!("Mac-Air"));
    assert_eq!(
        body["report_to"],
        serde_json::json!("af5919cb"),
        "parent promoted to origin report target"
    );
}

#[test]
fn relay_spawn_strips_parent_but_keeps_explicit_report_to() {
    let mut body = serde_json::json!({
        "role": "Worker",
        "parent_tab_id": "af5919cb",
        "report_to": "main"
    });
    tag_relay_spawn_source(
        &mut body,
        "POST",
        "/api/agents/spawn",
        "peer-mac",
        "Mac-Air",
    );
    assert_eq!(body.get("parent_tab_id"), None);
    assert_eq!(
        body["report_to"],
        serde_json::json!("main"),
        "explicit report_to wins over promoted parent"
    );
}

#[test]
fn relay_spawn_preserves_parent_for_non_worker_roles() {
    // Non-Worker roles keep parent_tab_id while still getting origin_peer stamped.
    for role in [
        serde_json::json!("Manager"),
        serde_json::json!("Orchestrator"),
        serde_json::Value::Null,
    ] {
        let mut body = serde_json::json!({
            "agent": "codex",
            "parent_tab_id": "af5919cb"
        });
        if !role.is_null() {
            body["role"] = role.clone();
        }
        tag_relay_spawn_source(
            &mut body,
            "POST",
            "/api/agents/spawn",
            "peer-mac",
            "Mac-Air",
        );
        assert_eq!(
            body["parent_tab_id"],
            serde_json::json!("af5919cb"),
            "non-Worker ({role}) must keep parent_tab_id"
        );
        assert_eq!(body["origin_peer"], serde_json::json!("Mac-Air"));
        assert_eq!(
            body.get("report_to"),
            None,
            "no report promotion for non-Worker"
        );
    }
}

#[test]
fn relay_spawn_ignores_blank_parent() {
    let mut body = serde_json::json!({"role": "Worker", "parent_tab_id": "  "});
    tag_relay_spawn_source(
        &mut body,
        "POST",
        "/api/agents/spawn",
        "peer-mac",
        "Mac-Air",
    );
    assert_eq!(body.get("parent_tab_id"), None);
    assert_eq!(
        body.get("report_to"),
        None,
        "blank parent is not a report target"
    );
}

#[test]
fn relay_spawn_tagger_ignores_non_spawn_paths() {
    let mut body = serde_json::json!({"text": "hi"});
    tag_relay_spawn_source(&mut body, "POST", "/api/tabs/x/send", "peer-mac", "Mac-Air");
    assert_eq!(body.get("origin_peer"), None);
    let mut body2 = serde_json::json!({});
    tag_relay_spawn_source(
        &mut body2,
        "GET",
        "/api/agents/spawn",
        "peer-mac",
        "Mac-Air",
    );
    assert_eq!(
        body2.get("origin_peer"),
        None,
        "GET spawn must not be tagged"
    );
}

#[test]
fn relay_send_tags_originating_peer() {
    let mut body = serde_json::json!({"text": "hi", "source_tab": "e8a0ac20"});
    tag_relay_send_source(&mut body, "POST", "/api/tabs/general/send", "BLACKSTAR");
    assert_eq!(body["source_peer"], serde_json::json!("BLACKSTAR"));
    // Text is NOT pre-wrapped — the send handler owns prefixing.
    assert_eq!(body["text"], serde_json::json!("hi"));
    assert_eq!(body["source_tab"], serde_json::json!("e8a0ac20"));
}

#[test]
fn relay_send_peer_overrides_spoofed_body_value() {
    let mut body = serde_json::json!({"text": "hi", "source_peer": "I-AM-ROOT"});
    tag_relay_send_source(&mut body, "post", "/api/tabs/x/send", "BLACKSTAR");
    assert_eq!(body["source_peer"], serde_json::json!("BLACKSTAR"));
}

#[test]
fn relay_non_send_paths_untouched() {
    let mut body = serde_json::json!({"foo": "bar"});
    tag_relay_send_source(&mut body, "GET", "/api/tabs", "BLACKSTAR");
    assert_eq!(body.get("source_peer"), None);
    let mut body2 = serde_json::json!({"foo": "bar"});
    tag_relay_send_source(&mut body2, "GET", "/api/tabs/x/send", "BLACKSTAR");
    assert_eq!(
        body2.get("source_peer"),
        None,
        "GET /send must not be tagged"
    );
}

#[test]
fn valid_relay_paths_pass() {
    assert!(validate_relay_path("/api/tabs/abc/output").is_ok());
    assert!(validate_relay_path("/api/tabs/abc/send").is_ok());
    assert!(validate_relay_path("/api/mesh/status").is_ok());
    assert!(validate_relay_path("/api/workspaces").is_ok());
}

#[test]
fn non_api_paths_rejected() {
    assert!(validate_relay_path("/etc/passwd").is_err());
    assert!(validate_relay_path("/api").is_err());
    assert!(validate_relay_path("/api/").is_ok()); // bare minimum
}

#[test]
fn userinfo_ssrf_rejected() {
    assert!(validate_relay_path("/api/tabs/send?url=http://evil@evil.com").is_err());
}

#[test]
fn scheme_prefix_rejected() {
    assert!(validate_relay_path("/api/http://evil.com").is_err());
    assert!(validate_relay_path("/api/https://evil.com").is_err());
    assert!(validate_relay_path("/api/file:///etc/passwd").is_err());
}

#[test]
fn protocol_relative_rejected() {
    assert!(validate_relay_path("//evil.com/api/tabs").is_err());
}

#[test]
fn only_cursor_streams_are_relayable() {
    assert!(is_streamable_relay_path("/api/events/stream"));
    assert!(is_streamable_relay_path("/api/output/stream"));
    assert!(!is_streamable_relay_path("/api/tabs/abc/send"));
    assert!(!is_streamable_relay_path("/api/config"));
}

#[test]
fn relay_asset_whitelist_is_ui_scoped() {
    assert!(is_relay_asset_path("plugins/git/ui/index.html"));
    assert!(is_relay_asset_path("plugins/git/ui/assets/app.js"));
    // Not a ui/ path, not a plugin path, or traversal — all rejected.
    assert!(!is_relay_asset_path("plugins/git/manifest.json"));
    assert!(!is_relay_asset_path("tabs/abc/ui/x"));
    assert!(!is_relay_asset_path("plugins/git/ui/../../../etc/passwd"));
}

#[test]
fn relay_refusal_distinguishes_unroutable_from_unpaired() {
    let mut mesh = crate::mesh::MeshManager::new("test-host".into(), 7685);
    mesh.disk_persist = false;
    let mut peer = crate::mesh::types::PairedPeer {
        instance_id: "peer-a".into(),
        instance_name: "TestPeer".into(),
        addresses: Vec::new(),
        api_port: 7685,
        public_key: String::new(),
        paired_at: "now".into(),
        last_seen: None,
        online: false,
        is_primary: false,
        access_level: crate::mesh::types::AccessLevel::Full,
        version: None,
        web_ui: None,
        relay_protocol_version: crate::mesh::types::RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    };
    mesh.paired_peers.insert("peer-a".into(), peer.clone());

    // Genuinely unknown peer: no paired record at all.
    let unpaired = match resolve_relay_peer(&mesh, "no-such-peer") {
        Ok(_) => panic!("unknown peer unexpectedly resolved"),
        Err(error) => error,
    };
    assert_eq!(unpaired.0, StatusCode::NOT_FOUND);
    assert!(
        unpaired.1.starts_with(PEER_NOT_PAIRED_REASON),
        "got: {}",
        unpaired.1
    );

    // Known, paired peer — but no reachable address, so client_for returns None.
    let unroutable = match resolve_relay_peer(&mesh, "peer-a") {
        Ok(_) => panic!("addressless peer unexpectedly resolved"),
        Err(error) => error,
    };
    assert_eq!(unroutable.0, StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        unroutable.1.starts_with(PEER_UNROUTABLE_REASON),
        "got: {}",
        unroutable.1
    );
    assert_ne!(
        unpaired.0, unroutable.0,
        "distinct status codes for distinct causes"
    );
    assert!(
        !unroutable.1.contains(PEER_NOT_PAIRED_REASON),
        "a paired-but-unroutable peer must never be mislabeled 'not paired': {}",
        unroutable.1
    );

    // Give the same paired record a real address — now it resolves, no set-addr needed.
    peer.addresses
        .push(crate::mesh::types::PeerAddress::from_ip("100.65.0.30"));
    mesh.paired_peers.insert("peer-a".into(), peer);
    assert!(resolve_relay_peer(&mesh, "peer-a").is_ok());
}

mod notification_provenance {
    use super::*;

    #[test]
    fn relayed_notification_cannot_claim_a_local_tab() {
        let mut body = serde_json::json!({
            "title": "Remote build finished",
            "body": "artifacts ready",
            "tab_id": "foreign-tab",
            "workspace_id": "foreign-workspace",
            "source_peer": "IMPERSONATED",
        });

        tag_relay_notification_source(&mut body, "POST", "/api/notifications", "BLACKSTAR");

        assert!(body.get("tab_id").is_none());
        assert!(body.get("workspace_id").is_none());
        assert_eq!(body["source_peer"], "BLACKSTAR");
        assert_eq!(body["title"], "Remote build finished");
        assert_eq!(body["body"], "artifacts ready");
    }

    #[test]
    fn only_notification_creation_is_rewritten() {
        let claim = serde_json::json!({"tab_id": "local-tab", "title": "kept"});
        for (method, path) in [
            ("GET", "/api/notifications"),
            ("POST", "/api/notifications/clear"),
            ("POST", "/api/tabs/local-tab/send"),
        ] {
            let mut body = claim.clone();
            tag_relay_notification_source(&mut body, method, path, "BLACKSTAR");
            assert_eq!(body, claim, "{method} {path}");
        }
    }
}

/// The relay's own loopback deadline must outlive the deadline of the route it
/// calls, or it discards a classified delivery answer and reports a bare
/// transport error instead. It must also stay inside the mesh client's relay
/// budget, or the sender gives up before the peer's answer arrives.
#[test]
fn a_send_is_never_abandoned_before_the_delivery_route_can_answer() {
    // `/api/tabs/{id}/send` waits on the target's own readiness for this long.
    const ROUTE_READINESS_WAIT: std::time::Duration = std::time::Duration::from_secs(90);
    // `codeterm-mesh` transport RELAY_TIMEOUT — the sender's whole-hop budget.
    const MESH_RELAY_BUDGET: std::time::Duration = std::time::Duration::from_secs(120);

    let send = local_forward_deadline("POST", "/api/tabs/e35319d7/send");
    assert!(
        send > ROUTE_READINESS_WAIT,
        "the relay must outlast the delivery route's own wait, else its answer is thrown away"
    );
    assert!(
        send < MESH_RELAY_BUDGET,
        "the peer must answer inside the sender's hop budget"
    );
    assert!(
        local_forward_deadline("GET", "/api/tabs") < send,
        "reads keep the short failure deadline"
    );
}

#[test]
fn an_abandoned_forward_is_unknown_and_an_unreachable_one_wrote_nothing() {
    let abandoned = forward_failure_response(true, "deadline".into());
    let error = abandoned
        .data
        .as_ref()
        .and_then(|d| d.get("error"))
        .expect("error object");
    assert!(!abandoned.ok);
    assert_eq!(
        error.get("code").and_then(|v| v.as_str()),
        Some(codeterm_types::mesh::delivery::RELAY_DELIVERY_DEADLINE_EXCEEDED)
    );
    assert!(
        error.get("bytes_written").is_none(),
        "the route may still be writing — claiming zero bytes would invite a duplicate"
    );
    assert!(
        outcome_holds_dedupe(&abandoned),
        "an in-doubt outcome stays deduped so a retry cannot double-deliver"
    );

    let unreachable = forward_failure_response(false, "connection refused".into());
    let error = unreachable
        .data
        .as_ref()
        .and_then(|d| d.get("error"))
        .expect("error object");
    assert_eq!(
        error.get("code").and_then(|v| v.as_str()),
        Some(codeterm_types::mesh::delivery::PEER_LOCAL_API_UNREACHABLE)
    );
    assert_eq!(
        error.get("bytes_written").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert!(
        !outcome_holds_dedupe(&unreachable),
        "nothing was written — the safe retry must not be swallowed"
    );
}

#[test]
fn locally_generated_relay_refusals_carry_a_code_not_a_bare_status() {
    let cases = [
        (
            saturated_response(),
            codeterm_types::mesh::delivery::MESH_DELIVERY_WINDOW_SATURATED,
        ),
        (
            relay_failure(
                403,
                codeterm_types::mesh::delivery::PEER_RELAY_NOT_AUTHORIZED,
                Some(false),
                "not authorized".into(),
            ),
            codeterm_types::mesh::delivery::PEER_RELAY_NOT_AUTHORIZED,
        ),
        (
            relay_failure(
                426,
                codeterm_types::mesh::delivery::PEER_RELAY_PROTOCOL_MISMATCH,
                Some(false),
                "protocol mismatch".into(),
            ),
            codeterm_types::mesh::delivery::PEER_RELAY_PROTOCOL_MISMATCH,
        ),
    ];
    for (response, code) in cases {
        assert!(!response.ok);
        assert_eq!(
            response
                .data
                .as_ref()
                .and_then(|d| d.get("error"))
                .and_then(|e| e.get("code"))
                .and_then(|v| v.as_str()),
            Some(code)
        );
        assert!(
            response
                .error
                .as_deref()
                .is_some_and(|e| e.starts_with(code)),
            "the forwarded summary leads with the code, never a bare HTTP status"
        );
    }
}

/// The sender hop's own transport failure — connection refused before any
/// bytes left, or a timeout after the peer accepted the connection — must
/// thread through the same `relay_failure()` vocabulary the receiving hop
/// uses, with distinguishable, retry-decidable codes. Exercises the exact
/// composition `mesh_relay_to_peer` performs on a `send_relay` error, driven
/// against real sockets so the reqwest classification itself is under test,
/// not a fabricated error.
///
/// The timeout half genuinely waits out `send_relay`'s real ~120s
/// `RELAY_TIMEOUT` (transport.rs) rather than a shorter stand-in: an earlier
/// version of this test held the peer connection open for only 10s and then
/// dropped it, which produced a connection *reset*, not a timeout — a
/// different `reqwest::Error` kind that happened to reach the same
/// conclusion by accident (`is_connect()` false) without ever exercising the
/// bound production actually uses. This test is intentionally slow.
#[tokio::test]
async fn sender_transport_failure_reports_the_same_vocabulary_as_the_receiver() {
    let probe_cmd = crate::mesh::types::RelayCommand {
        method: "GET".into(),
        path: "/api/mesh/status".into(),
        body: serde_json::Value::Null,
        delivery_id: None,
    };

    // Port 1 (TCPMUX) never has a listener — a synchronous refusal, no
    // bind/drop race for a free ephemeral port.
    let refused_client = crate::mesh::transport::PeerClient::new_unauthenticated("127.0.0.1", 1);
    let refused_err = refused_client
        .send_relay(&probe_cmd)
        .await
        .expect_err("nothing listens on 127.0.0.1:1");
    let nothing_sent = relay_failure(
        refused_err.http_status(),
        refused_err.code(),
        refused_err.bytes_written(),
        refused_err.to_string(),
    );

    // Accepts the connection then holds it open FOREVER — only
    // `send_relay`'s own governing timeout can end the wait.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        if let Ok((_socket, _)) = listener.accept().await {
            // Never drop `_socket`: closing it would race a connection reset
            // against the real timeout this test exists to prove.
            std::future::pending::<()>().await;
        }
    });
    let stalled_client = crate::mesh::transport::PeerClient::new_unauthenticated(
        &addr.ip().to_string(),
        addr.port(),
    );
    let stalled_err = stalled_client
        .send_relay(&probe_cmd)
        .await
        .expect_err("the peer never answers, so send_relay's own RELAY_TIMEOUT must fire");
    let submit_unconfirmed = relay_failure(
        stalled_err.http_status(),
        stalled_err.code(),
        stalled_err.bytes_written(),
        stalled_err.to_string(),
    );

    for (response, code, bytes_written) in [
        (
            &nothing_sent,
            codeterm_types::mesh::delivery::MESH_TRANSPORT_NOTHING_SENT,
            Some(false),
        ),
        (
            &submit_unconfirmed,
            codeterm_types::mesh::delivery::MESH_TRANSPORT_SUBMIT_UNCONFIRMED,
            None,
        ),
    ] {
        assert!(!response.ok);
        let error = response
            .data
            .as_ref()
            .and_then(|d| d.get("error"))
            .expect("error object");
        assert_eq!(error.get("code").and_then(|v| v.as_str()), Some(code));
        assert_eq!(
            error.get("bytes_written").and_then(|v| v.as_bool()),
            bytes_written
        );
        assert!(
            response
                .error
                .as_deref()
                .is_some_and(|e| e.starts_with(code)),
            "leads with the code, never a bare HTTP status: {:?}",
            response.error
        );
    }

    assert_ne!(
        nothing_sent
            .data
            .as_ref()
            .and_then(|d| d.get("error"))
            .and_then(|e| e.get("code")),
        submit_unconfirmed
            .data
            .as_ref()
            .and_then(|d| d.get("error"))
            .and_then(|e| e.get("code")),
        "nothing-landed and landed-unknown must remain distinguishable so a retry decision is decidable"
    );
}

/// Regression guard: the receiving hop's own classification is out of scope
/// for this change and pinned by the tests above/below it — this asserts it
/// still leads with the delivery code constant, not a bare status, unchanged
/// by the sender hop gaining its own classification.
#[test]
fn receiving_classification_still_leads_with_the_delivery_code() {
    use codeterm_types::mesh::delivery;

    let submit_unconfirmed = crate::service::delivery_error::DeliveryError::SubmitUnconfirmed {
        tab: "53854ac8".into(),
        detail: "x".into(),
    }
    .to_json();
    assert!(relay_error_summary(502, &submit_unconfirmed).starts_with(delivery::SUBMIT_UNCONFIRMED));

    let tab_busy = crate::service::delivery_error::DeliveryError::TabBusyRefused {
        tab: "53854ac8".into(),
        detail: "x".into(),
    }
    .to_json();
    assert!(relay_error_summary(409, &tab_busy).starts_with(delivery::TAB_BUSY_REFUSED));

    let tab_unreachable = crate::service::delivery_error::DeliveryError::TabNotReachableOnPeer {
        tab: "ghost".into(),
    }
    .to_json();
    assert!(
        relay_error_summary(404, &tab_unreachable).starts_with(delivery::TAB_NOT_REACHABLE_ON_PEER)
    );
}

/// The shared constants and the delivery taxonomy are one contract; a rename on
/// either side must fail here rather than silently stop matching on the wire.
#[test]
fn shared_delivery_constants_match_the_delivery_error_taxonomy() {
    use crate::service::delivery_error::DeliveryError;
    use codeterm_types::mesh::delivery;

    let tab = || "e35319d7".to_string();
    let detail = || String::new();
    assert_eq!(
        DeliveryError::TabNotReachableOnPeer { tab: tab() }.code(),
        delivery::TAB_NOT_REACHABLE_ON_PEER
    );
    assert_eq!(
        DeliveryError::TabBusyRefused {
            tab: tab(),
            detail: detail()
        }
        .code(),
        delivery::TAB_BUSY_REFUSED
    );
    assert_eq!(
        DeliveryError::PtyUnavailable {
            tab: tab(),
            detail: detail()
        }
        .code(),
        delivery::PTY_UNAVAILABLE
    );
    assert_eq!(
        DeliveryError::SubmitUnconfirmed {
            tab: tab(),
            detail: detail()
        }
        .code(),
        delivery::SUBMIT_UNCONFIRMED
    );
}
