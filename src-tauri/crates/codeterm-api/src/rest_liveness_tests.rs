use crate::routes::AppState;
use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
    routing::get,
    Router,
};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tower::ServiceExt;

fn app_state(fixture: &ServiceFixture) -> AppState {
    let config = codeterm_service_core::config::schema::AppConfig::default();
    let mut mesh =
        codeterm_service_core::mesh::MeshManager::new("cron-entry-off-runtime".into(), 0);
    mesh.disk_persist = false;
    AppState {
        svc: Arc::new(fixture.service.clone()),
        cron: Arc::new(parking_lot::Mutex::new(
            codeterm_service_core::cron::store::CronStore::new_with_path(
                fixture.temp.path().join("cron.json"),
            ),
        )),
        cron_wake: Arc::new(std::sync::Condvar::new()),
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
        snapshot_bound: codeterm_service_core::startup::snapshot_bound::SnapshotBound::default(),
    }
}

fn request(path: &str) -> Request<Body> {
    let mut request = Request::builder().uri(path).body(Body::empty()).unwrap();
    request.extensions_mut().insert(ConnectInfo(
        "127.0.0.1:1234".parse::<std::net::SocketAddr>().unwrap(),
    ));
    request
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn rest_liveness_health_completes_while_auth_waits_on_config() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = app_state(&fixture);
    let router = crate::routes::create_router(state.clone());
    let config = state.config.clone();
    let (locked_tx, locked_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let holder = tokio::task::spawn_blocking(move || {
        let _guard = config.lock();
        locked_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    });
    locked_rx.await.unwrap();
    let entered = Arc::new(tokio::sync::Notify::new());
    let mut authenticated = request("/api/identify");
    authenticated.extensions_mut().insert(entered.clone());
    let blocked = tokio::spawn(router.clone().oneshot(authenticated));
    entered.notified().await;
    assert!(!blocked.is_finished());
    assert!(state.config.try_lock().is_none());
    let started = Instant::now();
    let response = router.oneshot(request("/api/health")).await.unwrap();
    let elapsed = started.elapsed();
    release_tx.send(()).unwrap();
    holder.await.unwrap();
    assert_eq!(
        blocked.await.unwrap().unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        elapsed < Duration::from_millis(50),
        "health took {elapsed:?}"
    );
}

#[test]
fn rest_liveness_health_bypasses_saturated_blocking_pool_with_real_middleware() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    runtime.block_on(async {
        let fixture = ServiceFixture::new(MultiplexerState::empty());
        let state = app_state(&fixture);
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let blocker = Arc::new(parking_lot::Mutex::new(Some((entered_tx, release_rx))));
        let routes = Router::new()
            .route("/api/health", get(crate::health_routes::health))
            .route(
                "/blocking",
                get(move || {
                    let blocker = blocker.clone();
                    async move {
                        tokio::task::spawn_blocking(move || {
                            let (entered, release) = blocker.lock().take().unwrap();
                            entered.send(()).unwrap();
                            release.recv_timeout(Duration::from_secs(3)).unwrap();
                        })
                        .await
                        .unwrap();
                        StatusCode::OK
                    }
                }),
            );
        let router = crate::routes::apply_middleware(routes, state);
        let blocked = tokio::spawn(router.clone().oneshot(request("/blocking")));
        entered_rx.await.unwrap();
        let started = Instant::now();
        let response = router.oneshot(request("/api/health")).await.unwrap();
        let elapsed = started.elapsed();
        release_tx.send(()).unwrap();
        assert_eq!(blocked.await.unwrap().unwrap().status(), StatusCode::OK);
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            elapsed < Duration::from_millis(50),
            "health took {elapsed:?}"
        );
    });
}
