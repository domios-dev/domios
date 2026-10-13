use super::*;
use crate::api::events::SystemEvent;

#[test]
fn tab_state_weak_hint_event_marks_it_non_actionable() {
    let event = super::weak_hint_event("tab-1", ERROR_LOOP, 0.72, 5);

    assert_eq!(event["tab_id"], "tab-1");
    assert_eq!(event["label"], ERROR_LOOP);
    assert_eq!(event["confidence"], 0.72);
    assert_eq!(event["band"], "weak");
    assert_eq!(event["actionable"], false);
    assert_eq!(event["episode"], 5);
}

fn metadata(options: Vec<NumberedPromptOption>) -> TabStateMetadata {
    TabStateMetadata {
        declared_safe_choice: None,
        tab_snapshot: None,
        process_tree_key: None,
        envelope: None,
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
fn tab_state_strong_trust_emits_orchestrator_event_with_options() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&tab_id).unwrap().pending_prompt = Some(codeterm_types::pty::PendingPrompt {
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
fn tab_state_strong_auth_emits_orchestrator_event() {
    let (mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let mut events = fixture.service.events.subscribe();

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

    let event = events
        .try_recv()
        .expect("Orchestrator auth event is published");
    let SystemEvent::Generic { event_type, data } = event.event else {
        panic!("tab-state auth signal is a generic Orchestrator event");
    };
    assert_eq!(event_type, "tab_state_auth_required");
    assert_eq!(data["tab_id"], tab_id);
    assert_eq!(data["label"], LOGIN_OR_AUTH_REQUIRED);
}

#[test]
fn tab_state_single_strong_hint_is_marked_non_actionable() {
    let (mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let mut events = fixture.service.events.subscribe();
    let launch_key = super::super::state::launch_episode_key(&tab_id, Some("pty-launch"), None);
    let action =
        super::super::state::apply_verdict(&tab_id, &launch_key, LOGIN_OR_AUTH_REQUIRED, 0.92);
    assert_eq!(action, SignalAction::WeakHint);

    deliver(
        &fixture.service,
        &tab_id,
        &metadata(Vec::new()),
        4,
        ObservationTrigger::WorkerIdle,
        LOGIN_OR_AUTH_REQUIRED,
        0.92,
        action,
    );

    let event = events.try_recv().expect("informational hint is published");
    let SystemEvent::Generic { event_type, data } = event.event else {
        panic!("tab-state hint is a generic event");
    };
    assert_eq!(event_type, "tab-state-signal");
    assert_eq!(data["actionable"], false);
}
