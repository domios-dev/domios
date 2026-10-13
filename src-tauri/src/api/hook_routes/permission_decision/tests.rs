use super::*;

#[test]
fn claude_permission_hook_is_off_by_default_and_requires_explicit_boolean() {
    let mut config = codeterm_types::config::schema::ProviderConfig::default();
    assert!(!enabled(None));
    assert!(!enabled(Some(&config)));
    config
        .extra
        .insert(ENABLED_KEY.into(), toml::Value::String("true".into()));
    assert!(!enabled(Some(&config)));
    config
        .extra
        .insert(ENABLED_KEY.into(), toml::Value::Boolean(true));
    assert!(enabled(Some(&config)));
}

fn capability() -> PermissionHookCapability {
    PermissionHookCapability {
        protocol: "claude-permission-request".into(),
        options: vec![
            PromptOption {
                label: "opaque-a".into(),
                value: "allow".into(),
            },
            PromptOption {
                label: "opaque-b".into(),
                value: "deny".into(),
            },
        ],
    }
}

fn request(id: &str) -> (Request, oneshot::Receiver<Answer>) {
    let (sender, receiver) = oneshot::channel();
    let capability = capability();
    (
        Request {
            id: id.into(),
            session_id: "session".into(),
            nonce: "nonce".into(),
            prompt: prompt("tool", &json!({}), id, &capability),
            answers: answers(&capability).unwrap(),
            sender: Some(sender),
        },
        receiver,
    )
}

#[test]
fn claude_permission_hook_notification_and_missing_event_are_not_decisions() {
    for body in [
        json!({"hook_event_name":"Notification","tool_name":"Bash"}),
        json!({"tool_name":"Bash"}),
        json!({"hook_event_name":"PreToolUse"}),
    ] {
        assert!(!is_permission_request(&body));
    }
    assert!(is_permission_request(
        &json!({"hook_event_name":"PermissionRequest"})
    ));
    assert_eq!(decision(None), json!({}));
}

#[tokio::test]
async fn claude_permission_hook_owner_allow_and_deny_return_vendor_decisions() {
    for (value, expected) in [("1", Answer::Allow), ("permission:request:1", Answer::Deny)] {
        let mut broker = Broker::default();
        let (request, receiver) = request("request");
        assert!(broker.register("tab", request));
        assert_eq!(
            broker.answer("tab", Some("session"), Some("nonce"), value),
            Some(Ok(()))
        );
        let answer = wait_for_answer(receiver, Duration::ZERO).await;
        assert_eq!(answer, Some(expected));
        assert_eq!(
            decision(answer),
            json!({"hookSpecificOutput": {
                "hookEventName":"PermissionRequest", "decision":{"behavior":
                    if expected == Answer::Allow { "allow" } else { "deny" }}
            }})
        );
    }
}

#[tokio::test]
async fn claude_permission_hook_deadline_and_close_never_invent_an_answer() {
    let mut broker = Broker::default();
    let (pending, receiver) = request("deadline");
    broker.register("tab", pending);
    assert_eq!(wait_for_answer(receiver, Duration::ZERO).await, None);
    assert!(broker
        .answer("tab", Some("session"), Some("nonce"), "1")
        .unwrap()
        .is_err());
    broker.remove("tab", "deadline");
    let (pending, receiver) = request("close");
    broker.register("tab", pending);
    broker.remove("tab", "close");
    assert_eq!(wait_for_answer(receiver, Duration::ZERO).await, None);
}

#[tokio::test]
async fn claude_permission_hook_first_answer_wins_and_foreign_binding_cannot_allow() {
    let mut broker = Broker::default();
    let (pending, receiver) = request("first");
    broker.register("tab", pending);
    assert_eq!(
        broker.answer("tab", Some("session"), Some("nonce"), "2"),
        Some(Ok(()))
    );
    assert!(broker
        .answer("tab", Some("session"), Some("nonce"), "1")
        .unwrap()
        .is_err());
    assert_eq!(receiver.await.unwrap(), Answer::Deny);
    broker.remove("tab", "first");
    let (pending, receiver) = request("foreign");
    broker.register("tab", pending);
    assert!(broker
        .answer("tab", Some("other-session"), Some("nonce"), "1")
        .unwrap()
        .is_err());
    assert_eq!(receiver.await.ok(), None);
}

