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
fn transcript_retry_user_cancellation_advances_append_boundary_and_fences_claimed_retry() {
    let (fixture, tab, path) = scene();
    let svc = &fixture.service;
    observe_at(svc, &tab, "own", "nonce", &[failure("first", 7)], 123);
    let queue = svc.ext::<DelayedSendQueue>();
    let message = queue.claim_due(10_123).unwrap().remove(0);
    std::fs::write(path, "opaque-older-error\n").unwrap();
    super::super::cancel(svc, &tab).unwrap();
    assert_eq!(skip_retry(svc, &tab, &message.id), Some(true));
    observe_at(svc, &tab, "own", "nonce", &[failure("unread-old", 8)], 123);
    assert!(states().lock()[&tab].retry.pending.is_none());
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
    assert!(fixture.ui.0.lock().iter().any(|(_, value)| value
        .to_string()
        .contains("Automatic retry stopped after 10 attempts")));
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
