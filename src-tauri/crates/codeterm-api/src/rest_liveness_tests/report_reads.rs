use super::*;
use axum::extract::{Path, Query, State};
use futures_util::FutureExt;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn rest_health_answers_while_done_and_report_reads_wait_on_the_store() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = app_state(&fixture);
    let router = crate::routes::create_router(state.clone());
    let store = state.svc.report_store.clone();
    let (locked_tx, locked_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let _guard = store.lock();
        locked_tx.send(()).unwrap();
        let _ = release_rx.recv_timeout(Duration::from_secs(3));
    });
    locked_rx.await.unwrap();
    let mut done = Box::pin(crate::agent_done_routes::list_done(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Query(crate::agent_done_routes::DoneQuery { limit: Some(10) }),
    ));
    let mut reports = Box::pin(crate::agent_routes::get_reports(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Path("fixture".into()),
        Query(crate::agent_routes::ReportsQuery { clear: Some(false) }),
    ));
    let done_pending = done.as_mut().now_or_never().is_none();
    let reports_pending = reports.as_mut().now_or_never().is_none();
    let still_locked = state.svc.report_store.try_lock().is_none();
    let health = tokio::time::timeout(
        Duration::from_millis(250),
        router.oneshot(request("/api/health")),
    )
    .await;
    release_tx.send(()).ok();
    holder.join().unwrap();
    assert!(
        done_pending,
        "Done must yield while the report store is held"
    );
    assert!(
        reports_pending,
        "Reports must yield while the report store is held"
    );
    assert!(
        still_locked,
        "health must answer before the dependency is released"
    );
    assert_eq!(
        health
            .expect("REST health must remain responsive")
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(done.await.unwrap().0["completed"], serde_json::json!([]));
    assert_eq!(reports.await.unwrap().0, serde_json::json!([]));
}