#[test]
fn claude_permission_hook_identity_is_request_specific_and_cleanup_is_scoped() {
    let (first, _first_receiver) = request("first");
    let (second, _second_receiver) = request("second");
    let first_identity = PromptIdentity::new(PromptKind::Structured, &first.prompt);
    let second_identity = PromptIdentity::new(PromptKind::Structured, &second.prompt);
    assert_ne!(first_identity, second_identity);
    let mut broker = Broker::default();
    assert!(broker.register("tab", first));
    assert!(broker.register("tab", second));
    assert!(broker.remove("tab", "second").is_some());
    assert_eq!(broker.requests["tab"].len(), 1);
    assert!(broker.remove("tab", "first").is_some());
}

#[test]
fn claude_permission_hook_concurrent_requests_answer_independently_without_bell_entries() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::test_fixture::ServiceFixture;
    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = Some("session".into());
        tab.session_binding_nonce = Some("nonce".into());
    }
    let mut pending = Vec::new();
    for id in ["first", "second"] {
        let (request, receiver) = request(id);
        let prompt = request.prompt.clone();
        assert!(BROKER.lock().register(&tab_id, request));
        let guard = PendingGuard {
            svc: std::sync::Arc::new(fixture.service.clone()),
            tab_id: tab_id.clone(),
            id: id.into(),
            identity: PromptIdentity::new(PromptKind::Structured, &prompt),
        };
        sync_display(&fixture.service, &tab_id);
        let notice = notify(&fixture.service, &tab_id, "opaque", &prompt, id);
        pending.push((guard, receiver, prompt, notice));
    }
    let (second, mut second_receiver, second_prompt, second_notice) = pending.pop().unwrap();
    let (first, mut first_receiver, first_prompt, first_notice) = pending.pop().unwrap();
    assert_ne!(first_notice, second_notice);
    assert!(fixture.service.notif.lock().list(false).is_empty());
    assert!(respond_with_identity(
        &fixture.service,
        &tab_id,
        &second_prompt.options[1].value,
        Some(&PromptIdentity::new(PromptKind::Structured, &second_prompt))
    )
    .unwrap()
    .is_ok());
    assert_eq!(second_receiver.try_recv().unwrap(), Answer::Deny);
    assert!(first_receiver.try_recv().is_err());
    drop(second);
    assert_eq!(pending_for_tab(&tab_id), Some(first_prompt.clone()));
    assert!(respond_with_identity(
        &fixture.service,
        &tab_id,
        &first_prompt.options[0].value,
        Some(&PromptIdentity::new(PromptKind::Structured, &first_prompt))
    )
    .unwrap()
    .is_ok());
    assert_eq!(first_receiver.try_recv().unwrap(), Answer::Allow);
    drop(first);
    assert!(pending_for_tab(&tab_id).is_none());
    assert!(fixture.service.notif.lock().list(false).is_empty());
}

#[test]
fn claude_permission_hook_unknown_or_ambiguous_capability_is_unsupported() {
    let mut declared = capability();
    declared.protocol = "unknown".into();
    assert!(answers(&declared).is_none());
    declared.protocol = "claude-permission-request".into();
    declared.options[1].value = "allow".into();
    assert!(answers(&declared).is_none());
    declared.options[1].value = "deny".into();
    declared.options.reverse();
    assert_eq!(answers(&declared), Some(vec![Answer::Deny, Answer::Allow]));
}

