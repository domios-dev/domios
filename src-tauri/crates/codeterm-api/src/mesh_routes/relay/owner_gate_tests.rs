use super::*;
use codeterm_service_core::mesh::types::{
    AccessLevel, PairedPeer, RelayCommand, RelayResponse, RELAY_PROTOCOL_VERSION,
};

const ISOLATED_PEER: &str = "peer-isolated";

struct Tabs {
    owned: String,
    unowned: String,
    other_peers: String,
}

fn owned_by(peer_id: &str) -> codeterm_types::pty::RemoteOwnership {
    codeterm_types::pty::RemoteOwnership {
        controller_title: "remote".into(),
        controller_tab_id: None,
        peer_id: peer_id.into(),
        peer_name: peer_id.into(),
        team: None,
    }
}

fn mux_with_tabs() -> (
    codeterm_service_core::multiplexer::state::MultiplexerState,
    Tabs,
) {
    let (mut mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.snapshot().workspaces[0].id.clone();
    let unowned = mux.snapshot().workspaces[0].tabs[0].clone();
    let owned = mux.create_tab(&workspace_id).unwrap();
    let other_peers = mux.create_tab(&workspace_id).unwrap();
    mux.get_tab_mut(&owned).unwrap().ownership = Some(owned_by(ISOLATED_PEER));
    mux.get_tab_mut(&other_peers).unwrap().ownership = Some(owned_by("peer-other"));
    (
        mux,
        Tabs {
            owned,
            unowned,
            other_peers,
        },
    )
}

fn isolated_peer() -> PairedPeer {
    PairedPeer {
        instance_id: ISOLATED_PEER.into(),
        instance_name: "Isolated".into(),
        addresses: Vec::new(),
        api_port: 0,
        public_key: String::new(),
        paired_at: "now".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level: AccessLevel::AgentIsolated,
        domios_version: None,
        domios_build: None,
        web_ui: None,
        relay_protocol_version: RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    }
}

fn app_state(fixture: &codeterm_service_core::service::test_fixture::ServiceFixture) -> AppState {
    let config = codeterm_service_core::config::schema::AppConfig::default();
    let mut mesh = codeterm_service_core::mesh::MeshManager::new("relay-owner-gate".into(), 0);
    mesh.disk_persist = false;
    mesh.paired_peers.clear();
    mesh.insert_account_peer_for_test(isolated_peer());
    AppState {
        frontend_dev_url: None,
        svc: std::sync::Arc::new(fixture.service.clone()),
        cron: std::sync::Arc::new(parking_lot::Mutex::new(
            codeterm_service_core::cron::store::CronStore::new_with_path(
                fixture.temp.path().join("relay-owner-gate-crons.json"),
            ),
        )),
        cron_wake: Default::default(),
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

#[test]
fn relay_fences_a_closed_peer_before_forwarding() {
    let (mux, _) = mux_with_tabs();
    let fixture = codeterm_neighbors_service::test_support::fixture(mux);
    let state = app_state(&fixture);
    assert!(state.mesh.lock().traffic_open(ISOLATED_PEER));

    let completed = RelayResponse {
        ok: true,
        status: StatusCode::OK.as_u16(),
        error: None,
        data: Some(serde_json::json!({"protected": "mesh data"})),
    };
    state.mesh.lock().remove_account_peer(ISOLATED_PEER);

    assert!(relay_prewrite_refusal(&state, ISOLATED_PEER).is_some());
    assert!(
        completed.ok,
        "a completed PTY result is never replaced by a later gate change"
    );
    assert!(
        outcome_holds_dedupe(&completed),
        "the written result remains deduped"
    );
    assert_eq!(completed.data.unwrap()["protected"], "mesh data");
}

fn relay_command(method: &str, path: String) -> RelayCommand {
    RelayCommand {
        method: method.into(),
        path,
        body: serde_json::json!({ "text": "echo ISOLATION-PROBE" }),
        delivery_id: None,
    }
}

fn refusal_code(response: &codeterm_service_core::mesh::types::RelayResponse) -> Option<&str> {
    response.data.as_ref()?.get("error")?.get("code")?.as_str()
}

const TAB_WRITES: [&str; 5] = ["send", "exec", "respond", "prompt/respond", "attachments"];

#[tokio::test]
async fn relayed_writes_into_a_foreign_tab_are_refused_for_agent_isolated() {
    let (mux, tabs) = mux_with_tabs();
    let fixture = codeterm_neighbors_service::test_support::fixture(mux);
    let state = app_state(&fixture);
    for foreign in [&tabs.unowned, &tabs.other_peers] {
        for suffix in TAB_WRITES {
            for query in ["", "?source=probe"] {
                let path = format!("/api/tabs/{foreign}/{suffix}{query}");
                assert!(!crate::auth::authorize_peer_request(
                    &state,
                    &AccessLevel::AgentIsolated,
                    ISOLATED_PEER,
                    &Method::POST,
                    &path,
                ));
                let response = perform_test_relay(
                    &state,
                    ISOLATED_PEER,
                    &relay_command("POST", path),
                    crate::auth::AuthenticatedOrigin::Mesh,
                    None,
                    None,
                )
                .await
                .expect("refusal is a relay response");
                assert!(!response.ok, "{foreign} {suffix}{query}");
                assert_eq!(response.status, StatusCode::FORBIDDEN.as_u16());
                assert_eq!(
                    refusal_code(&response),
                    Some(codeterm_mesh::contract::delivery::PEER_RELAY_NOT_AUTHORIZED)
                );
            }
        }
        let text = format!("/api/tabs/{foreign}/text?formatted=false");
        let response = perform_test_relay(
            &state,
            ISOLATED_PEER,
            &relay_command("GET", text),
            crate::auth::AuthenticatedOrigin::Mesh,
            None,
            None,
        )
        .await
        .expect("refusal is a relay response");
        assert_eq!(response.status, StatusCode::FORBIDDEN.as_u16());
    }
}

#[tokio::test]
async fn relayed_writes_into_an_owned_tab_pass_the_shared_owner_check() {
    let (mux, tabs) = mux_with_tabs();
    let fixture = codeterm_neighbors_service::test_support::fixture(mux);
    let state = app_state(&fixture);
    let owner = |tab_id: &str| crate::auth::tab_owner_peer_id(&state, tab_id);
    for suffix in TAB_WRITES {
        for query in ["", "?source=probe"] {
            let path = format!("/api/tabs/{}/{suffix}{query}", tabs.owned);
            assert_eq!(
                authorize_peer_relay(
                    ISOLATED_PEER,
                    &AccessLevel::AgentIsolated,
                    &Method::POST,
                    &path,
                    &serde_json::Value::Null,
                    owner,
                ),
                PeerRelayAuthorization::Forward,
                "{path}"
            );
            assert!(crate::auth::authorize_peer_request(
                &state,
                &AccessLevel::AgentIsolated,
                ISOLATED_PEER,
                &Method::POST,
                &path,
            ));
        }
    }
    for path in ["/api/tabs?agents=false", "/api/tabs"] {
        assert_eq!(
            authorize_peer_relay(
                ISOLATED_PEER,
                &AccessLevel::AgentIsolated,
                &Method::GET,
                path,
                &serde_json::Value::Null,
                owner,
            ),
            PeerRelayAuthorization::Forward,
            "{path}"
        );
    }
}

#[tokio::test]
async fn dot_segments_cannot_retarget_an_authorized_relay() {
    let (mux, tabs) = mux_with_tabs();
    let fixture = codeterm_neighbors_service::test_support::fixture(mux);
    let state = app_state(&fixture);
    let (owned, foreign) = (&tabs.owned, &tabs.unowned);
    for path in [
        format!("/api/tabs/{owned}/../{foreign}/send"),
        format!("/api/tabs/{owned}/%2e%2E/{foreign}/send"),
        format!("/api/tabs/{owned}\\..\\{foreign}/send"),
    ] {
        let refused = perform_test_relay(
            &state,
            ISOLATED_PEER,
            &relay_command("POST", path),
            crate::auth::AuthenticatedOrigin::Mesh,
            None,
            None,
        )
        .await;
        let (status, _) = refused.expect_err("dot segments are refused before dispatch");
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}

#[test]
fn relayed_isolated_listing_keeps_only_owned_rows() {
    let (mux, tabs) = mux_with_tabs();
    let fixture = codeterm_neighbors_service::test_support::fixture(mux);
    let state = app_state(&fixture);
    let mut listing = serde_json::json!([
        { "id": tabs.owned, "other_workspace_tab_count": 3 },
        { "id": tabs.unowned },
        { "id": tabs.other_peers },
        { "id": "remote-peer:tab" },
    ]);
    crate::tab_routes::retain_peer_owned_tab_rows(&state, ISOLATED_PEER, &mut listing);
    assert_eq!(listing, serde_json::json!([{ "id": tabs.owned }]));
}

#[test]
fn relayed_spawn_without_origin_ownership_records_the_signed_peer() {
    let mut body = serde_json::json!({ "agent": "claude" });
    tag_relay_spawn_source(
        &mut body,
        "POST",
        "/api/agents/spawn",
        ISOLATED_PEER,
        "Isolated",
    );
    let ownership: codeterm_types::pty::RemoteOwnership =
        serde_json::from_value(body["ownership"].clone()).expect("spawn body carries ownership");
    assert_eq!(ownership.peer_id, ISOLATED_PEER);
    assert_eq!(ownership.peer_name, "Isolated");

    let mut malformed = serde_json::json!({ "agent": "claude", "ownership": "peer-other" });
    tag_relay_spawn_source(
        &mut malformed,
        "POST",
        "/api/agents/spawn?workspace_id=w",
        ISOLATED_PEER,
        "Isolated",
    );
    assert_eq!(
        malformed["ownership"]["peer_id"],
        serde_json::json!(ISOLATED_PEER)
    );
}

#[test]
fn a_query_string_cannot_route_workspace_close_around_peer_provenance() {
    assert_eq!(
        authorize_peer_relay(
            "signed-peer",
            &codeterm_service_core::mesh::types::AccessLevel::Full,
            &Method::POST,
            "/api/workspaces/ws_remote/close?caller_tab_id=x",
            &serde_json::Value::Null,
            |_| None
        ),
        PeerRelayAuthorization::WorkspaceClose {
            peer_id: "signed-peer",
            workspace_id: "ws_remote",
            force: false,
        }
    );
}

#[test]
fn relay_paths_with_dot_segments_or_backslashes_are_rejected() {
    for path in [
        "/api/tabs/a/../b/send",
        "/api/tabs/a/./send",
        "/api/tabs/a/%2e%2E/b/send",
        "/api/tabs/a/.%2e/b/send",
        r"/api/tabs/a\b/send",
    ] {
        assert!(validate_relay_path(path).is_err(), "{path}");
    }
    assert!(validate_relay_path("/api/tabs/a/send?note=../x").is_ok());
    assert!(validate_relay_path("/api/tabs/a.b/send").is_ok());
}

#[tokio::test]
async fn workspace_close_preview_relay_enforces_owner_origin_and_workspace_authority_without_mutation(
) {
    use codeterm_service_core::mesh::types::Permission;

    for path in [
        "/api/workspaces/ws_remote/close-preview",
        "/api/workspaces/ws_remote/close-preview?refresh=1",
    ] {
        for access in [
            AccessLevel::Full,
            AccessLevel::Custom(vec![Permission::ManageWorkspaces]),
        ] {
            assert_eq!(
                authorize_peer_relay(
                    "signed-peer",
                    &access,
                    &Method::GET,
                    path,
                    &serde_json::Value::Null,
                    |_| None,
                ),
                PeerRelayAuthorization::WorkspaceClosePreview {
                    peer_id: "signed-peer",
                    workspace_id: "ws_remote",
                },
                "{access:?} {path}"
            );
        }
        for access in [
            AccessLevel::ReadOnly,
            AccessLevel::AgentIsolated,
            AccessLevel::Custom(vec![Permission::ReadTabs]),
        ] {
            assert_eq!(
                authorize_peer_relay(
                    "signed-peer",
                    &access,
                    &Method::GET,
                    path,
                    &serde_json::Value::Null,
                    |_| None,
                ),
                PeerRelayAuthorization::Rejected,
                "{access:?} {path}"
            );
        }
    }

    let (mut mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let protected_ws = mux.snapshot().workspaces[0].id.clone();
    mux.get_workspace_mut(&protected_ws).unwrap().protected = true;
    let target_ws = mux.create_workspace("Disposable Remote".into(), None).id;
    let agent_tab = mux.create_tab(&target_ws).unwrap();
    mux.get_tab_mut(&agent_tab).unwrap().agent_type = Some("fixture-provider".into());
    mux.get_tab_mut(&agent_tab).unwrap().title = "WorkerOne".into();

    let fixture = codeterm_neighbors_service::test_support::fixture(mux);
    let storage =
        codeterm_storage::Storage::open(fixture.temp.path().join("storage"), Default::default())
            .unwrap();
    let store = codeterm_sentinel_host::store::Store::with_storage(
        &storage,
        codeterm_service_core::cron::clock::MockClock::starting_at(1000),
    );
    let handle = codeterm_sentinel_host::scheduler::fixture_handle_with_ui(
        store,
        fixture.service.ui.clone(),
    );
    codeterm_sentinel_host::manage_fixture_ports(&fixture.service, &handle);

    let state = app_state(&fixture);
    let mut full_peer = isolated_peer();
    full_peer.instance_id = "peer-full".into();
    full_peer.instance_name = "Full".into();
    full_peer.access_level = AccessLevel::Full;
    state.mesh.lock().insert_account_peer_for_test(full_peer);

    let cmd = RelayCommand {
        method: "GET".into(),
        path: format!("/api/workspaces/{target_ws}/close-preview"),
        body: serde_json::Value::Null,
        delivery_id: None,
    };

    for origin in [
        crate::auth::AuthenticatedOrigin::Agent,
        crate::auth::AuthenticatedOrigin::Mesh,
    ] {
        let refused = perform_test_relay(&state, "peer-full", &cmd, origin, None, None)
            .await
            .expect_err("non-owner origin must be refused");
        assert_eq!(refused.0, StatusCode::FORBIDDEN);
    }

    let isolated = perform_test_relay(
        &state,
        ISOLATED_PEER,
        &cmd,
        crate::auth::AuthenticatedOrigin::Owner,
        None,
        None,
    )
    .await
    .expect("isolated refusal is a relay response");
    assert!(!isolated.ok);
    assert_eq!(isolated.status, StatusCode::FORBIDDEN.as_u16());

    let expected_installation_id = state.mesh.lock().ident.identity.instance_id.clone();
    let mut before_snap = fixture.service.mux.read().snapshot();
    before_snap.generation = 0;
    let before = serde_json::to_value(before_snap).unwrap();
    let ok = perform_test_relay(
        &state,
        "peer-full",
        &cmd,
        crate::auth::AuthenticatedOrigin::Owner,
        None,
        None,
    )
    .await
    .expect("owner-origin authorized peer preview succeeds");
    assert!(ok.ok);
    assert_eq!(ok.status, StatusCode::OK.as_u16());
    let preview: codeterm_types::workspace_close_preview::WorkspaceClosePreview =
        serde_json::from_value(ok.data.expect("preview payload")).unwrap();
    assert_eq!(preview.installation_id, expected_installation_id);
    assert_eq!(preview.workspace_id, target_ws);
    assert_eq!(preview.workspace_name, "Disposable Remote");
    assert!(!preview.protected);
    assert_eq!(preview.agent_tabs.len(), 1);
    assert_eq!(preview.agent_tabs[0].title, "WorkerOne");
    let mut after_snap = fixture.service.mux.read().snapshot();
    after_snap.generation = 0;
    let after = serde_json::to_value(after_snap).unwrap();
    assert_eq!(
        after, before,
        "close-preview must not mutate multiplexer state"
    );

    let missing = perform_test_relay(
        &state,
        "peer-full",
        &RelayCommand {
            method: "GET".into(),
            path: "/api/workspaces/missing-ws/close-preview".into(),
            body: serde_json::Value::Null,
            delivery_id: None,
        },
        crate::auth::AuthenticatedOrigin::Owner,
        None,
        None,
    )
    .await
    .expect("missing workspace returns structured relay 404");
    assert!(!missing.ok);
    assert_eq!(missing.status, StatusCode::NOT_FOUND.as_u16());

    let protected = perform_test_relay(
        &state,
        "peer-full",
        &RelayCommand {
            method: "GET".into(),
            path: format!("/api/workspaces/{protected_ws}/close-preview"),
            body: serde_json::Value::Null,
            delivery_id: None,
        },
        crate::auth::AuthenticatedOrigin::Owner,
        None,
        None,
    )
    .await
    .expect("protected workspace returns structured relay 409");
    assert!(!protected.ok);
    assert_eq!(protected.status, StatusCode::CONFLICT.as_u16());
}

#[tokio::test]
async fn team_close_relay_authorizes_paired_owner_and_refuses_ungranted_or_unconfirmed_callers() {
    use codeterm_service_core::mesh::types::Permission;

    let valid_body = serde_json::json!({
        "force": true,
        "confirm": "team_remote",
        "force_busy": true,
    });
    for path in [
        "/api/teams/team_remote/close",
        "/api/teams/team_remote/close?caller_tab_id=x",
    ] {
        for access in [
            AccessLevel::Full,
            AccessLevel::Custom(vec![Permission::ManageWorkspaces]),
        ] {
            assert_eq!(
                authorize_peer_relay(
                    "signed-peer",
                    &access,
                    &Method::POST,
                    path,
                    &valid_body,
                    |_| None,
                ),
                PeerRelayAuthorization::TeamClose {
                    peer_id: "signed-peer",
                    team_id: "team_remote",
                    force: true,
                    confirm: Some("team_remote"),
                    force_busy: true,
                },
                "{access:?} {path}"
            );
        }
        for access in [
            AccessLevel::ReadOnly,
            AccessLevel::AgentIsolated,
            AccessLevel::Custom(vec![Permission::ReadTabs]),
        ] {
            assert_eq!(
                authorize_peer_relay(
                    "signed-peer",
                    &access,
                    &Method::POST,
                    path,
                    &valid_body,
                    |_| None,
                ),
                PeerRelayAuthorization::Rejected,
                "{access:?} {path}"
            );
        }
    }

    let (mut mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let protected_ws = mux.snapshot().workspaces[0].id.clone();
    mux.get_workspace_mut(&protected_ws).unwrap().protected = true;
    let protected_ws_team = "team_in_protected_ws".to_string();
    mux.upsert_team(codeterm_types::team::Team {
        id: protected_ws_team.clone(),
        name: Some("Protected WS Team".into()),
        lifecycle: Default::default(),
        created_at: 1000,
        workspace_id: protected_ws.clone(),
        locked: false,
    });

    let target_ws = mux.create_workspace("Remote Teams".into(), None).id;
    let target_team = "team_remote".to_string();
    mux.upsert_team(codeterm_types::team::Team {
        id: target_team.clone(),
        name: Some("Remote Crew".into()),
        lifecycle: Default::default(),
        created_at: 1000,
        workspace_id: target_ws.clone(),
        locked: false,
    });
    let member_tab = mux.create_tab(&target_ws).unwrap();
    mux.get_tab_mut(&member_tab).unwrap().team_id = Some(target_team.clone());

    let protected_tab_team = "team_with_protected_tab".to_string();
    mux.upsert_team(codeterm_types::team::Team {
        id: protected_tab_team.clone(),
        name: Some("Protected Member Team".into()),
        lifecycle: Default::default(),
        created_at: 1000,
        workspace_id: target_ws.clone(),
        locked: false,
    });
    let protected_tab = mux.create_tab(&target_ws).unwrap();
    mux.get_tab_mut(&protected_tab).unwrap().team_id = Some(protected_tab_team.clone());
    mux.get_tab_mut(&protected_tab).unwrap().protected = true;

    let fixture = codeterm_neighbors_service::test_support::fixture(mux);
    let state = app_state(&fixture);
    let mut full_peer = isolated_peer();
    full_peer.instance_id = "peer-full".into();
    full_peer.instance_name = "Full".into();
    full_peer.access_level = AccessLevel::Full;
    state.mesh.lock().insert_account_peer_for_test(full_peer);

    let mut readonly_peer = isolated_peer();
    readonly_peer.instance_id = "peer-readonly".into();
    readonly_peer.instance_name = "ReadOnly".into();
    readonly_peer.access_level = AccessLevel::ReadOnly;
    state
        .mesh
        .lock()
        .insert_account_peer_for_test(readonly_peer);

    let close_cmd = RelayCommand {
        method: "POST".into(),
        path: format!("/api/teams/{target_team}/close"),
        body: valid_body,
        delivery_id: None,
    };

    for caller_peer in ["peer-unpaired", ISOLATED_PEER, "peer-readonly"] {
        let refused = perform_test_relay(
            &state,
            caller_peer,
            &close_cmd,
            crate::auth::AuthenticatedOrigin::Owner,
            None,
            None,
        )
        .await
        .expect("unpaired or ungranted refusal is a relay response");
        assert!(!refused.ok, "{caller_peer}");
        assert_eq!(
            refused.status,
            StatusCode::FORBIDDEN.as_u16(),
            "{caller_peer}"
        );
    }

    for origin in [
        crate::auth::AuthenticatedOrigin::Agent,
        crate::auth::AuthenticatedOrigin::Mesh,
    ] {
        let refused = perform_test_relay(&state, "peer-full", &close_cmd, origin, None, None)
            .await
            .expect_err("non-owner origin must be refused");
        assert_eq!(refused.0, StatusCode::FORBIDDEN);
    }

    for unconfirmed_body in [
        serde_json::json!({ "force": false, "confirm": target_team, "force_busy": true }),
        serde_json::json!({ "force": true, "confirm": "wrong_team", "force_busy": true }),
        serde_json::json!({}),
    ] {
        let refused = perform_test_relay(
            &state,
            "peer-full",
            &RelayCommand {
                method: "POST".into(),
                path: format!("/api/teams/{target_team}/close"),
                body: unconfirmed_body,
                delivery_id: None,
            },
            crate::auth::AuthenticatedOrigin::Owner,
            None,
            None,
        )
        .await
        .expect("unconfirmed close returns structured relay 403");
        assert!(!refused.ok);
        assert_eq!(refused.status, StatusCode::FORBIDDEN.as_u16());
    }

    for protected_team in [&protected_ws_team, &protected_tab_team] {
        let refused = perform_test_relay(
            &state,
            "peer-full",
            &RelayCommand {
                method: "POST".into(),
                path: format!("/api/teams/{protected_team}/close"),
                body: serde_json::json!({
                    "force": true,
                    "confirm": protected_team,
                    "force_busy": true,
                }),
                delivery_id: None,
            },
            crate::auth::AuthenticatedOrigin::Owner,
            None,
            None,
        )
        .await
        .expect("protected Team close returns structured relay 403");
        assert!(!refused.ok, "{protected_team}");
        assert_eq!(
            refused.status,
            StatusCode::FORBIDDEN.as_u16(),
            "{protected_team}"
        );
        assert!(
            fixture
                .service
                .mux
                .read()
                .get_team(protected_team)
                .is_some(),
            "protected Team {protected_team} stays open"
        );
    }

    let missing = perform_test_relay(
        &state,
        "peer-full",
        &RelayCommand {
            method: "POST".into(),
            path: "/api/teams/missing-team/close".into(),
            body: serde_json::json!({
                "force": true,
                "confirm": "missing-team",
                "force_busy": true,
            }),
            delivery_id: None,
        },
        crate::auth::AuthenticatedOrigin::Owner,
        None,
        None,
    )
    .await
    .expect("missing Team returns structured relay 404");
    assert!(!missing.ok);
    assert_eq!(missing.status, StatusCode::NOT_FOUND.as_u16());

    assert!(
        fixture.service.mux.read().get_team(&target_team).is_some(),
        "Team stays open until an authorized confirmed owner call"
    );

    let ok = perform_test_relay(
        &state,
        "peer-full",
        &close_cmd,
        crate::auth::AuthenticatedOrigin::Owner,
        None,
        None,
    )
    .await
    .expect("authorized paired owner Team close succeeds");
    assert!(ok.ok, "expected ok relay response, got: {ok:?}");
    assert_eq!(ok.status, StatusCode::OK.as_u16());
    let data = ok.data.expect("team close payload");
    assert_eq!(data["team"]["id"], serde_json::json!(target_team));
    assert!(data.get("inventory").is_some(), "payload carries inventory");
    let mux = fixture.service.mux.read();
    assert!(
        mux.get_team(&target_team).is_none(),
        "closed Team is removed"
    );
    assert!(
        mux.get_tab(&member_tab).is_none(),
        "closed Team member tab is removed"
    );
}
