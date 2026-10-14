use super::*;

#[test]
fn restore_requires_content_and_exact_epoch_tab_thread_and_binding() {
    let expected = RuntimeContext {
        epoch: 3,
        tab_id: "own".into(),
        binding_nonce: "nonce".into(),
        thread_id: Some("thread".into()),
        ..Default::default()
    };
    assert!(should_restore(&expected, &expected, true, true));
    assert!(!should_restore(&expected, &expected, false, true));
    assert!(!should_restore(&expected, &expected, true, false));
    for current in [
        RuntimeContext {
            epoch: 4,
            ..expected.clone()
        },
        RuntimeContext {
            tab_id: "other".into(),
            ..expected.clone()
        },
        RuntimeContext {
            binding_nonce: "new".into(),
            ..expected.clone()
        },
        RuntimeContext {
            thread_id: Some("other".into()),
            ..expected.clone()
        },
        RuntimeContext {
            thread_id: None,
            ..expected.clone()
        },
    ] {
        assert!(!should_restore(&expected, &current, true, true));
    }
}

#[test]
fn compaction_claims_once_and_failure_holds_only_its_epoch() {
    let mut state = CompactionState::default();
    assert!(state.claim(3, "item"));
    assert!(!state.claim(3, "item"));
    state.finish(3, true);
    assert!(state.held(3));
    assert!(!state.held(4));
    state.finish(2, false);
    assert!(state.held(3));
    assert!(state.claim(3, "next"));
    state.finish(3, false);
    assert!(!state.held(3));
    state.finish(3, true);
    assert!(state.claim(4, "item"));
    assert!(state.held(4));
    state.finish(4, false);
    assert!(!state.held(4));
}

#[test]
fn restore_payload_preserves_opaque_instructions() {
    assert_eq!(
        restore_value(Some("opaque")),
        json!({"instructions":"opaque","additionalContext":[]})
    );
    assert_eq!(
        restore_value(None),
        json!({"instructions":null,"additionalContext":[]})
    );
}

#[tokio::test]
async fn mock_runtime_restores_once_off_reader_and_holds_failed_restoration() {
    use super::super::{fixtures, start};
    let mut scene = fixtures::scene();
    let session = scene.session.clone();
    let epoch = session.context.lock().epoch;
    let process = session.process.lock().clone().unwrap();
    start(
        scene.fixture.service.clone(),
        session.provider.clone(),
        session.clone(),
        process.clone(),
    );
    let task = schedule(&scene.fixture.service, &session, epoch, "thread", "item").unwrap();
    let request = fixtures::read(&mut scene.peer).await;
    assert_eq!(request["method"], "fixture/restore");
    assert_eq!(
        request["params"],
        restore_value(Some("opaque instructions"))
    );
    assert!(schedule(&scene.fixture.service, &session, epoch, "thread", "item").is_none());
    assert!(session.compaction.lock().held(epoch));
    fixtures::write(
        &mut scene.peer,
        json!({"id":"server-request","method":"fixture/unmapped","params":{}}),
    )
    .await;
    let reply = fixtures::read(&mut scene.peer).await;
    assert_eq!(reply["id"], "server-request");
    assert_eq!(reply["error"]["code"], -32601);
    fixtures::write(&mut scene.peer, json!({"id":request["id"],"result":{}})).await;
    tokio::time::timeout(std::time::Duration::from_millis(500), task)
        .await
        .unwrap()
        .unwrap();
    assert!(!session.compaction.lock().held(epoch));

    let task = schedule(&scene.fixture.service, &session, epoch, "thread", "next").unwrap();
    let request = fixtures::read(&mut scene.peer).await;
    fixtures::write(
        &mut scene.peer,
        json!({"id":request["id"],"error":{"code":-32000,"message":"opaque failure"}}),
    )
    .await;
    tokio::time::timeout(std::time::Duration::from_millis(500), task)
        .await
        .unwrap()
        .unwrap();
    assert!(session.compaction.lock().held(epoch));
    assert!(matches!(
        session.state.lock().phase,
        crate::service::agent_runtime::model::RuntimePhase::Degraded { .. }
    ));

    let tab = session.context.lock().tab_id.clone();
    let pending = scene
        .fixture
        .service
        .chat_send_ledger
        .try_start(&tab, "opaque input", None)
        .unwrap();
    let refused = crate::service::agent_runtime::transport::submit_loaded(
        &scene.fixture.service,
        &session,
        &process,
        &pending.send_id,
        codeterm_plugin::agent_runtime::TurnInput {
            text: "opaque input".into(),
            attachments: Vec::new(),
        },
    )
    .await;
    assert!(matches!(
        refused,
        Err(crate::service::delivery_error::DeliveryError::TabBusyRefused { .. })
    ));
    assert!(!scene
        .fixture
        .service
        .chat_send_ledger
        .has_rpc_handoff(&pending.send_id));

    let permit = session.gate.acquire().await.unwrap();
    let task = schedule(
        &scene.fixture.service,
        &session,
        epoch,
        "thread",
        "superseded",
    )
    .unwrap();
    session.context.lock().epoch += 1;
    drop(permit);
    tokio::time::timeout(std::time::Duration::from_millis(500), task)
        .await
        .unwrap()
        .unwrap();
    assert!(!session.compaction.lock().held(epoch + 1));
    assert!(scene.peer.buffer().is_empty());
    process.client.close(RuntimeFailure::Superseded);
}
