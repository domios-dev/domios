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
            authorize_peer_relay("signed-peer", &access, &method, path),
            PeerRelayAuthorization::WorkspaceClose {
                peer_id: "signed-peer",
                workspace_id: "ws_remote",
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
            authorize_peer_relay("signed-peer", &access, &method, path),
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
    );

    assert_eq!(
        authorization,
        PeerRelayAuthorization::WorkspaceClose {
            peer_id: "cryptographically-verified-peer",
            workspace_id: "ws_remote",
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
            authorize_peer_relay("signed-peer", &access, &method, path),
            PeerRelayAuthorization::Forward,
            "{method} {path}"
        );
    }
}

#[test]
fn peer_relay_resolves_id_or_case_insensitive_name() {
    let peers = [("peer-a", "BLACKSTAR"), ("peer-b", "ROG")];
    assert_eq!(
        resolve_paired_peer_id("peer-a", peers),
        Some("peer-a".to_string())
    );
    assert_eq!(
        resolve_paired_peer_id("blackstar", peers),
        Some("peer-a".to_string())
    );
    assert_eq!(resolve_paired_peer_id("missing", peers), None);
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
