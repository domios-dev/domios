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

fn set_fixture_session_without_report(
    fixture: &crate::service::test_fixture::ServiceFixture,
    tab_id: &str,
    session_id: &str,
) {
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(tab_id)
        .unwrap()
        .agent_session_id = Some(session_id.into());
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
fn chat_send_status_reconciles_unbound_jsonl_materialization_from_reported_session() {
    let (fixture, tab_id) = unbound_service_fixture("windowed");
    let root = fixture.temp.path().join("unbound-jsonl-materialization");
    std::fs::create_dir_all(&root).unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(WindowedJsonlProvider { root: root.clone() }));

    let evidence = fixture.service.chat_send_anchor(&tab_id);
    let anchor = evidence.anchor.as_ref().expect("materialization anchor");
    assert_eq!(anchor.session_id, None);
    let not_before_ms = match anchor.window {
        EvidenceWindow::JsonlSessionMaterialization { not_before_ms } => not_before_ms,
        ref other => panic!("expected session materialization boundary, got {other:?}"),
    };
    let send = fixture
        .service
        .chat_send_ledger
        .try_start_scoped(
            &tab_id,
            "unbound materialized turn",
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
        root.join("session-materialized.jsonl"),
        format!(
            "{}\n",
            serde_json::json!({
                "role": "user",
                "content": "unbound materialized turn",
                "timestamp": chrono::DateTime::from_timestamp_millis(not_before_ms as i64)
                    .unwrap()
                    .to_rfc3339(),
            })
        ),
    )
    .unwrap();

    set_fixture_session_without_report(&fixture, &tab_id, "session-materialized");
    fixture.service.reconcile_chat_send_evidence(&tab_id);
    assert_eq!(
        fixture.service.chat_send_ledger.reconcile(&tab_id)[0].state,
        ChatSendState::Unknown
    );
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .reported_session_binding_nonce = Some("binding-stale".into());
    fixture.service.reconcile_chat_send_evidence(&tab_id);
    assert_eq!(
        fixture.service.chat_send_ledger.reconcile(&tab_id)[0].state,
        ChatSendState::Unknown
    );

    bind_fixture_session(&fixture, &tab_id, "session-materialized");
    fixture.service.reconcile_chat_send_evidence(&tab_id);
    assert_eq!(
        fixture.service.chat_send_ledger.reconcile(&tab_id)[0].state,
        ChatSendState::Unknown
    );

    use std::io::Write;
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(root.join("session-materialized.jsonl"))
            .unwrap(),
        "{}",
        serde_json::json!({
            "role": "user",
            "content": "unbound materialized turn",
            "timestamp": chrono::DateTime::from_timestamp_millis(not_before_ms as i64 + 1)
                .unwrap()
                .to_rfc3339(),
        })
    )
    .unwrap();
    fixture.service.reconcile_chat_send_evidence(&tab_id);
    let confirmed = fixture.service.chat_send_ledger.reconcile(&tab_id);
    assert_eq!(confirmed[0].send_id, send.send_id);
    assert_eq!(confirmed[0].state, ChatSendState::Sent);
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
fn detected_session_requires_current_reported_binding_before_confirmation() {
    let (fixture, tab_id) = unbound_service_fixture("windowed");
    let root = fixture.temp.path().join("reported-binding-jsonl-evidence");
    std::fs::create_dir_all(&root).unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(WindowedJsonlProvider { root: root.clone() }));
    let session_path = root.join("session-detected.jsonl");
    std::fs::write(&session_path, "").unwrap();

    let evidence = fixture.service.chat_send_anchor(&tab_id);
    let send = fixture
        .service
        .chat_send_ledger
        .try_start_scoped(
            &tab_id,
            "reported exact turn",
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
            serde_json::json!({ "role": "user", "content": "reported exact turn" })
        ),
    )
    .unwrap();
    set_fixture_session_without_report(&fixture, &tab_id, "session-detected");

    fixture.service.reconcile_chat_send_evidence(&tab_id);
    assert_eq!(
        fixture.service.chat_send_ledger.reconcile(&tab_id)[0].state,
        ChatSendState::Unknown
    );
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .reported_session_binding_nonce = Some("binding-stale".into());
    fixture.service.reconcile_chat_send_evidence(&tab_id);
    assert_eq!(
        fixture.service.chat_send_ledger.reconcile(&tab_id)[0].state,
        ChatSendState::Unknown
    );

    bind_fixture_session(&fixture, &tab_id, "session-detected");
    fixture.service.reconcile_chat_send_evidence(&tab_id);
    let confirmed = fixture.service.chat_send_ledger.reconcile(&tab_id);
    assert_eq!(confirmed[0].send_id, send.send_id);
    assert_eq!(confirmed[0].state, ChatSendState::Sent);
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
    let not_before_ms = match evidence.anchor.as_ref().unwrap().window {
        EvidenceWindow::StructuredMaterialization { not_before_ms } => not_before_ms,
        ref other => panic!("expected materialization boundary, got {other:?}"),
    };
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
    provider.created_at.lock().insert(
        "session-structured".into(),
        std::time::UNIX_EPOCH + std::time::Duration::from_millis(not_before_ms + 1),
    );
    bind_fixture_session(&fixture, &tab_id, "session-structured");

    fixture.service.reconcile_chat_send_evidence(&tab_id);
    assert_eq!(
        fixture.service.chat_send_ledger.reconcile(&tab_id)[0].state,
        ChatSendState::Sent
    );
}

