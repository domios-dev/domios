use crate::routes::AppState;
use codeterm_service_core::service::test_fixture::ServiceFixture;
use std::sync::Arc;

pub fn app_state(fixture: &ServiceFixture) -> AppState {
    let config = codeterm_service_core::config::schema::AppConfig::default();
    let mut mesh =
        codeterm_service_core::mesh::MeshManager::new("cron-entry-off-runtime".into(), 0);
    mesh.disk_persist = false;
    AppState {
        frontend_dev_url: None,
        svc: Arc::new(fixture.service.clone()),
        cron: Arc::new(parking_lot::Mutex::new(
            codeterm_service_core::cron::store::CronStore::new_with_path(
                fixture.temp.path().join("cron.json"),
            ),
        )),
        cron_wake: Arc::new(std::sync::Condvar::new()),
        api_port: 0,
        update_state: Arc::new(parking_lot::Mutex::new(codeterm_update::UpdateState {
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
        config: fixture
            .service
            .ext::<codeterm_service_core::config::ConfigState>()
            .clone(),
        mesh: Arc::new(parking_lot::Mutex::new(mesh)),
        keepawake: Arc::new(parking_lot::Mutex::new(None)),
        resize_owner: Arc::new(parking_lot::Mutex::new(Default::default())),
        rest_api: Arc::new(codeterm_service_core::startup::RestApiControl::new()),
        test_clock: None,
        events: fixture.service.events.clone(),
        web_dist: None,
    }
}
