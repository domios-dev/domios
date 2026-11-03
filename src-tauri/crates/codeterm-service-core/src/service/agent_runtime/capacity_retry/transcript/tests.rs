use super::*;
use crate::service::test_fixture::ServiceFixture;
use codeterm_provider_signals::SessionObservations;

fn scene() -> (ServiceFixture, String, std::path::PathBuf) {
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.create_workspace("Fixture".into(), None).id;
    let tab = mux.create_tab(&workspace).unwrap();
    let target = mux.get_tab_mut(&tab).unwrap();
    target.agent_session_id = Some("own".into());
    target.session_binding_nonce = Some("nonce".into());
    let fixture = ServiceFixture::new(mux);
    let path = fixture.temp.path().join("session.jsonl");
    std::fs::write(&path, "opaque\n").unwrap();
    register(&fixture.service, &tab, "own", "nonce", &path);
    (fixture, tab, path)
}

fn failure(id: &str, offset: u64) -> RichMessage {
    RichMessage {
        provider_session_id: Some("own".into()),
        uuid: Some(id.into()),
        record_start: Some(offset),
        provider_observations: Some(SessionObservations {
            retry_error: Some(RuntimeErrorSignal {
                retryable: false,
                retryable_kind: Some(RuntimeRetryableErrorKind::Transient),
                retry_at_ms: None,
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn transcript_retry_ignores_history_foreign_and_managed_then_schedules_one_continue() {
    let (fixture, tab, _) = scene();
    let svc = &fixture.service;
    let mut foreign = failure("foreign", 7);
    foreign.provider_session_id = Some("foreign".into());
    let mut managed = failure("managed", 7);
    managed
        .provider_observations
        .as_mut()
        .unwrap()
        .retry_error
        .as_mut()
        .unwrap()
        .retryable = true;
    observe_at(
        svc,
        &tab,
        "own",
        "nonce",
        &[failure("old", 0), foreign, managed],
        123,
    );
    assert!(svc.ext::<DelayedSendQueue>().list(Some(&tab)).is_empty());
    observe_at(
        svc,
        &tab,
        "own",
        "nonce",
        &[failure("new", 7), failure("new", 7)],
        123,
    );
    let messages = svc.ext::<DelayedSendQueue>().list(Some(&tab));
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].text, "continue");
    assert_eq!(messages[0].scheduled_at_ms, 10_123);
    retire(svc, &tab);
}

#[test]
fn transcript_retry_user_cancellation_deduplicates_evidence_and_fences_claimed_retry() {
    let (fixture, tab, _) = scene();
    let svc = &fixture.service;
    observe_at(svc, &tab, "own", "nonce", &[failure("first", 7)], 123);
    let queue = svc.ext::<DelayedSendQueue>();
    let message = queue.claim_due(10_123).unwrap().remove(0);
    super::super::cancel(svc, &tab).unwrap();
    assert_eq!(skip_retry(svc, &tab, &message.id), Some(true));
    observe_at(svc, &tab, "own", "nonce", &[failure("first", 7)], 123);
    assert!(states().lock()[&tab].retry.pending.is_none());
    let user = RichMessage {
        provider_session_id: Some("own".into()),
        uuid: Some("new-user".into()),
        record_start: Some(8),
        role: "user".into(),
        ..Default::default()
    };
    observe_at(svc, &tab, "own", "nonce", &[user], 123);
    assert!(states().lock()[&tab].busy);
    observe_at(svc, &tab, "own", "nonce", &[failure("second", 19)], 123);
    let pending = states().lock()[&tab].retry.pending.clone().unwrap();
    svc.mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .session_binding_nonce = Some("replacement".into());
    assert_eq!(skip_retry(svc, &tab, &pending), Some(true));
    retire(svc, &tab);
}

#[test]
fn transcript_retry_cap_is_visible_and_success_resets_count() {
    let (fixture, tab, _) = scene();
    let svc = &fixture.service;
    for n in 1..=10 {
        observe_at(
            svc,
            &tab,
            "own",
            "nonce",
            &[failure(&format!("error-{n}"), 7)],
            123,
        );
    }
    assert!(svc.ext::<DelayedSendQueue>().list(Some(&tab)).is_empty());
    assert!(fixture
        .ui
        .0
        .lock()
        .iter()
        .any(|(_, value)| value.to_string().contains("Automatic recovery stopped:")));
    let success = RichMessage {
        provider_session_id: Some("own".into()),
        uuid: Some("success".into()),
        record_start: Some(7),
        provider_observations: Some(SessionObservations {
            retry_succeeded: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    observe_at(
        svc,
        &tab,
        "own",
        "nonce",
        &[success, failure("after-success", 7)],
        123,
    );
    assert_eq!(states().lock()[&tab].retry.attempts, 1);
    retire(svc, &tab);
}

#[test]
fn output_limit_notice_is_emitted_once_only_after_recovery_stops() {
    let (fixture, tab, _) = scene();
    let svc = &fixture.service;
    {
        let mut mux = svc.mux.write();
        let target = mux.get_tab_mut(&tab).unwrap();
        target.role = Some(codeterm_types::agent::AgentRole::Worker);
        target.parent_tab_id = Some("orchestrator".into());
        target.task_binding = Some(crate::multiplexer::types::TaskBinding {
            task_id: Some("task-opaque".into()),
            attempt_id: Some("attempt-opaque".into()),
            ..Default::default()
        });
    }
    let mut events = svc.events.subscribe();
    observe_at(svc, &tab, "own", "nonce", &[failure("transient", 7)], 122);
    let mut row = failure("first", 7);
    row.provider_observations
        .as_mut()
        .unwrap()
        .retry_error
        .as_mut()
        .unwrap()
        .retryable_kind = Some(RuntimeRetryableErrorKind::OutputLimit);
    observe_at(svc, &tab, "own", "nonce", &[row.clone()], 123);
    let queued = svc.ext::<DelayedSendQueue>().list(Some(&tab));
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].scheduled_at_ms, 123);
    let notices = |events: &mut tokio::sync::broadcast::Receiver<codeterm_events::SeqEvent>| {
        let mut notices = Vec::new();
        while let Ok(event) = events.try_recv() {
            if let codeterm_events::SystemEvent::Generic { event_type, data } = event.event {
                if event_type == "automatic-recovery-stopped" {
                    notices.push(data);
                }
            }
        }
        notices
    };
    assert!(notices(&mut events).is_empty());
    row.uuid = Some("second".into());
    observe_at(svc, &tab, "own", "nonce", &[row.clone(), row.clone()], 124);
    row.uuid = Some("third".into());
    observe_at(svc, &tab, "own", "nonce", &[row], 125);
    assert!(svc.ext::<DelayedSendQueue>().list(Some(&tab)).is_empty());
    let notices = notices(&mut events);
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0]["parent_tab_id"], "orchestrator");
    assert_eq!(notices[0]["task_id"], "task-opaque");
    assert_eq!(notices[0]["attempt_id"], "attempt-opaque");
    assert_eq!(
        notices[0]["recovery"],
        format!("domios send --tab {tab} \"continue\"")
    );
    assert!(notices[0]["reason"]
        .as_str()
        .unwrap()
        .contains("output limit"));
    retire(svc, &tab);
}

#[test]
fn exact_bound_file_offsets_identify_uuidless_error_records() {
    let (fixture, tab, _) = scene();
    let mut row = failure("opaque", 7);
    row.provider_session_id = None;
    row.uuid = None;
    observe_at(
        &fixture.service,
        &tab,
        "own",
        "nonce",
        &[row.clone(), row],
        123,
    );
    assert_eq!(
        fixture
            .service
            .ext::<DelayedSendQueue>()
            .list(Some(&tab))
            .len(),
        1
    );
    retire(&fixture.service, &tab);
}
