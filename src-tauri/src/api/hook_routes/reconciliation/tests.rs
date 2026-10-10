use super::*;
use futures_util::StreamExt;
use std::time::Duration;

#[tokio::test]
async fn response_body_finishes_before_blocked_reconciliation_starts() {
    let (started, mut observed) = oneshot::channel();
    let (release, blocked) = oneshot::channel();
    let (finished, done) = oneshot::channel();
    let response = after_response_work(
        Json(serde_json::json!({"opaque": 42})),
        uuid::Uuid::new_v4().to_string(),
        async move {
            started.send(()).unwrap();
            blocked.await.unwrap();
            finished.send(()).unwrap();
        },
    );
    assert_eq!(
        observed.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    );
    let mut body = response.into_body().into_data_stream();
    let bytes = body.next().await.unwrap().unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
        serde_json::json!({"opaque": 42})
    );
    assert_eq!(
        observed.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    );
    assert!(body.next().await.is_none());
    tokio::time::timeout(Duration::from_secs(1), observed)
        .await
        .unwrap()
        .unwrap();
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(1), done)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn accepted_order_survives_reversed_response_consumption() {
    let key = uuid::Uuid::new_v4().to_string();
    let (release, blocked) = oneshot::channel();
    let (first_started, first_observed) = oneshot::channel();
    let (second_started, mut second_observed) = oneshot::channel();
    let first = after_response_work(Json(serde_json::json!({})), key.clone(), async move {
        first_started.send(()).unwrap();
        blocked.await.unwrap();
    });
    let second = after_response_work(Json(serde_json::json!({})), key, async move {
        second_started.send(()).unwrap();
    });
    axum::body::to_bytes(second.into_body(), 1024)
        .await
        .unwrap();
    assert_eq!(
        second_observed.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    );
    axum::body::to_bytes(first.into_body(), 1024).await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), first_observed)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        second_observed.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    );
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(1), second_observed)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn disconnect_does_not_discard_accepted_reconciliation() {
    let (started, observed) = oneshot::channel();
    let response = after_response_work(
        Json(serde_json::json!({})),
        uuid::Uuid::new_v4().to_string(),
        async move {
            started.send(()).unwrap();
        },
    );
    drop(response);
    tokio::time::timeout(Duration::from_secs(1), observed)
        .await
        .unwrap()
        .unwrap();
}
