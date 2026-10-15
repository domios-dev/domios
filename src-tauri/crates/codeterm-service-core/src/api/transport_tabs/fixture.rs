use crate::api::routes::AppState;
use crate::service::test_fixture::ServiceFixture;
use std::sync::Arc;
use tauri::Manager;

pub(crate) fn app_state(fixture: &ServiceFixture) -> AppState {
    let config = fixture
        ._app
        .state::<crate::config::ConfigState>()
        .inner()
        .clone();
    config.lock().api.token = Some("preview-test-token".into());
    let mut mesh = crate::mesh::MeshManager::new("local-preview-test".into(), 0);
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
        snapshot_bound: crate::startup::snapshot_bound::SnapshotBound::default(),
    }
}
