use super::*;
use crate::api::events::SystemEvent;

fn metadata(options: Vec<NumberedPromptOption>) -> TabStateMetadata {
    TabStateMetadata {
        context: Value::Null,
        session_key: "session".into(),
        title: "worker".into(),
        spawning_tab_id: None,
        parent_tab_id: None,
        plan_id: None,
        task_id: None,
        attempt_id: None,
        prompt_options: options,
    }
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn tab_state_strong_trust_emits_orchestrator_event_with_options() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&tab_id).unwrap().pending_prompt =
        Some(codeterm_types::pty::PendingPrompt {
            question: "opaque prompt".into(),
            options: vec![codeterm_types::pty::PromptOption {
                label: "Allow once".into(),
                value: "2".into(),
            }],
            sections: None,
            multi_select: None,
            submit_available: None,
            selected_values: None,
        });
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let mut events = fixture.service.events.subscribe();
    let options = vec![NumberedPromptOption {
        number: 2,
        label: "Allow once".into(),
        value: "2".into(),
    }];

    deliver(
        &fixture.service,
        &tab_id,
        &metadata(options),
        3,
        ObservationTrigger::BusyNoOutput,
        TRUST_OR_PERMISSION_PROMPT,
        0.91,
        SignalAction::StrongAction,
    );

    let event = events.try_recv().expect("Orchestrator signal is published");
    let SystemEvent::Generic { event_type, data } = event.event else {
        panic!("tab-state emits a generic Orchestrator event");
    };
    assert_eq!(event_type, "tab_state_prompt_required");
    assert_eq!(data["pending_options"][0]["number"], 2);
    assert_eq!(data["pending_options"][0]["value"], "2");
    assert_eq!(data["tab_id"], tab_id);
    let mux = fixture.service.mux.read();
    assert!(mux
        .get_tab(&tab_id)
        .is_some_and(|tab| tab.pending_prompt.is_some()));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn tab_state_strong_auth_notifies_owner_action_needed() {
    let (mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);

    deliver(
        &fixture.service,
        &tab_id,
        &metadata(Vec::new()),
        4,
        ObservationTrigger::WorkerIdle,
        LOGIN_OR_AUTH_REQUIRED,
        0.92,
        SignalAction::StrongAction,
    );

    let store = fixture.service.notif.lock();
    let records = store.list(false);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].level, Some(NotifyLevel::ActionNeeded));
    assert!(records[0].body.as_deref().is_some_and(|body| body.contains(&tab_id)));
}
