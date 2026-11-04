use super::*;
use codeterm_service_core::service::OwnershipEvent;

#[test]
fn held_activation_keeps_health_and_the_only_blocking_worker_available() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    runtime.block_on(async {
        let (mux, tab) = MultiplexerState::new();
        let fixture = ServiceFixture::new(mux);
        let state = app_state(&fixture);
        let held = state.svc.pty_ownership.acquire(&tab).unwrap();
        let (entered, observed) = tokio::sync::oneshot::channel();
        let entered = Arc::new(parking_lot::Mutex::new(Some(entered)));
        state
            .svc
            .pty_ownership
            .set_test_observer(Arc::new(move |_, event| {
                if event == OwnershipEvent::AwaitingActivation {
                    if let Some(entered) = entered.lock().take() {
                        let _ = entered.send(());
                    }
                }
            }));
        let ownership = state.svc.pty_ownership.clone();
        let waiter = tokio::spawn(async move { ownership.acquire_async(tab).await });
        tokio::time::timeout(Duration::from_secs(2), observed)
            .await
            .unwrap()
            .unwrap();
        assert!(!waiter.is_finished());
        let router = crate::routes::apply_middleware(
            Router::new().route("/api/health", get(crate::health_routes::health)),
            state,
        );
        let health = tokio::time::timeout(
            Duration::from_secs(1),
            router.oneshot(request("/api/health")),
        )
        .await;
        let worker = tokio::time::timeout(
            Duration::from_secs(1),
            tokio::task::spawn_blocking(|| "available"),
        )
        .await;
        waiter.abort();
        assert!(matches!(waiter.await, Err(error) if error.is_cancelled()));
        drop(held);
        assert_eq!(health.unwrap().unwrap().status(), StatusCode::OK);
        assert_eq!(worker.unwrap().unwrap(), "available");
    });
}
