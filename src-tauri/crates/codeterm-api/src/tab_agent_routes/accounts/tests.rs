use axum::{body::Body, http::Request, routing::get, Router};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use futures_util::FutureExt;
use std::time::Duration;
use tower::ServiceExt;

#[tokio::test]
async fn tab_account_waits_off_runtime_for_mux() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    let router = Router::new()
        .route("/api/tabs/{id}/account", get(super::tab_status))
        .route("/api/health", get(crate::health_routes::health))
        .with_state(state.clone());
    let mux = state.svc.mux.clone();
    let (locked_tx, locked_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let _mux = mux.write();
        locked_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    locked_rx.await.unwrap();

    let request = |uri: &str| Request::builder().uri(uri).body(Body::empty()).unwrap();
    let account_path = format!("/api/tabs/{tab}/account");
    let mut account = Box::pin(router.clone().oneshot(request(&account_path)));
    assert!(account.as_mut().now_or_never().is_none());
    assert!(
        state.svc.mux.try_read().is_none(),
        "account handler must yield while mux is held"
    );
    let health = router.oneshot(request("/api/health")).await.unwrap();
    release_tx.send(()).unwrap();
    holder.join().unwrap();

    assert_eq!(health.status(), axum::http::StatusCode::OK);
    let response = account.await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
}
