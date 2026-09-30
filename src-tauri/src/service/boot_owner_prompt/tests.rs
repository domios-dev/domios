use super::*;
use crate::agent::prompt::PromptDecision;
use crate::multiplexer::types::{PendingPrompt, PromptOption};

fn screen_prompt() -> PendingPrompt {
    PendingPrompt {
        question: "opaque question?".into(),
        options: ["opaque-yes", "opaque-no"]
            .iter()
            .enumerate()
            .map(|(index, label)| PromptOption {
                label: (*label).into(),
                value: (index + 1).to_string(),
            })
            .collect(),
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    }
}

fn ready_tab(supervised: bool) -> (crate::service::test_fixture::ServiceFixture, String) {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&tab_id).expect("fixture tab").boot_stage = Some(BootStage::AgentReady);
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    if supervised {
        fixture
            .service
            .worker_ack
            .supervise_startup_for_test(&tab_id, "opaque-supervisor");
    }
    (fixture, tab_id)
}

fn stage(
    fixture: &crate::service::test_fixture::ServiceFixture,
    tab_id: &str,
) -> Option<BootStage> {
    fixture
        .service
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.boot_stage)
}

#[test]
fn stage_rules_hold_only_ready_launches_that_owe_a_handshake() {
    assert_eq!(
        held_stage(true, Some(BootStage::AgentReady)),
        Some(BootStage::AwaitingOwnerPrompt)
    );
    assert_eq!(held_stage(false, Some(BootStage::AgentReady)), None);
    assert_eq!(held_stage(true, Some(BootStage::AgentLaunching)), None);
    assert_eq!(
        released_stage(true, Some(BootStage::AwaitingOwnerPrompt)),
        Some(BootStage::AgentReady)
    );
    assert_eq!(
        released_stage(false, Some(BootStage::AwaitingOwnerPrompt)),
        None
    );
    assert_eq!(released_stage(true, Some(BootStage::AgentReady)), None);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn boot_permission_prompt_holds_readiness_without_notification() {
    let (fixture, tab_id) = ready_tab(true);
    let prompt = screen_prompt();
    fixture
        .service
        .apply_prompt_decision(&PromptDecision::NewPrompt {
            tab_id: tab_id.clone(),
            prompt: prompt.clone(),
        });
    assert_eq!(
        stage(&fixture, &tab_id),
        Some(BootStage::AwaitingOwnerPrompt)
    );

    let signal = crate::agent::permission_request::parse_permission_request(&serde_json::json!({
        "tool_name": "OpaqueTool",
        "permission_suggestions": [{ "behavior": "opaque-allow" }],
    }))
    .expect("hook payload parses");
    let hook_decision = fixture
        .service
        .prompt_detector
        .lock()
        .hook_detected_permission_request(&tab_id, signal);
    fixture
        .service
        .apply_hook_prompt_decision(&tab_id, &hook_decision);
    fixture
        .service
        .apply_prompt_decision(&PromptDecision::PromptUpdated {
            tab_id: tab_id.clone(),
            prompt: prompt.clone(),
        });

    let pending = fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .and_then(|tab| tab.pending_prompt.clone());
    assert_eq!(pending, Some(prompt), "the surfaced rendering stays");
    assert!(fixture.service.notif.lock().list(false).is_empty());
    assert_eq!(
        stage(&fixture, &tab_id),
        Some(BootStage::AwaitingOwnerPrompt)
    );

    fixture.service.clear_pending_prompt(&tab_id);
    assert_eq!(stage(&fixture, &tab_id), Some(BootStage::AgentReady));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn prompt_after_the_handshake_leaves_readiness_alone() {
    let (fixture, tab_id) = ready_tab(false);
    fixture
        .service
        .apply_prompt_decision(&PromptDecision::NewPrompt {
            tab_id: tab_id.clone(),
            prompt: screen_prompt(),
        });
    assert_eq!(stage(&fixture, &tab_id), Some(BootStage::AgentReady));
}
