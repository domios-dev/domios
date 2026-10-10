use super::*;

fn service_fixture(provider_id: &str) -> (crate::service::test_fixture::ServiceFixture, String) {
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

fn bind_session(
    fixture: &crate::service::test_fixture::ServiceFixture,
    tab_id: &str,
    session_id: &str,
) {
    let mut mux = fixture.service.mux.write();
    let tab = mux.get_tab_mut(tab_id).unwrap();
    tab.agent_session_id = Some(session_id.into());
    tab.reported_session_binding_nonce = Some("binding-current".into());
}

fn register_windowed(
    fixture: &crate::service::test_fixture::ServiceFixture,
    root: &std::path::Path,
) {
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(WindowedJsonlProvider {
            root: root.to_path_buf(),
            parse_offsets: Arc::default(),
        }));
}

fn append_jsonl_user_turn(path: &std::path::Path, content: &str) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    writeln!(
        file,
        "{}",
        serde_json::json!({ "role": "user", "content": content })
    )
    .unwrap();
}

fn append_structured_user_row(provider: &StructuredStoreProvider, session_id: &str, content: &str) {
    provider
        .rows
        .lock()
        .entry(session_id.to_string())
        .or_default()
        .push(crate::agent::chat_types::ChatMsg {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "user".into(),
            content: content.into(),
            ..Default::default()
        });
}

fn start_unconfirmed_send(
    fixture: &crate::service::test_fixture::ServiceFixture,
    tab_id: &str,
    payload: &str,
) -> ChatSendStatus {
    let evidence = fixture.service.chat_send_anchor(tab_id);
    let send = fixture
        .service
        .chat_send_ledger
        .try_start_scoped(tab_id, payload, evidence.anchor, evidence.live_binding)
        .unwrap();
    fixture
        .service
        .chat_send_ledger
        .finish(&send.send_id, Ok(()))
        .unwrap()
}

