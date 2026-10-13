use super::*;
use std::time::Duration;

#[test]
fn launch_waits_for_owner_prompt_without_spawn_failed() {
    owner_prompt_readiness_round_trip("owner-prompt-readiness");
}

#[test]
fn answered_owner_prompt_resumes_normal_readiness() {
    owner_prompt_readiness_round_trip("answered-owner-prompt");
}

fn owner_prompt_readiness_round_trip(prefix: &str) {
    let attempt = LaunchAttempt {
        tab_id: format!("{prefix}-tab"),
        pty_id: format!("{prefix}-pty"),
        binding_nonce: format!("{prefix}-nonce"),
    };
    owner_prompt_pending_until(
        &attempt.tab_id,
        "prompt-token-7",
        Instant::now() + Duration::from_secs(30),
    );
    arm(attempt.clone());

    let (changed, events) = std::sync::mpsc::channel();
    let observer_attempt = attempt.clone();
    let observer = std::thread::spawn(move || {
        observe_with_owner_prompt(
            &observer_attempt,
            Instant::now() + Duration::from_secs(30),
            |prompt| {
                changed
                    .send(prompt.map(|_| BootStage::AwaitingOwnerPrompt))
                    .unwrap()
            },
        )
    });
    assert_eq!(events.recv().unwrap(), Some(BootStage::AwaitingOwnerPrompt));
    owner_prompt_cleared(&attempt.tab_id);
    assert_eq!(events.recv().unwrap(), None);
    assert!(note_acknowledged(
        &attempt.tab_id,
        &attempt.pty_id,
        &attempt.binding_nonce
    ));
    assert_eq!(
        observer.join().unwrap(),
        LaunchObservation::Evidence(LaunchEvidence::Acknowledged)
    );
}

#[test]
fn auto_answered_prompt_does_not_suspend_readiness() {
    use crate::multiplexer::types::{PendingPrompt, PromptOption};

    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let tab_id = mux.create_tab(&workspace_id).unwrap();
    let prompt = PendingPrompt {
        question: "opaque-question".into(),
        options: vec![PromptOption {
            label: "opaque-option".into(),
            value: "opaque-value".into(),
        }],
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    };
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.pending_prompt = Some(prompt);
        tab.pending_prompt_kind = Some(codeterm_types::pty::PromptKind::Provider);
    }
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    owner_prompt_pending(&tab_id, "opaque-question");
    fixture.service.retract_surfaced_prompt(&tab_id);

    let attempt = LaunchAttempt {
        tab_id: tab_id.clone(),
        pty_id: "auto-prompt-readiness-pty".into(),
        binding_nonce: "auto-prompt-readiness-nonce".into(),
    };
    arm(attempt.clone());
    assert!(!owner_prompt_is_active(&tab_id));
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .is_some_and(|tab| tab.pending_prompt.is_none()));
    assert_eq!(
        observe_with_owner_prompt(&attempt, Instant::now(), |prompt| assert!(prompt.is_none())),
        LaunchObservation::Deadline
    );
}

#[test]
fn startup_supervision_resumes_when_owner_prompt_clears() {
    let tab_id = "startup-owner-prompt-tab";
    owner_prompt_pending(tab_id, "prompt-token-8");
    let waiter_tab = tab_id.to_string();
    let waiter = std::thread::spawn(move || await_owner_prompt_resolution(&waiter_tab));
    owner_prompt_cleared(tab_id);
    assert_eq!(waiter.join().unwrap(), None);
}

#[test]
fn unanswered_owner_prompt_fails_at_owner_deadline_with_named_reason() {
    let attempt = LaunchAttempt {
        tab_id: "unanswered-owner-prompt-tab".into(),
        pty_id: "unanswered-owner-prompt-pty".into(),
        binding_nonce: "unanswered-owner-prompt-nonce".into(),
    };
    arm(attempt.clone());
    let question = "prompt-token-7";
    owner_prompt_pending_until(&attempt.tab_id, question, Instant::now());
    let failure =
        observe_with_owner_prompt(&attempt, Instant::now() + Duration::from_secs(30), |_| {});
    let LaunchObservation::OwnerDeadline {
        question: failed_question,
    } = failure
    else {
        panic!("expected owner prompt deadline failure");
    };
    assert!(owner_prompt_deadline_reason(&failed_question).contains(question));
    owner_prompt_cleared(&attempt.tab_id);
}

#[test]
fn owner_prompt_over_a_ready_launch_restores_agent_ready_when_answered() {
    let mut ready = false;
    assert_eq!(
        owner_prompt_stage(true, Some(BootStage::AgentReady), &mut ready),
        Some(BootStage::AwaitingOwnerPrompt)
    );
    assert_eq!(
        owner_prompt_stage(false, Some(BootStage::AwaitingOwnerPrompt), &mut ready),
        Some(BootStage::AgentReady)
    );
}

#[test]
fn owner_prompt_before_readiness_does_not_claim_agent_ready_when_answered() {
    let mut ready = false;
    assert_eq!(
        owner_prompt_stage(true, None, &mut ready),
        Some(BootStage::AwaitingOwnerPrompt)
    );
    assert_eq!(
        owner_prompt_stage(false, Some(BootStage::AwaitingOwnerPrompt), &mut ready),
        None
    );
    let mut ready = true;
    assert_eq!(
        owner_prompt_stage(false, Some(BootStage::SpawnFailed), &mut ready),
        None
    );
}
