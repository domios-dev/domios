use crate::routes::AppState;
use codeterm_host_runtime::test_fixture::HostFixture as ServiceFixture;
use std::sync::Arc;
use tauri::Manager;

pub(crate) fn app_state(fixture: &ServiceFixture) -> AppState {
    let config = fixture
        .app
        .state::<codeterm_service_core::config::ConfigState>()
        .inner()
        .clone();
    config.lock().api.token = Some("preview-test-token".into());
    let mut mesh = codeterm_service_core::mesh::MeshManager::new("local-preview-test".into(), 0);
    mesh.disk_persist = false;
    AppState {
        app: fixture.app.handle().clone(),
        svc: Arc::new(fixture.service.clone()),
        cron: Arc::new(parking_lot::Mutex::new(
            codeterm_service_core::cron::store::CronStore::new_with_path(
                fixture.temp.path().join("crons.json"),
            ),
        )),
        cron_wake: Arc::new(std::sync::Condvar::new()),
        api_port: 0,
        update_state: Arc::new(parking_lot::Mutex::new(
            codeterm_commands::update::UpdateState {
                install_method: codeterm_commands::update::InstallMethod::Unknown,
                last_check: None,
                last_check_time: None,
                channel: codeterm_commands::update::Channel::default(),
                phase: codeterm_commands::update::UpdatePhase::Idle,
                last_error: None,
                phase_changed_at: None,
                downloaded_bytes: 0,
                download_total_bytes: None,
            },
        )),
        config,
        mesh: Arc::new(parking_lot::Mutex::new(mesh)),
        keepawake: Arc::new(parking_lot::Mutex::new(None)),
        resize_owner: Arc::new(parking_lot::Mutex::new(Default::default())),
        rest_api: Arc::new(codeterm_service_core::startup::RestApiControl::new()),
        test_clock: None,
        events: fixture.service.events.clone(),
        web_dist: None,
    }
}
