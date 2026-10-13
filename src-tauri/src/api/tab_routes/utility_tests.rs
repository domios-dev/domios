use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::ShellActivity;
use crate::service::test_fixture::ServiceFixture;
use std::sync::Arc;
use tauri::Manager;

fn app_state(fixture: &ServiceFixture) -> AppState {
    let mut config = crate::config::schema::AppConfig::default();
    config.utility_tabs.max_per_team.build = Some(1);
    let config = Arc::new(parking_lot::Mutex::new(config));
    assert!(fixture._app.manage(config.clone()));
    let mut mesh = crate::mesh::MeshManager::new("utility-tabs-test".into(), 0);
    mesh.disk_persist = false;
    AppState {
        svc: Arc::new(fixture.service.clone()),
        cron: Arc::new(parking_lot::Mutex::new(
            crate::cron::store::CronStore::new_with_path(fixture.temp.path().join("crons.json")),
        )),
        cron_wake: Arc::new(std::sync::Condvar::new()),
        api_port: 0,
        update_state: Arc::new(parking_lot::Mutex::new(crate::update::UpdateState {
            install_method: crate::update::InstallMethod::Unknown,
            last_check: None,
            last_check_time: None,
            channel: crate::update::Channel::default(),
            phase: crate::update::UpdatePhase::Idle,
            last_error: None,
            phase_changed_at: None,
            downloaded_bytes: 0,
            download_total_bytes: None,
        })),
        config,
        mesh: Arc::new(parking_lot::Mutex::new(mesh)),
        keepawake: Arc::new(parking_lot::Mutex::new(None)),
        resize_owner: Arc::new(parking_lot::Mutex::new(Default::default())),
        rest_api: Arc::new(crate::startup::RestApiControl::new()),
        test_clock: None,
        events: fixture.service.events.clone(),
        web_dist: None,
        snapshot_bound: crate::api::snapshot_bound::SnapshotBound::default(),
    }
}

#[tokio::test]
async fn utility_tabs_api_description_icon_reuse_and_busy_refusal() {
    let (mut mux, owner) = MultiplexerState::new();
    mux.set_orchestrator_role(&owner).unwrap();
    let team = mux.resolve_or_mint_team_for_parent(&owner).unwrap().team_id;
    let workspace = mux.find_workspace_for_tab(&owner).unwrap();
    let fixture = ServiceFixture::new(mux);
    let state = app_state(&fixture);
    let body = |description: &str, icon: &str| {
        serde_json::from_value::<CreateShellTabBody>(serde_json::json!({
            "workspace_id": workspace, "creator_tab_id": owner, "team_id": team,
            "purpose": "build", "title": "Builder", "description": description, "icon": icon
        }))
        .unwrap()
    };
    let before = state.svc.mux.read().snapshot().tabs.len();
    let invalid = create_shell_tab(
        AxumState(state.clone()),
        HeaderMap::new(),
        Json(body("Invalid", "missing-icon")),
    )
    .await
    .unwrap_err();
    assert_eq!(invalid.0, StatusCode::BAD_REQUEST);
    assert_eq!(state.svc.mux.read().snapshot().tabs.len(), before);
    let first = create_shell_tab(
        AxumState(state.clone()),
        HeaderMap::new(),
        Json(body("First build", "hammer")),
    )
    .await
    .unwrap()
    .0;
    let id = first["tab_id"].as_str().unwrap().to_string();
    assert_eq!(first["description"], "First build");
    {
        let mux = state.svc.mux.read();
        let tab = mux.get_tab(&id).unwrap();
        assert_eq!(tab.description.as_deref(), Some("First build"));
        assert_eq!(tab.icon.as_deref(), Some("hammer"));
        assert_eq!(tab.team_id.as_deref(), Some(team.as_str()));
    }
    for activity in [ShellActivity::Unknown, ShellActivity::Running] {
        state
            .svc
            .mux
            .write()
            .get_tab_mut(&id)
            .unwrap()
            .shell_command_state
            .activity = activity;
        let refused = create_shell_tab(
            AxumState(state.clone()),
            HeaderMap::new(),
            Json(body("Busy", "hammer")),
        )
        .await
        .unwrap_err();
        assert_eq!(refused.0, StatusCode::CONFLICT);
        assert_eq!(state.svc.mux.read().snapshot().tabs.len(), before + 1);
    }
    state
        .svc
        .mux
        .write()
        .get_tab_mut(&id)
        .unwrap()
        .shell_command_state
        .activity = ShellActivity::Idle;
    let reused = create_shell_tab(
        AxumState(state.clone()),
        HeaderMap::new(),
        Json(body("Second build", "flask-conical")),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(reused["tab_id"], id);
    assert!(reused["pty_id"].is_string());
    let snapshot = state.svc.mux.read().snapshot();
    assert_eq!(snapshot.tabs.len(), before + 1);
    let tab = snapshot.tabs.get(&id).unwrap();
    assert_eq!(tab.description.as_deref(), Some("Second build"));
    assert_eq!(tab.icon.as_deref(), Some("flask-conical"));
    assert_eq!(tab.title, "Builder");
}
