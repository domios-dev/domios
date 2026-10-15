use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, Query, State as AxumState};
use axum::http::HeaderMap;

use super::off_runtime_gate::StorageCron;
use crate::routes::AppState;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::test_fixture::ServiceFixture;

fn app_state(fixture: &ServiceFixture, cron: &StorageCron) -> AppState {
    let config = codeterm_service_core::config::schema::AppConfig::default();
    let mut mesh =
        codeterm_service_core::mesh::MeshManager::new("cron-entry-off-runtime".into(), 0);
    mesh.disk_persist = false;
    AppState {
        svc: Arc::new(fixture.service.clone()),
        cron: cron.cron.clone(),
        cron_wake: cron.wake.clone(),
        api_port: 0,
        update_state: Arc::new(parking_lot::Mutex::new(
            codeterm_commands::update::UpdateState {
                install_method: codeterm_commands::update::InstallMethod::Unknown,
                last_check: None,
                last_check_time: None,
                channel: config.updater.channel,
                phase: codeterm_commands::update::UpdatePhase::Idle,
                last_error: None,
                phase_changed_at: None,
                downloaded_bytes: 0,
                download_total_bytes: None,
            },
        )),
        config: Arc::new(parking_lot::Mutex::new(config)),
        mesh: Arc::new(parking_lot::Mutex::new(mesh)),
        keepawake: Arc::new(parking_lot::Mutex::new(None)),
        resize_owner: Arc::new(parking_lot::Mutex::new(Default::default())),
        rest_api: Arc::new(codeterm_service_core::startup::RestApiControl::new()),
        test_clock: None,
        events: fixture.service.events.clone(),
        web_dist: None,
    }
}

#[test]
fn storage_cutover_cron_unavailable_read_maps_to_service_unavailable() {
    assert_eq!(
        super::cron_read_error("cron storage unavailable: database unavailable".into()).0,
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn storage_cutover_unavailable_desk_rest_reads_return_503_with_the_preservation_message() {
    use codeterm_service_core::startup::{StorageAuthority, STORAGE_UNAVAILABLE_MESSAGE};
    use tauri::Manager;

    let fixture = ServiceFixture::new(MultiplexerState::empty());
    assert!(fixture._app.manage(StorageAuthority::Unavailable {
        storage: None,
        reason: "test storage failure".into(),
    }));
    let storage_cron = StorageCron::open();
    let state = app_state(&fixture, &storage_cron);

    let mux_error = crate::web_routes::mux_state(AxumState(state.clone()), HeaderMap::new())
        .await
        .unwrap_err();
    assert_eq!(mux_error.0, axum::http::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(mux_error.1, STORAGE_UNAVAILABLE_MESSAGE);

    let workspace_error =
        crate::workspace_routes::list_workspaces(AxumState(state.clone()), HeaderMap::new())
            .await
            .unwrap_err();
    assert_eq!(
        workspace_error.0,
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(workspace_error.1, STORAGE_UNAVAILABLE_MESSAGE);

    let tab_error = crate::tab_routes::list_tabs(
        AxumState(state),
        HeaderMap::new(),
        Query(crate::tab_routes::TabListQuery {
            workspace_id: None,
            focused_workspace: false,
            all_workspaces: false,
            agents: false,
            local_only: true,
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(tab_error.0, axum::http::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(tab_error.1, STORAGE_UNAVAILABLE_MESSAGE);
}

/// One runtime thread: the parked disable may hold it only if the route blocks inline.
#[tokio::test]
async fn cron_entry_off_runtime_route_disable_parks_commit_while_unrelated_routes_complete() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let storage_cron = StorageCron::open();
    let held = storage_cron.create("held", "ws-held");
    let other = storage_cron.create("unrelated", "ws-other");
    let state = app_state(&fixture, &storage_cron);
    let mut gate = storage_cron.gate_next_commit();

    let disable = tokio::spawn(super::cron_disable(
        AxumState(state.clone()),
        HeaderMap::new(),
        Path(held.id.clone()),
    ));
    gate.parked().await;

    let listed = super::cron_list(
        AxumState(state.clone()),
        HeaderMap::new(),
        Query(HashMap::from([(
            "workspace_id".to_string(),
            "ws-held".to_string(),
        )])),
    )
    .await
    .expect("list completes while the commit is parked");
    assert_eq!(listed.0[0]["id"], held.id.as_str());
    assert_eq!(
        listed.0[0]["enabled"], true,
        "the parked commit is not yet published"
    );
    let fetched = super::cron_get(
        AxumState(state.clone()),
        HeaderMap::new(),
        Path(other.id.clone()),
    )
    .await
    .expect("get completes while the commit is parked");
    assert_eq!(fetched.0["id"], other.id.as_str());

    assert!(gate.release(), "the commit stayed parked until released");
    let disabled = disable
        .await
        .expect("disable route task")
        .expect("disable route");
    assert_eq!(disabled.0["enabled"], false);
    assert_eq!(storage_cron.enabled(&held.id), Some(false));
    assert_eq!(storage_cron.enabled(&other.id), Some(true));
}
