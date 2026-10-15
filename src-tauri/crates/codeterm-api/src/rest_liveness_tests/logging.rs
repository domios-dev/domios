use super::*;
use codeterm_util::nonblocking_log::NonBlockingLog;
use std::io::Write;

struct ParkedStderr {
    entered: Option<tokio::sync::oneshot::Sender<()>>,
    release: std::sync::mpsc::Receiver<()>,
}

impl Write for ParkedStderr {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if let Some(entered) = self.entered.take() {
            entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(3)).unwrap();
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn authenticated(path: &str) -> Request<Body> {
    let mut request = request(path);
    request
        .headers_mut()
        .insert("authorization", "Bearer fixture-token".parse().unwrap());
    request
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn parked_stderr_and_slow_catalogue_do_not_delay_health_or_inline_auth() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = app_state(&fixture);
    state.config.lock().api.token = Some("fixture-token".into());
    let (sink_entered_tx, sink_entered_rx) = tokio::sync::oneshot::channel();
    let (sink_release_tx, sink_release_rx) = std::sync::mpsc::channel();
    let sink = NonBlockingLog::spawn(
        "stderr",
        ParkedStderr {
            entered: Some(sink_entered_tx),
            release: sink_release_rx,
        },
        1,
        Vec::new(),
    )
    .unwrap();
    sink.submit(Arc::from(&b"park\n"[..]));
    sink_entered_rx.await.unwrap();
    sink.submit(Arc::from(&b"queued\n"[..]));
    let drops = sink.drops();
    let (catalogue_entered_tx, catalogue_entered_rx) = tokio::sync::oneshot::channel();
    let (catalogue_release_tx, catalogue_release_rx) = std::sync::mpsc::channel();
    let catalogue = Arc::new(parking_lot::Mutex::new(Some((
        catalogue_entered_tx,
        catalogue_release_rx,
    ))));
    let routes = Router::new()
        .route("/api/health", get(crate::health_routes::health))
        .route("/api/identify", get(crate::health_routes::identify))
        .route(
            "/api/agents/providers",
            get(move || {
                let (catalogue, sink) = (catalogue.clone(), sink.clone());
                async move {
                    sink.submit(Arc::from(&b"catalogue request\n"[..]));
                    tokio::task::spawn_blocking(move || {
                        let (entered, release) = catalogue.lock().take().unwrap();
                        entered.send(()).unwrap();
                        release.recv_timeout(Duration::from_secs(3)).unwrap();
                    })
                    .await
                    .unwrap();
                    axum::Json(serde_json::json!([]))
                }
            }),
        );
    let router = crate::routes::apply_middleware(routes, state);
    let listing = tokio::spawn(
        router
            .clone()
            .oneshot(authenticated("/api/agents/providers")),
    );
    catalogue_entered_rx.await.unwrap();
    let started = Instant::now();
    let health = router
        .clone()
        .oneshot(request("/api/health"))
        .await
        .unwrap();
    let inline_auth = router
        .oneshot(authenticated("/api/identify"))
        .await
        .unwrap();
    let elapsed = started.elapsed();
    let still_waiting = !listing.is_finished();
    catalogue_release_tx.send(()).unwrap();
    sink_release_tx.send(()).unwrap();
    assert_eq!(listing.await.unwrap().unwrap().status(), StatusCode::OK);
    assert!(still_waiting);
    assert_eq!(health.status(), StatusCode::OK);
    assert_eq!(inline_auth.status(), StatusCode::OK);
    assert!(
        elapsed < Duration::from_millis(50),
        "liveness requests took {elapsed:?}"
    );
    assert_eq!(drops.total(), 1);
}
