use super::*;

fn diagnostic_fixture(
    provider: Box<dyn AgentProvider>,
) -> (crate::service::test_fixture::ServiceFixture, LaunchAttempt) {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let attempt = LaunchAttempt {
        tab_id,
        pty_id: uuid::Uuid::new_v4().to_string(),
        binding_nonce: uuid::Uuid::new_v4().to_string(),
    };
    let tab = mux.get_tab_mut(&attempt.tab_id).unwrap();
    tab.agent_type = Some(provider.id().to_string());
    tab.pty_id = Some(attempt.pty_id.clone());
    tab.session_binding_nonce = Some(attempt.binding_nonce.clone());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(provider);
    (fixture, attempt)
}

fn capture_gate_diagnostic(
    fixture: &crate::service::test_fixture::ServiceFixture,
    attempt: &LaunchAttempt,
    provider: &str,
    mode: LaunchInputMode,
) -> (
    Result<LaunchInputEvidence, LaunchInputRefusal>,
    serde_json::Value,
) {
    let mut snapshots = Vec::new();
    let result = fixture.service.await_launch_input_readiness_observed(
        attempt,
        provider,
        std::time::Duration::ZERO,
        mode,
        |snapshot| snapshots.push(serde_json::to_value(snapshot).unwrap()),
    );
    assert_eq!(
        snapshots.len(),
        1,
        "every terminal path records one final observation"
    );
    (result, snapshots.pop().unwrap())
}

#[test]
fn diagnostic_success_keeps_actual_inputs_and_unchecked_process_unknown() {
    let (fixture, attempt) = diagnostic_fixture(Box::new(MockProvider::new("opaque-gate")));
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&attempt.tab_id).unwrap();
        tab.agent_session_id = Some("opaque-own-session".into());
        tab.reported_session_binding_nonce = Some(attempt.binding_nonce.clone());
    }
    let (result, snapshot) =
        capture_gate_diagnostic(&fixture, &attempt, "opaque-gate", LaunchInputMode::Standard);
    assert_eq!(result, Ok(LaunchInputEvidence::SessionBinding));
    assert_eq!(snapshot["outcome"], "session_binding");
    assert_eq!(snapshot["tab_id"], attempt.tab_id);
    assert_eq!(snapshot["pty_id"], attempt.pty_id);
    assert_eq!(snapshot["binding_nonce"], attempt.binding_nonce);
    assert_eq!(snapshot["mode"], "standard");
    assert_eq!(snapshot["decision_inputs"]["value"]["session_bound"], true);
    assert_eq!(
        snapshot["decision_inputs"]["value"]["composer_ready"],
        false
    );
    assert_eq!(snapshot["process_gate_allows"]["value"], true);
    assert_eq!(
        snapshot["provider_process_alive"]["evaluation"],
        "not_evaluated"
    );
    assert_eq!(
        snapshot["classification"],
        serde_json::json!({"evaluation": "observed", "value": null})
    );
}

#[test]
fn diagnostic_refusal_excludes_classifier_content() {
    use codeterm_plugin::tab_state::{Surface, TabStateAction};
    let secret = "opaque-classifier-content-1a37";
    let mut classification = launch_classification(TabState::WaitingOnAuth).unwrap();
    classification.action = Some(TabStateAction {
        surface: Some(Surface {
            kind: "opaque-kind".into(),
            payload: serde_json::json!({"text": secret}),
        }),
        ..Default::default()
    });
    let (fixture, attempt) = diagnostic_fixture(Box::new(
        MockProvider::new("opaque-gate").with_tab_state(classification),
    ));
    let (result, snapshot) =
        capture_gate_diagnostic(&fixture, &attempt, "opaque-gate", LaunchInputMode::Standard);
    assert_eq!(
        result,
        Err(LaunchInputRefusal::ProviderDialog(TabState::WaitingOnAuth))
    );
    assert_eq!(snapshot["outcome"], "provider_dialog");
    assert_eq!(
        snapshot["classification"]["value"],
        serde_json::json!({"state": "waiting_on_auth", "confidence": 1.0})
    );
    let encoded = serde_json::to_string(&snapshot).unwrap();
    assert!(!encoded.contains(secret));
    assert!(!encoded.contains("opaque-kind"));
    assert!(encoded.len() < 4096);
}

