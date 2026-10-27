use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};

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
