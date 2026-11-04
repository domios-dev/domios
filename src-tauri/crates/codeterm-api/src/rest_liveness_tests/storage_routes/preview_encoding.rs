use super::*;
use std::sync::{mpsc, Mutex};

struct HeldSerialization {
    entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    release: mpsc::Receiver<()>,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl serde::Serialize for HeldSerialization {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.entered
            .lock()
            .unwrap()
            .take()
            .unwrap()
            .send(())
            .unwrap();
        self.release.recv_timeout(Duration::from_secs(10)).unwrap();
        serializer.serialize_str("opaque-preview-content")
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn json_serialization_keeps_rest_responsive_and_holds_guard_until_encoded() {
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    let state = app_state(&fixture);
    let semaphore = Arc::new(tokio::sync::Semaphore::new(1));
    let permit = semaphore.clone().acquire_owned().await.unwrap();
    let (entered, started) = tokio::sync::oneshot::channel();
    let (release, receive) = mpsc::channel();
    let payload = Arc::new(Mutex::new(Some(HeldSerialization {
        entered: Mutex::new(Some(entered)),
        release: receive,
        _permit: permit,
    })));
    let router = axum::Router::new().route(
        "/heavy",
        axum::routing::get(crate::blocking_json::handler(move || {
            let payload = payload.clone();
            async move { axum::Json(payload.lock().unwrap().take().unwrap()) }
        })),
    );
    let operation = tokio::spawn(router.oneshot(request("/heavy")));
    started.await.unwrap();
    assert!(!operation.is_finished());
    assert_eq!(semaphore.available_permits(), 0);
    crate::rest_liveness_tests::assert_health_list_send_respond(state).await;
    release.send(()).unwrap();
    let response = operation.await.unwrap().unwrap();
    assert_eq!(semaphore.available_permits(), 1);
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/json");
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    assert_eq!(body.as_ref(), br#""opaque-preview-content""#);
}
