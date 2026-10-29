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
        rearm: Arc::new(super::super::recovery::Rearm::default()),
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

#[tokio::test]
async fn account_setup_outage_recovers_without_a_ui_action_and_stop_cancels_active_work() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = &fixture.service;
    let (clock, mut deadlines) = super::super::recovery::test_clock::Clock::new();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = calls.clone();
    let (active, mut ready) = tokio::sync::mpsc::unbounded_channel();
    let (held, ended) = tokio::sync::oneshot::channel::<()>();
    let held = Arc::new(parking_lot::Mutex::new(Some(held)));
    service.start_neighbor_updates_with(
        move || {
            let count = count.clone();
            let active = active.clone();
            let held = held.clone();
            async move {
                if count.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err(Error::Transport);
                }
                let _held = held.lock().take();
                active.send(()).unwrap();
                std::future::pending::<Result<()>>().await
            }
        },
        clock.clone(),
    );
    assert_eq!(
        deadlines.recv().await,
        Some(std::time::Duration::from_millis(500))
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    clock.advance();
    ready.recv().await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(!service
        .neighbor_updates()
        .task
        .lock()
        .as_ref()
        .unwrap()
        .task
        .is_finished());
    service.stop_neighbor_updates();
    assert!(ended.await.is_err());
    assert!(service.neighbor_updates().task.lock().is_none());
}

#[tokio::test]
async fn account_rearm_during_a_failing_setup_is_retained() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = &fixture.service;
    let (clock, mut deadlines) = super::super::recovery::test_clock::Clock::new();
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let permit = release.clone();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = calls.clone();
    let (events, mut observed) = tokio::sync::mpsc::unbounded_channel();
    service.start_neighbor_updates_with(
        move || {
            let count = count.clone();
            let events = events.clone();
            let permit = permit.clone();
            async move {
                let call = count.fetch_add(1, Ordering::SeqCst);
                events.send(call).unwrap();
                if call == 0 {
                    permit.acquire().await.unwrap().forget();
                    return Err(Error::Locked);
                }
                std::future::pending::<Result<()>>().await
            }
        },
        clock,
    );
    assert_eq!(observed.recv().await, Some(0));
    service.rearm_neighbor_updates();
    release.add_permits(1);
    assert_eq!(observed.recv().await, Some(1));
    assert!(deadlines.try_recv().is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    service.stop_neighbor_updates();
}