#[test]
fn claude_permission_hook_tab_answer_uses_structured_route_without_a_bell_entry() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::test_fixture::ServiceFixture;
    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let (pending, mut receiver) = request("routed");
    let prompt = pending.prompt.clone();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = Some("session".into());
        tab.session_binding_nonce = Some("nonce".into());
    }
    assert!(BROKER.lock().register(&tab_id, pending));
    let identity = PromptIdentity::new(PromptKind::Structured, &prompt);
    let _guard = PendingGuard {
        svc: std::sync::Arc::new(fixture.service.clone()),
        tab_id: tab_id.clone(),
        id: "routed".into(),
        identity: identity.clone(),
    };
    fixture
        .service
        .push_structured_prompt(&tab_id, Some(prompt.clone()));
    let notification_id = notify(&fixture.service, &tab_id, "opaque", &prompt, "routed");
    assert!(respond_with_identity(
        &fixture.service,
        &tab_id,
        &prompt.options[0].value,
        Some(&identity)
    )
    .unwrap()
    .is_ok());
    assert_eq!(receiver.try_recv().unwrap(), Answer::Allow);
    drop(_guard);
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .pending_prompt
        .is_none());
    assert!(fixture
        .service
        .notif
        .lock()
        .notification(&notification_id)
        .is_none());
}

#[tokio::test]
async fn claude_permission_hook_timeout_and_provider_clear_retire_request_without_decision_or_bell_entry(
) {
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::test_fixture::ServiceFixture;

    for provider_cleared in [false, true] {
        let (mux, tab_id) = MultiplexerState::new();
        let fixture = ServiceFixture::new(mux);
        let (pending, receiver) = request("unanswered");
        let prompt = pending.prompt.clone();
        let identity = PromptIdentity::new(PromptKind::Structured, &prompt);
        assert!(BROKER.lock().register(&tab_id, pending));
        let guard = PendingGuard {
            svc: std::sync::Arc::new(fixture.service.clone()),
            tab_id: tab_id.clone(),
            id: "unanswered".into(),
            identity,
        };
        fixture
            .service
            .push_structured_prompt(&tab_id, Some(prompt.clone()));
        let notification_id = notify(&fixture.service, &tab_id, "opaque", &prompt, "unanswered");
        if provider_cleared {
            fixture.service.clear_pending_prompt(&tab_id);
        }
        assert_eq!(
            decision(wait_for_answer(receiver, Duration::ZERO).await),
            json!({})
        );
        drop(guard);
        assert!(pending_for_tab(&tab_id).is_none());
        assert!(fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .pending_prompt
            .is_none());
        assert!(fixture
            .service
            .notif
            .lock()
            .notification(&notification_id)
            .is_none());
    }
}

#[test]
fn claude_permission_hook_numeric_reply_requires_displayed_hook_identity() {
    let (pending, _receiver) = request("visible");
    let identity = PromptIdentity::new(PromptKind::Structured, &pending.prompt);
    let mut foreign = identity.clone();
    foreign.options[0].value = "foreign-a".into();
    foreign.options[1].value = "foreign-b".into();
    assert_eq!(response_identity("1", None, None), None);
    assert_eq!(response_identity("1", None, Some(&foreign)), None);
    assert_eq!(
        response_identity("1", None, Some(&identity)),
        Some(Some(&identity))
    );
    assert_eq!(
        response_identity("1", Some(&foreign), Some(&identity)),
        None
    );
    assert_eq!(
        response_identity("permission:visible:0", None, Some(&foreign)),
        Some(None)
    );
    assert_eq!(response_identity("foreign-a", None, Some(&identity)), None);
}

#[tokio::test]
async fn claude_permission_hook_cancelling_one_request_preserves_other_choice() {
    let mut broker = Broker::default();
    let (first, first_receiver) = request("cancelled");
    let (second, second_receiver) = request("survivor");
    broker.register("tab", first);
    broker.register("tab", second);
    broker.remove("tab", "cancelled");
    assert_eq!(first_receiver.await.ok(), None);
    assert!(broker
        .answer(
            "tab",
            Some("session"),
            Some("nonce"),
            "permission:cancelled:0"
        )
        .unwrap()
        .is_err());
    assert_eq!(
        broker.answer(
            "tab",
            Some("session"),
            Some("nonce"),
            "permission:survivor:1"
        ),
        Some(Ok(()))
    );
    assert_eq!(second_receiver.await.unwrap(), Answer::Deny);
}