#[test]
fn structured_unbound_anchor_refuses_cross_cwd_detected_session() {
    let (fixture, tab_id) = unbound_service_fixture("structured-store");
    let provider = StructuredStoreProvider::new();
    provider.append_user_row("session-foreign-cwd", "foreign turn");
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(provider));

    let evidence = fixture
        .service
        .chat_send_anchor(&tab_id)
        .anchor
        .expect("deferred structured boundary remains available");
    assert_eq!(evidence.session_id, None);
    assert!(matches!(
        evidence.window,
        EvidenceWindow::StructuredMaterialization { .. }
    ));
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
fn structured_session_created_at_floored_boundary_cannot_confirm() {
    let (fixture, tab_id) = unbound_service_fixture("structured-store");
    let provider = StructuredStoreProvider::new();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(provider.clone()));
    let evidence = fixture.service.chat_send_anchor(&tab_id);
    let not_before_ms = match evidence.anchor.as_ref().unwrap().window {
        EvidenceWindow::StructuredMaterialization { not_before_ms } => not_before_ms,
        ref other => panic!("expected materialization boundary, got {other:?}"),
    };
    let send = fixture
        .service
        .chat_send_ledger
        .try_start_scoped(
            &tab_id,
            "equal millisecond turn",
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
        "session-equal".into(),
        vec![crate::agent::chat_types::ChatMsg {
            id: "equal".into(),
            kind: "user".into(),
            content: "equal millisecond turn".into(),
            ..Default::default()
        }],
    );
    provider.created_at.lock().insert(
        "session-equal".into(),
        std::time::UNIX_EPOCH + std::time::Duration::from_millis(not_before_ms),
    );
    bind_fixture_session(&fixture, &tab_id, "session-equal");

    fixture.service.reconcile_chat_send_evidence(&tab_id);
    assert_eq!(
        fixture.service.chat_send_ledger.reconcile(&tab_id)[0].state,
        ChatSendState::Unknown
    );
}

