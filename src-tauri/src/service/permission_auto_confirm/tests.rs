use super::*;
use crate::multiplexer::types::PromptOption;

fn facts() -> TabFacts {
    TabFacts {
        enabled: true,
        agent_type: Some("claude".into()),
        pty_id: Some("pty-1".into()),
        session_id: Some("session-1".into()),
        session_authenticated: true,
        has_lifecycle: true,
    }
}

fn request(now: Instant) -> PermissionRequestInstance {
    PermissionRequestInstance {
        id: 7,
        received_at: now,
    }
}

fn prompt(question: &str) -> PendingPrompt {
    PendingPrompt {
        question: question.into(),
        options: ["Yes", "No"]
            .iter()
            .enumerate()
            .map(|(i, label)| PromptOption {
                label: (*label).into(),
                value: (i + 1).to_string(),
            })
            .collect(),
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    }
}

fn signal(command: &str) -> PermissionRequestSignal {
    PermissionRequestSignal {
        tool_name: "Bash".into(),
        tool_input: serde_json::json!({ "command": command }),
        permission_suggestions: vec![],
    }
}

fn binding() -> PromptBinding {
    PromptBinding {
        tab_id: "tab-1".into(),
        session_id: "session-1".into(),
        pty_id: "pty-1".into(),
        request_id: 7,
        screen_fingerprint: screen_fingerprint(&prompt("Proceed?")),
        prompt_fingerprint: "fp".into(),
    }
}

#[test]
fn precheck_accepts_a_fresh_request_on_a_bound_session() {
    let now = Instant::now();
    assert_eq!(
        precheck(&facts(), request(now), now),
        Ok(("session-1".to_string(), "pty-1".to_string()))
    );
}

#[test]
fn permission_auto_confirm_refuses_the_update_dialog() {
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../plugins/provider-codex/tests/fixtures/directory-trust/codex-update-available.txt",
    );
    let screen = std::fs::read_to_string(fixture).expect("Codex update fixture");
    let update = codeterm_plugin::tab_state::TabStateClassification {
        state: codeterm_plugin::tab_state::TabState::UpdatePrompt,
        confidence: crate::agent::state_reactor::GLOBAL_THRESHOLD,
        data: None,
        action: None,
    };
    let snapshot = codeterm_plugin::tab_state::TabStateSnapshot {
        tab_id: "update-tab".into(),
        agent_type: "codex".into(),
        session_id: None,
        cwd: None,
        screen_text: screen,
    };
    let provider = crate::agent::test_provider::MockProvider::new("codex").with_tab_state(update);
    let classification =
        crate::agent::provider::AgentProvider::classify_tab_state(&provider, &snapshot);
    assert_eq!(
        manual_update_prompt_verdict(classification.as_ref()),
        Some(Verdict::Refused(Refusal::NotRecognized))
    );
}

#[test]
fn precheck_refuses_every_missing_binding() {
    let now = Instant::now();
    let cases: Vec<(TabFacts, Refusal)> = vec![
        (
            TabFacts {
                enabled: false,
                ..facts()
            },
            Refusal::Disabled,
        ),
        (
            TabFacts {
                pty_id: None,
                ..facts()
            },
            Refusal::PtyMoved,
        ),
        (
            TabFacts {
                session_id: None,
                ..facts()
            },
            Refusal::SessionUnbound,
        ),
        (
            TabFacts {
                session_authenticated: false,
                ..facts()
            },
            Refusal::SessionUnbound,
        ),
        (
            TabFacts {
                has_lifecycle: false,
                ..facts()
            },
            Refusal::NoLifecycle,
        ),
    ];
    for (facts, refusal) in cases {
        assert_eq!(precheck(&facts, request(now), now), Err(refusal));
    }
    let later = now + REQUEST_MAX_AGE + Duration::from_secs(1);
    assert_eq!(
        precheck(&facts(), request(now), later),
        Err(Refusal::StaleRequest)
    );
}