#[test]
fn diagnostic_lazy_deadline_preserves_negative_process_and_input_evidence() {
    let (fixture, attempt) = diagnostic_fixture(Box::new(MockProvider::new("opaque-gate")));
    let (result, snapshot) = capture_gate_diagnostic(
        &fixture,
        &attempt,
        "opaque-gate",
        LaunchInputMode::LazySessionStart,
    );
    assert_eq!(result, Err(LaunchInputRefusal::Deadline));
    assert_eq!(snapshot["outcome"], "deadline");
    assert_eq!(snapshot["provider_process_alive"]["value"], false);
    assert_eq!(snapshot["terminal_input_ready"]["value"], false);
    assert_eq!(snapshot["provider_tui"]["value"], false);
    assert_eq!(snapshot["tui_evidence"]["value"], false);
    assert_eq!(snapshot["directory_trust_state"]["value"], "unknown");
    assert_eq!(snapshot["event_generation"], 0);
    assert_eq!(snapshot["deadline_ms"], 0);
    assert!(snapshot["elapsed_ms"].is_u64());
}

#[test]
fn diagnostic_superseded_attempt_never_claims_evaluated_readiness() {
    let (fixture, mut attempt) = diagnostic_fixture(Box::new(MockProvider::new("opaque-gate")));
    attempt.binding_nonce = "opaque-old-nonce".into();
    let (result, snapshot) =
        capture_gate_diagnostic(&fixture, &attempt, "opaque-gate", LaunchInputMode::Standard);
    assert_eq!(result, Err(LaunchInputRefusal::AttemptSuperseded));
    assert_eq!(snapshot["binding_nonce"], "opaque-old-nonce");
    assert_eq!(snapshot["outcome"], "attempt_superseded");
    assert_eq!(snapshot["session_bound"]["evaluation"], "not_evaluated");
    assert_eq!(snapshot["composer_ready"]["evaluation"], "not_evaluated");
    assert_eq!(snapshot["decision_inputs"]["evaluation"], "not_evaluated");
}

#[test]
fn diagnostic_onboarding_deadline_records_unknown_unevaluated_predicates() {
    let provider = codeterm_plugin::agent_claude::ClaudeProvider::new();
    let screen = include_str!("../../../../../plugins/provider-claude/tests/fixtures/onboarding/auto-environment-captured.txt");
    assert!(
        provider.launch_onboarding_response(screen).is_some(),
        "onboarding precondition must be recognized"
    );
    let (fixture, attempt) = diagnostic_fixture(Box::new(provider));
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&attempt.tab_id)
        .unwrap()
        .role = Some(codeterm_types::agent::roles::AgentRole::Worker);
    let observed_screen = {
        let mut screens = fixture.service.screen_state.lock();
        screens.resize(&attempt.pty_id, 180, 40);
        let frame = screen.lines().collect::<Vec<_>>().join("\r\n");
        screens.process(&attempt.pty_id, frame.as_bytes());
        screens.get_screen_text(&attempt.pty_id).unwrap()
    };
    assert!(
        fixture
            .service
            .agent_registry
            .with_provider("claude", |provider| {
                provider
                    .launch_onboarding_response(&observed_screen)
                    .is_some()
            })
            .unwrap(),
        "the actual gate frame must satisfy the onboarding precondition",
    );
    let (result, snapshot) =
        capture_gate_diagnostic(&fixture, &attempt, "claude", LaunchInputMode::Standard);
    assert_eq!(result, Err(LaunchInputRefusal::Deadline));
    assert_eq!(snapshot["onboarding_wait"], true);
    assert_eq!(snapshot["pending_prompt"]["value"], false);
    assert_eq!(snapshot["composer_ready"]["evaluation"], "not_evaluated");
    assert_eq!(
        snapshot["terminal_input_ready"]["evaluation"],
        "not_evaluated"
    );
    assert_eq!(snapshot["classification"]["evaluation"], "not_evaluated");
    assert_eq!(snapshot["decision_inputs"]["evaluation"], "not_evaluated");
    assert!(!serde_json::to_string(&snapshot)
        .unwrap()
        .contains(screen.trim()));
}
