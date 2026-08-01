use super::*;

fn unbound_service_fixture(
    provider_id: &str,
) -> (crate::service::test_fixture::ServiceFixture, String) {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some(provider_id.into());
    tab.cwd = Some("/proj".into());
    tab.agent_set_at = Some(std::time::Instant::now());
    tab.session_binding_nonce = Some("binding-current".into());
    (
        crate::service::test_fixture::ServiceFixture::new(mux),
        tab_id,
    )
}

fn bind_fixture_session(
    fixture: &crate::service::test_fixture::ServiceFixture,
    tab_id: &str,
    session_id: &str,
) {
    let mut mux = fixture.service.mux.write();
    let tab = mux.get_tab_mut(tab_id).unwrap();
    tab.agent_session_id = Some(session_id.into());
    tab.reported_session_binding_nonce = Some("binding-current".into());
}

#[test]
fn chat_send_status_reconciles_detached_jsonl_turn_from_captured_anchor() {
    let (fixture, tab_id) = unbound_service_fixture("windowed");
    let root = fixture.temp.path().join("jsonl-evidence");
    std::fs::create_dir_all(&root).unwrap();
    let provider = WindowedJsonlProvider { root: root.clone() };
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(provider));
    let session_path = root.join("session-jsonl.jsonl");
    bind_fixture_session(&fixture, &tab_id, "session-jsonl");

    let evidence = fixture.service.chat_send_anchor(&tab_id);
    let send = fixture
        .service
        .chat_send_ledger
        .try_start_scoped(
            &tab_id,
            "detached exact turn",
            evidence.anchor,
            evidence.live_binding,
        )
        .unwrap();
    fixture
        .service
        .chat_send_ledger
        .finish(&send.send_id, Ok(()))
        .unwrap();
    std::fs::write(
        session_path,
        format!(
            "{}\n",
            serde_json::json!({ "role": "user", "content": "detached exact turn" })
        ),
    )
    .unwrap();

    fixture.service.reconcile_chat_send_evidence(&tab_id);
    assert_eq!(
        fixture.service.chat_send_ledger.reconcile(&tab_id)[0].state,
        ChatSendState::Sent
    );
}

#[test]
fn missing_jsonl_store_without_acknowledged_binding_stays_anchorless() {
    let (fixture, tab_id) = unbound_service_fixture("windowed");
    let root = fixture.temp.path().join("unacknowledged-jsonl-evidence");
    std::fs::create_dir_all(&root).unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(WindowedJsonlProvider { root }));
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .agent_session_id = Some("session-unacknowledged".into());

    assert!(fixture.service.chat_send_anchor(&tab_id).anchor.is_none());
}

#[test]
fn chat_send_status_reconciles_rebound_jsonl_turn_from_detected_session_anchor() {
    let (fixture, tab_id) = unbound_service_fixture("windowed");
    let root = fixture.temp.path().join("rebound-jsonl-evidence");
    std::fs::create_dir_all(&root).unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(WindowedJsonlProvider { root: root.clone() }));
    let session_path = root.join("session-rebound.jsonl");
    std::fs::write(
        &session_path,
        format!(
            "{}\n",
            serde_json::json!({ "role": "user", "content": "pre-rebind history" })
        ),
    )
    .unwrap();

    let evidence = fixture.service.chat_send_anchor(&tab_id);
    let send = fixture
        .service
        .chat_send_ledger
        .try_start_scoped(
            &tab_id,
            "rebound exact turn",
            evidence.anchor,
            evidence.live_binding,
        )
        .unwrap();
    fixture
        .service
        .chat_send_ledger
        .finish(&send.send_id, Ok(()))
        .unwrap();
    use std::io::Write;
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(session_path)
            .unwrap(),
        "{}",
        serde_json::json!({ "role": "user", "content": "rebound exact turn" })
    )
    .unwrap();
    bind_fixture_session(&fixture, &tab_id, "session-rebound");

    fixture.service.reconcile_chat_send_evidence(&tab_id);
    assert_eq!(
        fixture.service.chat_send_ledger.reconcile(&tab_id)[0].state,
        ChatSendState::Sent
    );
}

#[test]
fn chat_send_status_reconciles_detached_structured_turn_from_captured_anchor() {
    let (fixture, tab_id) = unbound_service_fixture("structured-store");
    let provider = StructuredStoreProvider::new();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(provider.clone()));

    let evidence = fixture.service.chat_send_anchor(&tab_id);
    let send = fixture
        .service
        .chat_send_ledger
        .try_start_scoped(
            &tab_id,
            "detached structured turn",
            evidence.anchor,
            evidence.live_binding,
        )
        .unwrap();
    fixture
        .service
        .chat_send_ledger
        .finish(&send.send_id, Ok(()))
        .unwrap();
    provider.append_user_row("session-structured", "detached structured turn");
    bind_fixture_session(&fixture, &tab_id, "session-structured");

    fixture.service.reconcile_chat_send_evidence(&tab_id);
    assert_eq!(
        fixture.service.chat_send_ledger.reconcile(&tab_id)[0].state,
        ChatSendState::Sent
    );
}

