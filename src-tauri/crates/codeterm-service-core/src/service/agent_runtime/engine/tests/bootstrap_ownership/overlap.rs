use super::*;
use crate::service::pending_delivery_payloads::{
    live_payload_keys, user_sends::retention::RestoredPayloadOwners,
};

const PAYLOAD: &str = "\topaque-overlapping-bootstrap-\u{03bb}\r\n";

async fn request(peer: &mut BufReader<tokio::io::DuplexStream>, send: &str, thread: &str) -> Value {
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(2), peer.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    let request: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(request["method"], "fixture/submit");
    assert_eq!(request["params"]["input"]["text"], PAYLOAD);
    assert_eq!(request["params"]["sendId"], send);
    assert_eq!(request["params"]["threadId"], thread);
    request
}

async fn accept(peer: &mut BufReader<tokio::io::DuplexStream>, request: &Value) {
    let response = serde_json::json!({"id":request["id"],"result":{"turnId":"owned-turn"}});
    peer.get_mut()
        .write_all(format!("{response}\n").as_bytes())
        .await
        .unwrap();
}

fn assert_no_request(peer: &mut BufReader<tokio::io::DuplexStream>) {
    use std::{
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    use tokio::io::AsyncRead;
    assert!(peer.buffer().is_empty());
    let mut bytes = [0; 1];
    let mut buffer = tokio::io::ReadBuf::new(&mut bytes);
    assert!(matches!(
        Pin::new(peer.get_mut()).poll_read(&mut Context::from_waker(Waker::noop()), &mut buffer),
        Poll::Pending
    ));
}

fn overlapping_bootstraps(rollback: bool) {
    let (fixture, first, first_peer, _first_cleanup) = runtime_fixture();
    let workspace = fixture
        .service
        .mux
        .read()
        .find_workspace_for_tab(&first)
        .unwrap();
    let second = {
        let mut mux = fixture.service.mux.write();
        let source = mux.get_tab(&first).unwrap().clone();
        let id = mux.create_tab(&workspace).unwrap();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_type = source.agent_type;
        tab.agent_session_id = Some("second-thread".into());
        tab.session_binding_nonce = Some(uuid::Uuid::new_v4().to_string());
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
        tab.cwd = source.cwd;
        tab.backing = crate::multiplexer::types::TabBacking::ChatBackend {
            plugin_id: tab.agent_type.clone().unwrap(),
            session_id: "second-thread".into(),
        };
        id
    };
    let (second_peer, _second_cleanup) =
        super::super::sync_send::attach_runtime_peer(&fixture, &second);
    fixture.install_seeded_storage_desk();
    let sends = [
        uuid::Uuid::new_v4().to_string(),
        uuid::Uuid::new_v4().to_string(),
    ];
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let handles: Vec<_> = [&first, &second]
            .into_iter()
            .zip(&sends)
            .map(|(tab, send)| {
                let service = &fixture.service;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    service
                        .create_pending_delivery_with_send_id(
                            tab,
                            PendingDeliveryChannel::InteractiveInput,
                            PAYLOAD,
                            Some(send),
                        )
                        .unwrap();
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
    });
    let records: Vec<_> = [&first, &second]
        .into_iter()
        .map(|id| {
            fixture
                .service
                .mux
                .read()
                .get_tab(id)
                .unwrap()
                .pending_delivery
                .clone()
                .unwrap()
        })
        .collect();
    assert_eq!(records[0].content_digest, records[1].content_digest);
    assert_ne!(records[0].record_id(), records[1].record_id());
    assert_ne!(
        fixture
            .service
            .pending_delivery_payloads
            .path_for_test(&records[0]),
        fixture
            .service
            .pending_delivery_payloads
            .path_for_test(&records[1])
    );

    crate::async_runtime::block_on(async {
        let mut first_peer = BufReader::new(first_peer);
        let mut second_peer = BufReader::new(second_peer);
        let submit = |tab: &str| {
            let service = fixture.service.clone();
            let session = for_tab(tab).unwrap();
            let process = session.process.lock().clone().unwrap();
            crate::async_runtime::spawn(async move {
                for _ in 0..2 {
                    super::super::super::super::bootstrap::submit(&service, &session, &process)
                        .await
                        .unwrap();
                }
            })
        };
        let first_submit = submit(&first);
        let second_submit = submit(&second);
        let first_request = request(&mut first_peer, &sends[0], "own-thread").await;
        let second_request = request(&mut second_peer, &sends[1], "second-thread").await;
        accept(&mut first_peer, &first_request).await;
        tokio::time::timeout(Duration::from_secs(5), first_submit)
            .await
            .unwrap()
            .unwrap();
        assert_no_request(&mut first_peer);
        let owner = fixture.service.clone();
        let retired_tab = first.clone();
        let live_tab = second.clone();
        let surviving_record = records[1].clone();
        tokio::task::spawn_blocking(move || {
            if rollback {
                assert_eq!(
                    owner.rollback_failed_agent_spawn(&retired_tab, "injected rollback"),
                    "injected rollback"
                );
                assert!(owner.mux.read().get_tab(&retired_tab).is_none());
                owner
                    .pending_delivery_payloads
                    .user_sends
                    .purge_closed_tab(&retired_tab)
                    .unwrap();
            } else {
                owner
                    .reconcile_pending_delivery(&retired_tab, Ok(true))
                    .unwrap();
            }
            let snapshot = owner.mux.read().snapshot();
            let persisted = owner.mux.read().to_session_snapshot();
            owner
                .pending_delivery_payloads
                .sweep_orphans(&live_payload_keys(&snapshot))
                .unwrap();
            owner
                .pending_delivery_payloads
                .user_sends
                .sweep_closed_tabs(&RestoredPayloadOwners::new(&snapshot, Some(&persisted)))
                .unwrap();
            assert_eq!(
                owner
                    .pending_delivery_payloads
                    .resolve(&surviving_record)
                    .unwrap(),
                PAYLOAD
            );
            assert_eq!(
                owner
                    .mux
                    .read()
                    .get_tab(&live_tab)
                    .unwrap()
                    .pending_delivery
                    .as_ref()
                    .unwrap()
                    .record_id(),
                surviving_record.record_id()
            );
        })
        .await
        .unwrap();
        accept(&mut second_peer, &second_request).await;
        tokio::time::timeout(Duration::from_secs(5), second_submit)
            .await
            .unwrap()
            .unwrap();
        assert_no_request(&mut second_peer);
        assert_eq!(
            fixture
                .service
                .chat_send_ledger
                .status(&sends[1])
                .unwrap()
                .state,
            ChatSendState::Written
        );
        let claimed = fixture
            .service
            .mux
            .read()
            .get_tab(&second)
            .unwrap()
            .pending_delivery
            .clone()
            .unwrap();
        assert_eq!(claimed.record_id(), records[1].record_id());
        assert_eq!(claimed.state(), PendingDeliveryState::Unconfirmed);
        assert!(claimed.attempt_provenance().is_some());
    });
    crate::service::session_persist::stop_worker(&fixture.service).unwrap();
}

#[test]
fn concurrent_identical_bootstraps_each_submit_once_across_sibling_confirmation_and_sweep() {
    overlapping_bootstraps(false);
}

#[test]
fn concurrent_identical_bootstraps_each_submit_once_across_sibling_rollback_purge_and_sweep() {
    overlapping_bootstraps(true);
}
