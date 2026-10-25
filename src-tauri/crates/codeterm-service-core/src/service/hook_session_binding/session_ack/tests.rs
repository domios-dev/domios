use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use std::sync::{mpsc, Arc};
use std::time::Duration;

#[test]
fn blueprint_v2_call_durable_async_authentication_keeps_single_thread_runtime_responsive() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let _writes = fixture.bind_recorded_agent_recipient(&tab, "own-session", "own-nonce");
    let claim = SessionStartClaim {
        prev: Some("own-session".into()),
        changed: false,
        nonce_verified: true,
        nonce_already_acknowledged: true,
        identity_evidence: crate::agent::provider_identity::ProviderIdentityEvidence::capture(
            fixture.service.mux.read().get_tab(&tab).unwrap(),
        ),
    };
    let gate = fixture.service.pty_ownership.acquire(&tab);
    let (waiting_tx, waiting_rx) = mpsc::channel();
    fixture
        .service
        .pty_ownership
        .set_test_observer(Arc::new(move |_, event| {
            if event == crate::service::pty_ownership::OwnershipEvent::AwaitingActivation {
                let _ = waiting_tx.send(());
            }
        }));
    let service = fixture.service.clone();
    let (responsive_tx, responsive_rx) = mpsc::channel();
    let runtime = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async move {
            let binding = acknowledge_and_reconcile_session_binding_async(&service, &tab, &claim, Some("own-nonce"));
            tokio::pin!(binding);
            tokio::select! {
                biased;
                result = &mut binding => { panic!("custody must be waiting on the held PTY gate: {result:?}"); }
                _ = async { tokio::task::yield_now().await; let _ = responsive_tx.send(()); } => { binding.await.unwrap(); }
            }
        });
    });
    let waiting = waiting_rx.recv_timeout(Duration::from_secs(5));
    let responsive = responsive_rx.recv_timeout(Duration::from_secs(5));
    drop(gate);
    runtime.join().unwrap();
    waiting.expect("binding reached the contended PTY gate");
    responsive.expect("the async runtime continued while custody was blocked");
}
