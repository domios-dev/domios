use super::*;
use crate::service::{
    agent_runtime::capacity_retry::transcript,
    chat_send_transaction::{SendLiveBinding, SourceRecord, SourceRowRef},
    test_fixture::ServiceFixture,
};
use codeterm_plugin::provider::{
    AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery,
};
use codeterm_provider_signals::{RuntimeAccountLimit, SessionObservations};
use codeterm_types::agent::chat_types::RichMessage;

struct Provider;
#[async_trait::async_trait]
impl AgentProvider for Provider {
    fn id(&self) -> &str {
        "quota-fixture"
    }
    fn display_name(&self) -> &str {
        "Quota fixture"
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        panic!("must not launch")
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::PollShellPrompt {
            timeout: std::time::Duration::ZERO,
        }
    }
    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
    fn session_file_path(&self, _: &str, _: &str) -> Option<std::path::PathBuf> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        panic!("must not restart")
    }
    fn account_capability(
        &self,
    ) -> Option<codeterm_plugin::provider_account::storage::AccountCapability> {
        Some(
            serde_json::from_value(serde_json::json!({
                "homeEnvironment":"FIXTURE_ACCOUNT_HOME", "defaultHome":"~/.fixture",
                "secretEnvironment":[], "isolatedPaths":["auth.json"], "sharedPaths":[],
                "continuation":"unverified"
            }))
            .unwrap(),
        )
    }
}

fn user(
    svc: &ServiceContext,
    tab: &str,
    id: &str,
    offset: u64,
    path: &std::path::Path,
) -> RichMessage {
    let send = svc
        .chat_send_ledger
        .try_start_scoped(
            tab,
            id,
            None,
            Some(SendLiveBinding {
                session_id: Some("own".into()),
                binding_nonce: Some("nonce".into()),
            }),
        )
        .unwrap();
    admit_pty_write(svc, "exact-live-pty", Some(&send.send_id));
    let source_id = crate::service::tab_chat::resume::capture_incarnation(
        &svc.chat_registry.jsonl_source(tab),
        &crate::service::tab_chat::resume_cursor::ChatEpoch {
            source_session_id: "own".into(),
            session_binding_nonce: None,
        },
        path,
        offset,
        None,
    )
    .unwrap()
    .id;
    let source = SourceRowRef {
        source_session_id: "own".into(),
        record: SourceRecord::Jsonl {
            incarnation: source_id.to_string(),
            record_start: offset,
            record_row: 0,
        },
    };
    svc.chat_send_ledger
        .confirm_send_with_row(&send.send_id, Some(&source))
        .unwrap();
    RichMessage {
        provider_session_id: Some("own".into()),
        role: "user".into(),
        uuid: Some(id.into()),
        record_start: Some(offset),
        provider_observations: Some(SessionObservations {
            turn_id: Some(id.into()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn delayed_pty_quota_after_newer_input_cannot_switch_or_authorize_teardown() {
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.create_workspace("Fixture".into(), None).id;
    let tab = mux.create_tab(&workspace).unwrap();
    let target = mux.get_tab_mut(&tab).unwrap();
    target.agent_type = Some("quota-fixture".into());
    target.agent_session_id = Some("own".into());
    target.session_binding_nonce = Some("nonce".into());
    target.reported_session_binding_nonce = Some("nonce".into());
    target.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
    mux.link_pty(&tab, "exact-live-pty".into());
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    svc.agent_registry.register_provider(Box::new(Provider));
    let path = fixture.temp.path().join("own.jsonl");
    std::fs::write(&path, "").unwrap();
    transcript::register(svc, &tab, "own", "nonce", &path);
    std::fs::write(&path, "opaque\n".repeat(32)).unwrap();
    admit_pty_input(svc, &tab);
    let first = user(svc, &tab, "u1", 0, &path);
    transcript::observe(svc, &tab, "own", "nonce", &[first.clone()]);
    let witness = pty_input_witness(svc, &tab, "own", "nonce", &first, &path).unwrap();
    let expected = capture(svc, &tab).unwrap();
    // U2 is admitted and pinned to the same PTY before its user record is visible.
    let second = svc
        .chat_send_ledger
        .try_start(&tab, "opaque-u2", None)
        .unwrap();
    let mut payload = crate::service::messenger::AgentDeliveryPayload {
        durable_delivery: None,
        trace_id: None,
        observer: None,
        idempotency_key: None,
        expected_pty_id: None,
        expected_session_id: None,
        expected_binding_nonce: None,
        transport: crate::service::messenger::Messenger::from(
            crate::service::messenger::MessageEnvelope::from_codeterm(),
        )
        .framed_delivery("opaque-u2"),
    };
    let claim = svc.pty_ownership.acquire(&tab).unwrap();
    svc.pin_agent_delivery_binding(&claim, &mut payload)
        .unwrap();
    drop(claim);
    assert!(!witness.current(svc, &tab));
    let signal = RuntimeErrorSignal {
        retryable: false,
        retryable_kind: None,
        retry_at_ms: None,
        account_limit: Some(RuntimeAccountLimit { resets_at_ms: None }),
    };
    let late = RichMessage {
        provider_session_id: Some("own".into()),
        uuid: Some("late-u1-quota".into()),
        record_start: Some(5),
        provider_observations: Some(SessionObservations {
            turn_id: Some("u1".into()),
            retry_error: Some(signal.clone()),
            ..Default::default()
        }),
        ..Default::default()
    };
    transcript::observe(svc, &tab, "own", "nonce", &[late.clone(), late]);
    assert!(transcript_limit(
        svc,
        &tab,
        "own",
        "nonce",
        "u1",
        &signal,
        Some(&witness)
    ));
    assert!(!account_turn_boundary(svc, &tab, &expected, "u1"));
    let state = svc.ext::<State>();
    assert!(state.scheduler.lock().boundaries.is_empty());
    assert!(state.scheduler.lock().pending.is_empty());
    dispatch(svc, &tab);
    let current = svc.mux.read().get_tab(&tab).cloned().unwrap();
    assert_eq!(current.pty_id.as_deref(), Some("exact-live-pty"));
    assert!(expected.validate(&current, None).is_ok());
    assert_eq!(
        svc.chat_send_ledger.status(&second.send_id).unwrap().state,
        crate::service::chat_send_transaction::ChatSendState::Pending
    );
    assert!(!svc
        .ext::<crate::service::agent_lifecycle::RestartTracker>()
        .lock()
        .in_progress
        .contains(&tab));
    // Even the old terminal after a confirmed U2 row cannot adopt U2's generation.
    let second_row = user(svc, &tab, "u2", 10, &path);
    let second_witness = pty_input_witness(svc, &tab, "own", "nonce", &second_row, &path).unwrap();
    assert!(transcript_limit(
        svc,
        &tab,
        "own",
        "nonce",
        "u1",
        &signal,
        Some(&second_witness)
    ));
    assert!(state.scheduler.lock().boundaries.is_empty());
    // A non-ledger PTY input also permanently retires an otherwise current witness.
    assert!(second_witness.current(svc, &tab));
    admit_pty_input(svc, &tab);
    assert!(!second_witness.current(svc, &tab));
    assert!(pty_input_witness(svc, &tab, "own", "nonce", &first, &path).is_none());
    let mut uncorrelated = second_row;
    uncorrelated.provider_observations.as_mut().unwrap().turn_id = None;
    assert!(pty_input_witness(svc, &tab, "own", "nonce", &uncorrelated, &path).is_none());
    transcript::retire(svc, &tab);
}
