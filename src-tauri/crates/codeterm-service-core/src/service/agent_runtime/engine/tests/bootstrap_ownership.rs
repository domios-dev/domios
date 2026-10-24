use super::super::*;
use super::sync_send::runtime_fixture;
use crate::multiplexer::types::{PendingDeliveryChannel, PendingDeliveryState};
use crate::service::chat_send_transaction::ChatSendState;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[test]
fn fresh_runtime_bootstrap_survives_identical_sibling_completion_before_launch() {
    const PAYLOAD: &str = "\topaque-bootstrap-\u{03bb}\r\n";
    let (fixture, second, peer, _cleanup) = runtime_fixture();
    let workspace = fixture
        .service
        .mux
        .read()
        .find_workspace_for_tab(&second)
        .unwrap();
    let first = fixture.service.mux.write().create_tab(&workspace).unwrap();
    let storage = fixture.install_seeded_storage_desk();
    let first_send = uuid::Uuid::new_v4().to_string();
    let second_send = uuid::Uuid::new_v4().to_string();
    for (tab, send) in [(&first, &first_send), (&second, &second_send)] {
        fixture
            .service
            .create_pending_delivery_with_send_id(
                tab,
                PendingDeliveryChannel::InteractiveInput,
                PAYLOAD,
                Some(send),
            )
            .unwrap();
    }
    let pending = |id: &str| {
        fixture
            .service
            .mux
            .read()
            .get_tab(id)
            .unwrap()
            .pending_delivery
            .clone()
            .unwrap()
    };
    let first_record = pending(&first);
    let second_record = pending(&second);
    assert_eq!(first_record.content_digest, second_record.content_digest);
    assert_ne!(first_record.record_id(), second_record.record_id());
    {
        let mux = fixture.service.mux.read();
        for id in [&first, &second] {
            assert!(!mux.get_tab(id).unwrap().restored_from_snapshot);
        }
    }

    fixture
        .service
        .reconcile_pending_delivery(&first, Ok(true))
        .unwrap();
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&first)
        .unwrap()
        .pending_delivery
        .is_none());
    assert!(!fixture
        .service
        .pending_delivery_payloads
        .path_for_test(&first_record)
        .exists());
    assert_eq!(pending(&second), second_record);
    assert_eq!(second_record.state(), PendingDeliveryState::Pending);
    assert!(second_record.attempt_provenance().is_none());

    let session = for_tab(&second).unwrap();
    let process = session.process.lock().clone().unwrap();
    let expected_send = second_send.clone();
    let peer = crate::async_runtime::spawn(async move {
        let mut peer = BufReader::new(peer);
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(2), peer.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(request["method"], "fixture/submit");
        assert_eq!(request["params"]["input"]["text"], PAYLOAD);
        assert_eq!(request["params"]["sendId"], expected_send);
        assert_eq!(request["params"]["threadId"], "own-thread");
        let response = serde_json::json!({"id":request["id"],"result":{"turnId":"bootstrap-turn"}});
        peer.get_mut()
            .write_all(format!("{response}\n").as_bytes())
            .await
            .unwrap();
        peer
    });
    crate::async_runtime::block_on(async {
        super::super::super::bootstrap::submit(&fixture.service, &session, &process)
            .await
            .unwrap();
        super::super::super::bootstrap::submit(&fixture.service, &session, &process)
            .await
            .unwrap();
    });
    let mut peer = crate::async_runtime::block_on(peer).unwrap().into_inner();
    let claimed = pending(&second);
    assert_eq!(claimed.record_id(), second_record.record_id());
    assert_eq!(claimed.send_id(), Some(second_send.as_str()));
    assert_eq!(claimed.state(), PendingDeliveryState::Unconfirmed);
    assert!(claimed.attempt_provenance().is_some());
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&second_send)
            .unwrap()
            .state,
        ChatSendState::Written
    );

    use std::{
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    use tokio::io::AsyncRead;
    let mut bytes = [0; 1];
    let mut buffer = tokio::io::ReadBuf::new(&mut bytes);
    assert!(matches!(
        Pin::new(&mut peer).poll_read(&mut Context::from_waker(Waker::noop()), &mut buffer),
        Poll::Pending
    ));
    crate::service::session_persist::stop_worker(&fixture.service).unwrap();
    let snapshot = crate::session::store::StorageDeskAdapter::from_storage(storage)
        .snapshot()
        .unwrap();
    let persisted = crate::multiplexer::state::MultiplexerState::from_session_snapshot(&snapshot);
    assert!(persisted
        .get_tab(&first)
        .unwrap()
        .pending_delivery
        .is_none());
    assert_eq!(
        persisted
            .get_tab(&second)
            .unwrap()
            .pending_delivery
            .as_ref(),
        Some(&claimed)
    );
}
