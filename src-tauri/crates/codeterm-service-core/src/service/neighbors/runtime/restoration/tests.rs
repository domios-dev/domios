use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
mod file_support;

#[test]
fn readonly_startup_requires_the_exact_local_receiver_key_and_no_pending_cleanup() {
    let mut status = ReceiverStatus {
        receiver_revision: 2,
        receiver_installation_id: Some("here".into()),
        receiver_key_id: Some("vk_current".into()),
        receiver_machine_label: None,
        receiving_enabled: true,
        is_me: true,
        legacy_active: false,
    };
    assert!(!needs_key_recovery(&status, Some("vk_current"), false));
    assert!(needs_key_recovery(&status, Some("vk_old"), false));
    assert!(needs_key_recovery(&status, None, false));
    assert!(needs_key_recovery(&status, Some("vk_current"), true));
    status.is_me = false;
    assert!(!needs_key_recovery(&status, None, false));
}

#[tokio::test]
async fn restoration_starts_without_the_operation_queue_and_off_cancels_it() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let (owner, mut queued) = tokio::sync::mpsc::channel(1);
    *service.neighbor_cell().owner.lock() = Some(owner);
    service
        .neighbor_cell()
        .bootstrapped
        .store(true, Ordering::Release);
    let (started, ready) = tokio::sync::oneshot::channel();
    let (held, released) = tokio::sync::oneshot::channel::<()>();
    service.spawn_neighbor_restoration(async move {
        let _held = held;
        started.send(()).unwrap();
        std::future::pending::<()>().await;
    });
    tokio::select! { result = ready => result.unwrap(), _ = queued.recv() => panic!("Watch startup cannot wait in the network-operation queue") }
    service.pause_neighbor_reception();
    assert!(released.await.is_err());
    assert!(service.neighbor_cell().restoration.lock().is_none());
    assert!(!service.neighbor_cell().bootstrapped.load(Ordering::Acquire));
}

#[tokio::test]
async fn setup_outages_recover_control_and_friend_watches_without_opening_neighbors() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let (clock, mut deadlines) = super::super::super::recovery::test_clock::Clock::new();
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = attempts.clone();
    let (active, mut ready) = tokio::sync::mpsc::unbounded_channel();
    let (held, ended) = tokio::sync::oneshot::channel::<()>();
    let held = Arc::new(parking_lot::Mutex::new(Some(held)));
    service.start_neighbor_restoration_with(
        service.neighbor_history_generation(),
        move || {
            let count = count.clone();
            let active = active.clone();
            let held = held.clone();
            async move {
                let stage = count.fetch_add(1, Ordering::SeqCst);
                if stage < 2 {
                    return Err(Error::Transport);
                }
                let _held = held.lock().take();
                active.send(["control", "friend"]).unwrap();
                std::future::pending::<codeterm_neighbors::Result<()>>().await
            }
        },
        clock.clone(),
    );
    for stage in 1..=2 {
        assert_eq!(
            deadlines.recv().await,
            Some(std::time::Duration::from_millis(500))
        );
        assert_eq!(attempts.load(Ordering::SeqCst), stage);
        clock.advance();
    }
    assert_eq!(ready.recv().await, Some(["control", "friend"]));
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
    service.pause_neighbor_reception();
    assert!(ended.await.is_err());
    assert!(service.neighbor_cell().restoration.lock().is_none());
}

#[tokio::test]
async fn entitlement_rearm_arriving_before_failed_restoration_returns_is_not_lost() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let (clock, mut deadlines) = super::super::super::recovery::test_clock::Clock::new();
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let permit = release.clone();
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (events, mut observed) = tokio::sync::mpsc::unbounded_channel();
    service.start_neighbor_restoration_with(
        service.neighbor_history_generation(),
        move || {
            let count = count.clone();
            let events = events.clone();
            let permit = permit.clone();
            async move {
                let call = count.fetch_add(1, Ordering::SeqCst);
                events.send(call).unwrap();
                if call == 0 {
                    permit.acquire().await.unwrap().forget();
                    return Err(Error::PermissionDenied);
                }
                std::future::pending::<codeterm_neighbors::Result<()>>().await
            }
        },
        clock,
    );
    assert_eq!(observed.recv().await, Some(0));
    service.rearm_neighbor_restoration();
    release.add_permits(1);
    assert_eq!(observed.recv().await, Some(1));
    assert!(deadlines.try_recv().is_err());
    service.pause_neighbor_reception();
}