#[test]
fn structured_session_created_before_boundary_cannot_confirm() {
    let (fixture, tab_id) = unbound_service_fixture("structured-store");
    let provider = StructuredStoreProvider::new();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(provider.clone()));
    let evidence = fixture.service.chat_send_anchor(&tab_id);
    let send = fixture
        .service
        .chat_send_ledger
        .try_start_scoped(
            &tab_id,
            "stale structured turn",
            evidence.anchor,
            evidence.live_binding,
        )
        .unwrap();
    fixture
        .service
        .chat_send_ledger
        .finish(&send.send_id, Ok(()))
        .unwrap();
    provider.rows.lock().insert(
        "session-stale".into(),
        vec![crate::agent::chat_types::ChatMsg {
            id: "stale".into(),
            kind: "user".into(),
            content: "stale structured turn".into(),
            ..Default::default()
        }],
    );
    provider
        .created_at
        .lock()
        .insert("session-stale".into(), std::time::UNIX_EPOCH);
    bind_fixture_session(&fixture, &tab_id, "session-stale");

    fixture.service.reconcile_chat_send_evidence(&tab_id);
    assert_eq!(
        fixture.service.chat_send_ledger.reconcile(&tab_id)[0].state,
        ChatSendState::Unknown
    );
}

#[test]
fn chat_send_status_rejects_pre_send_foreign_and_claimed_turns() {
    let (fixture, tab_id) = unbound_service_fixture("windowed");
    let root = fixture.temp.path().join("inverse-jsonl-evidence");
    std::fs::create_dir_all(&root).unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(WindowedJsonlProvider { root: root.clone() }));
    let session_path = root.join("session-owned.jsonl");
    std::fs::write(
        &session_path,
        format!(
            "{}\n",
            serde_json::json!({ "role": "user", "content": "same exact turn" })
        ),
    )
    .unwrap();

    let first_evidence = fixture.service.chat_send_anchor(&tab_id);
    let second_evidence = fixture.service.chat_send_anchor(&tab_id);
    let first = fixture
        .service
        .chat_send_ledger
        .try_start_scoped(
            &tab_id,
            "same exact turn",
            first_evidence.anchor,
            first_evidence.live_binding,
        )
        .unwrap();
    let second = fixture
        .service
        .chat_send_ledger
        .try_start_scoped(
            &tab_id,
            "same exact turn",
            second_evidence.anchor,
            second_evidence.live_binding,
        )
        .unwrap();
    for send in [&first, &second] {
        fixture
            .service
            .chat_send_ledger
            .finish(&send.send_id, Ok(()))
            .unwrap();
    }
    bind_fixture_session(&fixture, &tab_id, "session-owned");
    fixture.service.reconcile_chat_send_evidence(&tab_id);
    assert_eq!(fixture.service.chat_send_ledger.reconcile(&tab_id).len(), 2);

    use std::io::Write;
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(session_path)
            .unwrap(),
        "{}",
        serde_json::json!({ "role": "user", "content": "same exact turn" })
    )
    .unwrap();
    {
        let mut mux = fixture.service.mux.write();
        mux.get_tab_mut(&tab_id).unwrap().agent_session_id = Some("session-foreign".into());
    }
    fixture.service.reconcile_chat_send_evidence(&tab_id);
    assert_eq!(fixture.service.chat_send_ledger.reconcile(&tab_id).len(), 2);

    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = Some("session-owned".into());
        tab.session_binding_nonce = Some("binding-stale".into());
    }
    fixture.service.reconcile_chat_send_evidence(&tab_id);
    assert_eq!(fixture.service.chat_send_ledger.reconcile(&tab_id).len(), 2);

    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .session_binding_nonce = Some("binding-current".into());
    fixture.service.reconcile_chat_send_evidence(&tab_id);
    let unresolved = fixture.service.chat_send_ledger.reconcile(&tab_id);
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].send_id, second.send_id);
    assert_eq!(unresolved[0].state, ChatSendState::Unknown);
    assert_eq!(
        fixture.service.chat_send_ledger.claimed_turn_count(
            &tab_id,
            "session-owned",
            "same exact turn"
        ),
        1
    );
    fixture.service.reconcile_chat_send_evidence(&tab_id);
    assert_eq!(
        fixture.service.chat_send_ledger.reconcile(&tab_id)[0].send_id,
        second.send_id
    );
}
