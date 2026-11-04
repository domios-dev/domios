use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
    routing::get,
    Router,
};
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::test_fixture::ServiceFixture;
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;

use crate::test_support::app_state;

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
    let response = tokio::time::timeout(
        Duration::from_millis(250),
        router.oneshot(request("/api/health")),
    )
    .await
    .expect("health must finish while auth waits on configuration")
    .unwrap();
    release_tx.send(()).unwrap();
    holder.await.unwrap();
    assert_eq!(
        blocked.await.unwrap().unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 16 * 1024)
        .await
        .unwrap();
    let health: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(health["status"], "ok");
    assert_eq!(health["startup"]["state"], "ready");
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
        let response = tokio::time::timeout(
            Duration::from_millis(250),
            router.oneshot(request("/api/health")),
        )
        .await
        .expect("health must finish while the blocking pool is occupied")
        .unwrap();
        release_tx.send(()).unwrap();
        assert_eq!(blocked.await.unwrap().unwrap().status(), StatusCode::OK);
        assert_eq!(response.status(), StatusCode::OK);
    });
}

mod logging;
mod pty_activation;
mod report_reads;
mod restart_provider;
mod storage_routes;

#[tokio::test]
async fn published_mux_state_returns_seeded_json_while_state_writers_hold_both_locks() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = app_state(&fixture);
    let expected = state.svc.published_mux_state();
    let svc = state.svc.clone();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
        let mut mux = svc.mux.write();
        let _tracker = svc.agent_tracker.lock();
        mux.create_workspace("not published yet".into(), None);
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    });
    entered_rx.await.unwrap();
    let result = tokio::time::timeout(
        Duration::from_millis(250),
        crate::web_routes::mux_state(axum::extract::State(state), axum::http::HeaderMap::new()),
    )
    .await;
    release_tx.send(()).unwrap();
    writer.join().unwrap();
    let response = result
        .expect("published view must not wait for either state lock")
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/json");
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(body, expected);
}

fn owner_headers(state: &crate::routes::AppState) -> axum::http::HeaderMap {
    let token = state
        .config
        .lock()
        .api
        .token
        .get_or_insert_with(|| "liveness-owner-fixture".into())
        .clone();
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
    headers
}

pub(crate) async fn assert_health_list_send_respond(state: crate::routes::AppState) {
    let health = tokio::time::timeout(
        Duration::from_secs(1),
        crate::health_routes::health(axum::extract::State(state.clone())),
    )
    .await
    .expect("health must remain responsive");
    assert_eq!(health.0["status"], "ok");
    let listing = tokio::time::timeout(
        Duration::from_secs(1),
        crate::tab_routes::list_tabs(
            axum::extract::State(state.clone()),
            owner_headers(&state),
            axum::extract::Query(crate::tab_routes::TabListQuery {
                local_only: true,
                all_workspaces: true,
                ..Default::default()
            }),
        ),
    )
    .await
    .expect("tab list must remain responsive")
    .unwrap();
    assert!(listing.0.is_array());
    let rejection = tokio::time::timeout(
        Duration::from_secs(1),
        crate::tab_routes::send_text(
            axum::extract::State(state.clone()),
            owner_headers(&state),
            axum::extract::Path("missing-liveness-target".into()),
            axum::Json(serde_json::from_value(serde_json::json!({"text":"opaque"})).unwrap()),
        ),
    )
    .await
    .expect("send preflight must remain responsive")
    .unwrap_err();
    let response = axum::response::IntoResponse::into_response(rejection);
    let bytes = axum::body::to_bytes(response.into_body(), 16 * 1024)
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["error"]["code"], "tab_not_reachable_on_peer");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn rest_liveness_credential_lookup_waits_off_worker_while_mux_is_held() {
    use futures_util::FutureExt;
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let state = app_state(&fixture);
    let token = state.svc.frontend_pty_env(Some(&tab)).unwrap()["CODETERM_API_TOKEN"].clone();
    let router = crate::routes::create_router(state.clone());
    let service = state.svc.clone();
    let (entered, ready) = tokio::sync::oneshot::channel();
    let (release, receive) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let _mux = service.mux.write();
        entered.send(()).unwrap();
        receive.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    ready.await.unwrap();
    let mut identified = request("/api/identify");
    identified
        .headers_mut()
        .insert("authorization", format!("Bearer {token}").parse().unwrap());
    let mut identifying = Box::pin(router.clone().oneshot(identified));
    assert!(identifying.as_mut().now_or_never().is_none());
    let health = tokio::time::timeout(
        Duration::from_secs(1),
        router.oneshot(request("/api/health")),
    )
    .await;
    release.send(()).unwrap();
    holder.join().unwrap();
    assert_eq!(
        health
            .expect("credential/mux wait must not block REST health")
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(identifying.await.unwrap().status(), StatusCode::OK);
}
