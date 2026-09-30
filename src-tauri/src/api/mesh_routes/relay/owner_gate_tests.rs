use super::*;
use crate::mesh::types::{AccessLevel, PairedPeer, RelayCommand, RELAY_PROTOCOL_VERSION};

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

fn mux_with_tabs() -> (crate::multiplexer::state::MultiplexerState, Tabs) {
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
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
        version: None,
        web_ui: None,
        relay_protocol_version: RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    }
}

fn app_state(fixture: &crate::service::test_fixture::ServiceFixture) -> AppState {
    let config = crate::config::schema::AppConfig::default();
    let mut mesh = crate::mesh::MeshManager::new("relay-owner-gate".into(), 0);
    mesh.disk_persist = false;
    mesh.paired_peers.clear();
    mesh.paired_peers
        .insert(ISOLATED_PEER.into(), isolated_peer());
    AppState {
        svc: std::sync::Arc::new(fixture.service.clone()),
        cron: std::sync::Arc::new(parking_lot::Mutex::new(
            crate::cron::store::CronStore::new_with_path(
                fixture.temp.path().join("relay-owner-gate-crons.json"),
            ),
        )),
        cron_wake: std::sync::Arc::new(std::sync::Condvar::new()),
        api_port: 0,
        update_state: std::sync::Arc::new(parking_lot::Mutex::new(crate::update::UpdateState {
            install_method: crate::update::InstallMethod::Unknown,
            last_check: None,
            last_check_time: None,
            channel: config.updater.channel,
            phase: crate::update::UpdatePhase::Idle,
            last_error: None,
            phase_changed_at: None,
            downloaded_bytes: 0,
            download_total_bytes: None,
        })),
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

fn relay_command(method: &str, path: String) -> RelayCommand {
    RelayCommand {
        method: method.into(),
        path,
        body: serde_json::json!({ "text": "echo ISOLATION-PROBE" }),
        delivery_id: None,
    }
}

fn refusal_code(response: &crate::mesh::types::RelayResponse) -> Option<&str> {
    response.data.as_ref()?.get("error")?.get("code")?.as_str()
}

const TAB_WRITES: [&str; 5] = ["send", "exec", "respond", "prompt/respond", "attachments"];

#[cfg_attr(not(windows), ignore = "requires the desktop Tauri test runtime")]
#[tokio::test]
async fn relayed_writes_into_a_foreign_tab_are_refused_for_agent_isolated() {
    let (mux, tabs) = mux_with_tabs();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let state = app_state(&fixture);
    for foreign in [&tabs.unowned, &tabs.other_peers] {
        for suffix in TAB_WRITES {
            for query in ["", "?source=probe"] {
                let path = format!("/api/tabs/{foreign}/{suffix}{query}");
                assert!(!crate::api::auth::authorize_peer_request(
                    &state,
                    &AccessLevel::AgentIsolated,
                    ISOLATED_PEER,
                    &Method::POST,
                    &path,
                ));
                let response = perform_relay(&state, ISOLATED_PEER, &relay_command("POST", path))
                    .await
                    .expect("refusal is a relay response");
                assert!(!response.ok, "{foreign} {suffix}{query}");
                assert_eq!(response.status, StatusCode::FORBIDDEN.as_u16());
                assert_eq!(
                    refusal_code(&response),
                    Some(codeterm_types::mesh::delivery::PEER_RELAY_NOT_AUTHORIZED)
                );
            }
        }
        let text = format!("/api/tabs/{foreign}/text?formatted=false");
        let response = perform_relay(&state, ISOLATED_PEER, &relay_command("GET", text))
            .await
            .expect("refusal is a relay response");
        assert_eq!(response.status, StatusCode::FORBIDDEN.as_u16());
    }
}

#[cfg_attr(not(windows), ignore = "requires the desktop Tauri test runtime")]
#[tokio::test]
async fn relayed_writes_into_an_owned_tab_pass_the_shared_owner_check() {
    let (mux, tabs) = mux_with_tabs();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let state = app_state(&fixture);
    let owner = |tab_id: &str| crate::api::auth::tab_owner_peer_id(&state, tab_id);
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
            assert!(crate::api::auth::authorize_peer_request(
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

#[cfg_attr(not(windows), ignore = "requires the desktop Tauri test runtime")]
#[tokio::test]
async fn dot_segments_cannot_retarget_an_authorized_relay() {
    let (mux, tabs) = mux_with_tabs();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let state = app_state(&fixture);
    let (owned, foreign) = (&tabs.owned, &tabs.unowned);
    for path in [
        format!("/api/tabs/{owned}/../{foreign}/send"),
        format!("/api/tabs/{owned}/%2e%2E/{foreign}/send"),
        format!("/api/tabs/{owned}\\..\\{foreign}/send"),
    ] {
        let refused = perform_relay(&state, ISOLATED_PEER, &relay_command("POST", path)).await;
        let (status, _) = refused.expect_err("dot segments are refused before dispatch");
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}

#[cfg_attr(not(windows), ignore = "requires the desktop Tauri test runtime")]
#[test]
fn relayed_isolated_listing_keeps_only_owned_rows() {
    let (mux, tabs) = mux_with_tabs();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let state = app_state(&fixture);
    let mut listing = serde_json::json!([
        { "id": tabs.owned, "other_workspace_tab_count": 3 },
        { "id": tabs.unowned },
        { "id": tabs.other_peers },
        { "id": "remote-peer:tab" },
    ]);
    crate::api::tab_routes::retain_peer_owned_tab_rows(&state, ISOLATED_PEER, &mut listing);
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
            &crate::mesh::types::AccessLevel::Full,
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