fn state_of(
    fixture: &crate::service::test_fixture::ServiceFixture,
    send: &ChatSendStatus,
) -> ChatSendState {
    fixture
        .service
        .chat_send_ledger
        .status(&send.send_id)
        .unwrap()
        .state
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn detached_jsonl_turn_reconciles_only_past_the_captured_service_anchor() {
    let (fixture, tab_id) = service_fixture("windowed");
    let root = fixture.temp.path().join("jsonl-evidence");
    std::fs::create_dir_all(&root).unwrap();
    register_windowed(&fixture, &root);
    let session_path = root.join("session-jsonl.jsonl");
    append_jsonl_user_turn(&session_path, "detached exact turn");
    bind_session(&fixture, &tab_id, "session-jsonl");

    let send = start_unconfirmed_send(&fixture, &tab_id, "detached exact turn");
    fixture.service.reconcile_chat_send_evidence(&tab_id);
    assert_eq!(state_of(&fixture, &send), ChatSendState::Unknown);

    append_jsonl_user_turn(&session_path, "detached exact turn");
    fixture.service.reconcile_chat_send_evidence(&tab_id);
    assert_eq!(state_of(&fixture, &send), ChatSendState::Sent);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn missing_jsonl_store_mints_only_a_live_absence_witness() {
    let (fixture, tab_id) = service_fixture("windowed");
    let root = fixture.temp.path().join("missing-jsonl-evidence");
    std::fs::create_dir_all(&root).unwrap();
    register_windowed(&fixture, &root);
    bind_session(&fixture, &tab_id, "session-missing");

    let evidence = fixture.service.chat_send_anchor(&tab_id);

    assert!(evidence.anchor.is_none());
    assert!(evidence.absence_witness.is_some());
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn fresh_trace_without_delivery_anchor_registers_before_materialization() {
    let (fixture, tab_id) = service_fixture("windowed");
    let root = fixture.temp.path().join("fresh-trace-source");
    std::fs::create_dir_all(&root).unwrap();
    register_windowed(&fixture, &root);
    bind_session(&fixture, &tab_id, "fresh-session");
    let evidence = fixture.service.chat_send_anchor(&tab_id);
    assert!(evidence.anchor.is_none());
    assert!(evidence.absence_witness.is_some());
    let events = crate::flow_trace::test_capture::capture(|| {
        let flow = crate::flow_trace::PtyFlow::start(None, &tab_id, "windowed").unwrap();
        flow.watch(
            &fixture.service,
            evidence.anchor.as_ref(),
            Some("binding-current"),
            "opaque",
            true,
        );
    });
    crate::flow_trace::cancel_tab(&tab_id);
    assert!(!root.join("fresh-session.jsonl").exists());
    assert!(events.iter().any(|event| event.stage == "trace.observer"
        && event.outcome == "registered_before_materialization"));
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn unbound_tab_never_adopts_a_foreign_structured_session_as_its_anchor() {
    let (fixture, tab_id) = service_fixture("structured-store");
    let provider = StructuredStoreProvider::new();
    append_structured_user_row(&provider, "session-foreign", "foreign turn");
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(provider));

    assert!(fixture.service.chat_send_anchor(&tab_id).anchor.is_none());
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn detached_structured_turn_reconciles_only_past_the_captured_service_anchor() {
    let (fixture, tab_id) = service_fixture("structured-store");
    let provider = StructuredStoreProvider::new();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(provider.clone()));
    append_structured_user_row(&provider, "session-structured", "detached structured turn");
    bind_session(&fixture, &tab_id, "session-structured");

    let send = start_unconfirmed_send(&fixture, &tab_id, "detached structured turn");
    fixture.service.reconcile_chat_send_evidence(&tab_id);
    assert_eq!(state_of(&fixture, &send), ChatSendState::Unknown);

    append_structured_user_row(&provider, "session-structured", "detached structured turn");
    fixture.service.reconcile_chat_send_evidence(&tab_id);
    assert_eq!(state_of(&fixture, &send), ChatSendState::Sent);
}

#[test]
fn identical_payloads_with_staggered_anchors_claim_distinct_turns() {
    let fx = Fixture::new();
    let first = fx
        .ledger
        .try_start(
            "tab-1",
            "same staggered turn",
            Some(fx.anchor_now("sess-a")),
        )
        .unwrap();
    fx.ledger.finish(&first.send_id, Ok(())).unwrap();
    fx.append_user_turn("sess-a", "same staggered turn");
    let second = fx
        .ledger
        .try_start(
            "tab-1",
            "same staggered turn",
            Some(fx.anchor_now("sess-a")),
        )
        .unwrap();
    fx.ledger.finish(&second.send_id, Ok(())).unwrap();

    let confirmed = fx.scan("tab-1");
    assert_eq!(confirmed.len(), 1);
    assert_eq!(confirmed[0].send_id, first.send_id);
    assert!(
        fx.scan("tab-1").is_empty(),
        "the pre-boundary turn is not the second send's"
    );

    fx.append_user_turn("sess-a", "same staggered turn");
    let confirmed = fx.scan("tab-1");
    assert_eq!(confirmed.len(), 1);
    assert_eq!(confirmed[0].send_id, second.send_id);
    assert!(fx.scan("tab-1").is_empty());
}

#[test]
fn identical_payloads_with_inverted_capture_and_start_order_claim_distinct_turns() {
    let fx = Fixture::new();
    let loose_anchor = fx.anchor_now("sess-a");
    fx.append_user_turn("sess-a", "same inverted turn");
    let restrictive_anchor = fx.anchor_now("sess-a");
    let restrictive = fx
        .ledger
        .try_start("tab-1", "same inverted turn", Some(restrictive_anchor))
        .unwrap();
    let loose = fx
        .ledger
        .try_start("tab-1", "same inverted turn", Some(loose_anchor))
        .unwrap();
    for send in [&restrictive, &loose] {
        fx.ledger.finish(&send.send_id, Ok(())).unwrap();
    }
    fx.append_user_turn("sess-a", "same inverted turn");

    let confirmed = fx
        .scan("tab-1")
        .into_iter()
        .map(|status| status.send_id)
        .collect::<Vec<_>>();

    assert_eq!(confirmed, vec![restrictive.send_id, loose.send_id]);
    assert!(fx.scan("tab-1").is_empty());
}

#[test]
fn staggered_windows_never_assign_one_turn_to_two_sends() {
    let fx = Fixture::new();
    let first = fx
        .ledger
        .try_start("tab-1", "same text", Some(fx.anchor_now("sess-a")))
        .unwrap();
    fx.ledger.finish(&first.send_id, Ok(())).unwrap();
    let second = fx
        .ledger
        .try_start("tab-1", "same text", Some(fx.anchor_now("sess-a")))
        .unwrap();
    fx.ledger.finish(&second.send_id, Ok(())).unwrap();
    fx.append_user_turn("sess-a", "same text");
    let third = fx
        .ledger
        .try_start("tab-1", "same text", Some(fx.anchor_now("sess-a")))
        .unwrap();
    fx.ledger.finish(&third.send_id, Ok(())).unwrap();
    fx.append_user_turn("sess-a", "same text");

    let confirmed = fx
        .scan("tab-1")
        .into_iter()
        .map(|status| status.send_id)
        .collect::<Vec<_>>();

    assert_eq!(confirmed, vec![first.send_id, second.send_id]);
    assert_eq!(
        fx.ledger.status(&third.send_id).unwrap().state,
        ChatSendState::Unknown
    );
}