#[test]
fn identical_payloads_with_staggered_anchors_claim_distinct_turns() {
    let (fixture, tab_id) = unbound_service_fixture("windowed");
    let root = fixture.temp.path().join("staggered-jsonl-evidence");
    std::fs::create_dir_all(&root).unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(WindowedJsonlProvider { root: root.clone() }));
    let session_path = root.join("session-staggered.jsonl");
    std::fs::write(&session_path, "").unwrap();
    bind_fixture_session(&fixture, &tab_id, "session-staggered");

    let first_evidence = fixture.service.chat_send_anchor(&tab_id);
    let first = fixture
        .service
        .chat_send_ledger
        .try_start_scoped(
            &tab_id,
            "same staggered turn",
            first_evidence.anchor,
            first_evidence.live_binding,
        )
        .unwrap();
    fixture
        .service
        .chat_send_ledger
        .finish(&first.send_id, Ok(()))
        .unwrap();
    std::fs::write(
        &session_path,
        format!(
            "{}\n",
            serde_json::json!({ "role": "user", "content": "same staggered turn" })
        ),
    )
    .unwrap();

    let second_evidence = fixture.service.chat_send_anchor(&tab_id);
    let second = fixture
        .service
        .chat_send_ledger
        .try_start_scoped(
            &tab_id,
            "same staggered turn",
            second_evidence.anchor,
            second_evidence.live_binding,
        )
        .unwrap();
    fixture
        .service
        .chat_send_ledger
        .finish(&second.send_id, Ok(()))
        .unwrap();
    use std::io::Write;
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(session_path)
            .unwrap(),
        "{}",
        serde_json::json!({ "role": "user", "content": "same staggered turn" })
    )
    .unwrap();

    fixture.service.reconcile_chat_send_evidence(&tab_id);
    let confirmed = fixture.service.chat_send_ledger.reconcile(&tab_id);
    assert_eq!(confirmed.len(), 1);
    assert_eq!(confirmed[0].send_id, second.send_id);
    assert_eq!(confirmed[0].state, ChatSendState::Sent);
}

#[test]
fn identical_payloads_with_inverted_capture_and_start_order_claim_distinct_turns() {
    let (fixture, tab_id) = unbound_service_fixture("windowed");
    let root = fixture.temp.path().join("inverted-order-jsonl-evidence");
    std::fs::create_dir_all(&root).unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(WindowedJsonlProvider { root: root.clone() }));
    let session_path = root.join("session-inverted.jsonl");
    std::fs::write(&session_path, "").unwrap();
    bind_fixture_session(&fixture, &tab_id, "session-inverted");

    let loose_evidence = fixture.service.chat_send_anchor(&tab_id);
    std::fs::write(
        &session_path,
        format!(
            "{}\n",
            serde_json::json!({ "role": "user", "content": "same inverted turn" })
        ),
    )
    .unwrap();
    let restrictive_evidence = fixture.service.chat_send_anchor(&tab_id);

    let restrictive = fixture
        .service
        .chat_send_ledger
        .try_start_scoped(
            &tab_id,
            "same inverted turn",
            restrictive_evidence.anchor,
            restrictive_evidence.live_binding,
        )
        .unwrap();
    fixture
        .service
        .chat_send_ledger
        .finish(&restrictive.send_id, Ok(()))
        .unwrap();
    let loose = fixture
        .service
        .chat_send_ledger
        .try_start_scoped(
            &tab_id,
            "same inverted turn",
            loose_evidence.anchor,
            loose_evidence.live_binding,
        )
        .unwrap();
    fixture
        .service
        .chat_send_ledger
        .finish(&loose.send_id, Ok(()))
        .unwrap();

    use std::io::Write;
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(session_path)
            .unwrap(),
        "{}",
        serde_json::json!({ "role": "user", "content": "same inverted turn" })
    )
    .unwrap();

    fixture.service.reconcile_chat_send_evidence(&tab_id);
    let confirmed = fixture.service.chat_send_ledger.reconcile(&tab_id);
    assert_eq!(confirmed.len(), 1);
    assert_eq!(confirmed[0].send_id, loose.send_id);
    assert_eq!(confirmed[0].state, ChatSendState::Sent);
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
        fixture
            .service
            .chat_send_ledger
            .confirmed_evidence_claims(&tab_id, "session-owned", "same exact turn")
            .len(),
        1
    );
    fixture.service.reconcile_chat_send_evidence(&tab_id);
    assert_eq!(
        fixture.service.chat_send_ledger.reconcile(&tab_id)[0].send_id,
        second.send_id
    );
}
