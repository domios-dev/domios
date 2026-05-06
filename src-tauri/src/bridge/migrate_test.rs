#![cfg(test)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};

use crate::startup::start_telegram_bridge_for_test;

#[derive(Clone)]
struct MockState {
    active: Arc<AtomicU32>,
    conflict_seen: Arc<AtomicU32>,
}

struct ActiveGuard(Arc<AtomicU32>);
impl Drop for ActiveGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn mock_get_updates(
    Path((_token, method)): Path<(String, String)>,
    State(state): State<MockState>,
    Json(_body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    if method != "getUpdates" {
        return (StatusCode::OK, Json(json!({"ok": true, "result": []})));
    }

    let active = state.active.fetch_add(1, Ordering::SeqCst) + 1;
    let _guard = ActiveGuard(state.active.clone());
    if active > 1 {
        state.conflict_seen.fetch_add(1, Ordering::SeqCst);
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "ok": false,
                "error_code": 409,
                "description": "Conflict: terminated by other getUpdates request"
            })),
        );
    }

    tokio::time::sleep(Duration::from_millis(800)).await;
    (StatusCode::OK, Json(json!({"ok": true, "result": []})))
}

async fn spawn_mock_telegram() -> (String, MockState) {
    let state = MockState {
        active: Arc::new(AtomicU32::new(0)),
        conflict_seen: Arc::new(AtomicU32::new(0)),
    };

    let app = Router::new()
        .route("/bot{token}/{method}", post(mock_get_updates))
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    (format!("http://{}", addr), state)
}

async fn wait_for_first_poll(
    sup: &Arc<crate::bridge::supervisor::BridgeSupervisor>,
    deadline: Duration,
) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < deadline {
        if sup.snapshot().last_poll_ok.is_some() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

#[tokio::test]
async fn migrate_handover_no_409() {
    let (api_base, mock) = spawn_mock_telegram().await;
    let token = "FAKE_TOKEN".to_string();

    let sup_a = start_telegram_bridge_for_test(token.clone(), api_base.clone()).await;
    assert!(
        wait_for_first_poll(&sup_a, Duration::from_secs(5)).await,
        "supervisor A never landed first poll"
    );

    let stop_started = std::time::Instant::now();
    sup_a
        .stop(Duration::from_secs(5))
        .await
        .expect("supervisor A stop");
    let stop_elapsed = stop_started.elapsed();
    assert!(
        stop_elapsed < Duration::from_secs(3),
        "stop took {:?}, expected <3s (cancel was honored)",
        stop_elapsed
    );

    while mock.active.load(Ordering::SeqCst) > 0 {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    mock.conflict_seen.store(0, Ordering::SeqCst);

    let sup_b = start_telegram_bridge_for_test(token, api_base).await;
    assert!(
        wait_for_first_poll(&sup_b, Duration::from_secs(5)).await,
        "supervisor B never landed first poll"
    );

    let snap = sup_b.snapshot();
    assert_eq!(
        snap.error_count, 0,
        "supervisor B saw errors: {:?}",
        snap.last_poll_error
    );
    assert_eq!(
        mock.conflict_seen.load(Ordering::SeqCst),
        0,
        "mock saw 409 conflicts during clean handover"
    );

    sup_b.stop(Duration::from_secs(5)).await.expect("stop b");
}
