use std::sync::Arc;

use crate::api::routes::AppState;
use crate::service::prompt_resolution::tests::{
    assert_resolved_and_new_prompt_notifies, pending_prompt_fixture,
};

fn app_state(fixture: &crate::service::test_fixture::ServiceFixture) -> AppState {
    let config = crate::config::schema::AppConfig::default();
    AppState {
        svc: Arc::new(fixture.service.clone()),
        cron: Arc::new(parking_lot::Mutex::new(
            crate::cron::store::CronStore::new_with_path(
                fixture.temp.path().join("prompt-resolution-crons.json"),
            ),
        )),
        cron_wake: Arc::new(std::sync::Condvar::new()),
        api_port: 0,
        update_state: Arc::new(parking_lot::Mutex::new(crate::update_state::UpdateState {
            install_method: crate::update_state::InstallMethod::Unknown,
            last_check: None,
            last_check_time: None,
            channel: config.updater.channel,
            phase: crate::update_state::UpdatePhase::Idle,
            last_error: None,
            phase_changed_at: None,
            downloaded_bytes: 0,
            download_total_bytes: None,
        })),
        config: Arc::new(parking_lot::Mutex::new(config)),
        mesh: Arc::new(parking_lot::Mutex::new(crate::mesh::MeshManager::new(
            "prompt-resolution-test".into(),
            0,
        ))),
        keepawake: Arc::new(parking_lot::Mutex::new(None)),
        resize_owner: Arc::new(parking_lot::Mutex::new(
            crate::resize_owner::ResizeOwnerRegistry::default(),
        )),
        rest_api: Arc::new(crate::startup::RestApiControl::new()),
        test_clock: None,
        events: fixture.service.events.clone(),
        web_dist: None,
        snapshot_bound: crate::startup::snapshot_bound::SnapshotBound::default(),
    }
}

#[tokio::test]
async fn stop_transition_resolves_the_prompt_through_hook() {
    let (fixture, tab_id, previous_id) = pending_prompt_fixture();
    let state = app_state(&fixture);

    let _ = crate::api::hook_routes::hook_stop(
        axum::extract::State(state),
        axum::extract::Path(tab_id.clone()),
        axum::http::HeaderMap::new(),
        crate::api_support::optional_json::OptionalJson(None),
    )
    .await
    .expect("stop hook accepted");

    assert_resolved_and_new_prompt_notifies(&fixture, &tab_id, &previous_id);
}
