use super::*;

#[tokio::test]
async fn disabling_cancels_the_account_signal_watch_and_invalidates_its_generation() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let (started, running) = tokio::sync::oneshot::channel();
    let (held, ended) = tokio::sync::oneshot::channel::<()>();
    let (stopped, _shutdown) = watch::channel(false);
    let task = tokio::spawn(async move {
        let _held = held;
        started.send(()).unwrap();
        std::future::pending::<()>().await;
    });
    *fixture.service.neighbor_updates().task.lock() = Some(Handle {
        task,
        stopped,
        retry: Arc::new(Notify::new()),
    });
    running.await.unwrap();
    fixture
        .service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = false;
    fixture.service.neighbors_config_changed();
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(1), ended)
            .await
            .unwrap()
            .is_err()
    );
    assert!(fixture.service.neighbor_updates().task.lock().is_none());
    assert_eq!(
        fixture
            .service
            .neighbor_updates()
            .generation
            .load(Ordering::Acquire),
        1
    );
}
