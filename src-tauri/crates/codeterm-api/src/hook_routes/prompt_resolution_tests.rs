use std::sync::Arc;

use crate::routes::AppState;
use codeterm_service_core::service::prompt_resolution_tests::{
    assert_resolved_and_new_prompt_notifies, pending_prompt_fixture,
};

fn app_state(fixture: &codeterm_service_core::service::test_fixture::ServiceFixture) -> AppState {
    let config = codeterm_service_core::config::schema::AppConfig::default();
    AppState {
        svc: Arc::new(fixture.service.clone()),
        cron: Arc::new(parking_lot::Mutex::new(
            codeterm_service_core::cron::store::CronStore::new_with_path(
                fixture.temp.path().join("prompt-resolution-crons.json"),
            ),
        )),
        cron_wake: Arc::new(std::sync::Condvar::new()),
        api_port: 0,
        update_state: Arc::new(parking_lot::Mutex::new(
            codeterm_service_core::update_state::UpdateState {
                install_method: codeterm_service_core::update_state::InstallMethod::Unknown,
                last_check: None,
                last_check_time: None,
                channel: config.updater.channel,
                phase: codeterm_service_core::update_state::UpdatePhase::Idle,
                last_error: None,
                phase_changed_at: None,
                downloaded_bytes: 0,
                download_total_bytes: None,
            },
        )),
        config: Arc::new(parking_lot::Mutex::new(config)),
        mesh: Arc::new(parking_lot::Mutex::new(
            codeterm_service_core::mesh::MeshManager::new("prompt-resolution-test".into(), 0),
        )),
        keepawake: Arc::new(parking_lot::Mutex::new(None)),
        resize_owner: Arc::new(parking_lot::Mutex::new(
            codeterm_service_core::resize_owner::ResizeOwnerRegistry::default(),
        )),
        rest_api: Arc::new(codeterm_service_core::startup::RestApiControl::new()),
        test_clock: None,
        events: fixture.service.events.clone(),
        web_dist: None,
        snapshot_bound: codeterm_service_core::startup::snapshot_bound::SnapshotBound::default(),
    }
}

#[tokio::test]
async fn stop_transition_resolves_the_prompt_through_hook() {
    let (fixture, tab_id, previous_id) = pending_prompt_fixture();
    let state = app_state(&fixture);

    let _ = crate::hook_routes::hook_stop(
        axum::extract::State(state),
        axum::extract::Path(tab_id.clone()),
        axum::http::HeaderMap::new(),
        codeterm_service_core::api_support::OptionalJson(None),
    )
    .await
    .expect("stop hook accepted");

    assert_resolved_and_new_prompt_notifies(&fixture, &tab_id, &previous_id);
}