#[test]
fn binding_holds_only_while_tab_session_pty_and_prompt_are_unchanged() {
    let bound = binding();
    let live = bound.screen_fingerprint.clone();
    assert_eq!(verify_binding(&bound, Some(&facts()), Some(&live)), Ok(()));

    let kill_switch = TabFacts {
        enabled: false,
        ..facts()
    };
    assert_eq!(
        verify_binding(&bound, Some(&kill_switch), Some(&live)),
        Err(Refusal::Disabled)
    );
    let new_session = TabFacts {
        session_id: Some("session-2".into()),
        ..facts()
    };
    assert_eq!(
        verify_binding(&bound, Some(&new_session), Some(&live)),
        Err(Refusal::SessionMoved)
    );
    let new_pty = TabFacts {
        pty_id: Some("pty-2".into()),
        ..facts()
    };
    assert_eq!(
        verify_binding(&bound, Some(&new_pty), Some(&live)),
        Err(Refusal::PtyMoved)
    );
    let other = screen_fingerprint(&prompt("Something else?"));
    assert_eq!(
        verify_binding(&bound, Some(&facts()), Some(&other)),
        Err(Refusal::PromptChanged)
    );
    assert_eq!(
        verify_binding(&bound, Some(&facts()), None),
        Err(Refusal::PromptChanged)
    );
    assert_eq!(
        verify_binding(&bound, None, Some(&live)),
        Err(Refusal::PtyMoved)
    );
}

#[test]
fn a_request_instance_is_answered_at_most_once() {
    let mut ledger = Ledger::default();
    let now = Instant::now();
    assert_eq!(ledger.claim("tab-1", 7, "screen", now), Ok(()));
    assert_eq!(
        ledger.claim("tab-1", 7, "screen", now),
        Err(Refusal::AlreadyAnswered)
    );
    assert_eq!(ledger.claim("tab-2", 7, "screen", now), Ok(()));
}

#[test]
fn answers_are_rate_limited_per_tab_and_the_window_slides() {
    let mut ledger = Ledger::default();
    let now = Instant::now();
    for id in 0..RATE_LIMIT as u64 {
        assert_eq!(ledger.claim("tab-1", id, "screen", now), Ok(()));
    }
    assert_eq!(
        ledger.claim("tab-1", 999, "screen", now),
        Err(Refusal::RateLimited)
    );
    assert_eq!(ledger.claim("tab-2", 999, "screen", now), Ok(()));
    assert_eq!(
        ledger.claim("tab-1", 999, "screen", now + RATE_WINDOW),
        Ok(())
    );
}

#[test]
fn only_the_just_answered_prompt_settles_and_only_briefly() {
    let mut ledger = Ledger::default();
    let now = Instant::now();
    ledger.claim("tab-1", 1, "screen-a", now).unwrap();
    assert!(ledger.settling("tab-1", "screen-a", now));
    assert!(!ledger.settling("tab-1", "screen-b", now));
    assert!(!ledger.settling("tab-2", "screen-a", now));
    assert!(!ledger.settling("tab-1", "screen-a", now + SETTLE_WINDOW));
    ledger.end_settling("tab-1");
    assert!(!ledger.settling("tab-1", "screen-a", now));
}

#[test]
fn a_refusal_is_reported_once_per_request_and_reason() {
    let mut ledger = Ledger::default();
    assert!(ledger.note_refusal("tab-1", 1, Refusal::NotRecognized));
    assert!(!ledger.note_refusal("tab-1", 1, Refusal::NotRecognized));
    assert!(ledger.note_refusal("tab-1", 1, Refusal::RateLimited));
    assert!(ledger.note_refusal("tab-1", 2, Refusal::RateLimited));
}

#[test]
fn prompt_fingerprint_binds_tab_session_pty_request_and_tool_input() {
    let base = prompt_fingerprint("t", "s", "p", 1, &signal("ls"), &prompt("Proceed?"));
    assert_eq!(
        base,
        prompt_fingerprint("t", "s", "p", 1, &signal("ls"), &prompt("Proceed?"))
    );
    let variants = [
        prompt_fingerprint("t2", "s", "p", 1, &signal("ls"), &prompt("Proceed?")),
        prompt_fingerprint("t", "s2", "p", 1, &signal("ls"), &prompt("Proceed?")),
        prompt_fingerprint("t", "s", "p2", 1, &signal("ls"), &prompt("Proceed?")),
        prompt_fingerprint("t", "s", "p", 2, &signal("ls"), &prompt("Proceed?")),
        prompt_fingerprint("t", "s", "p", 1, &signal("rm"), &prompt("Proceed?")),
        prompt_fingerprint("t", "s", "p", 1, &signal("ls"), &prompt("Other?")),
    ];
    for variant in variants {
        assert_ne!(base, variant);
    }
}
