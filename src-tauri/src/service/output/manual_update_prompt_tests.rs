use super::parse_manual_update_prompt;
use crate::agent::prompt::PromptDecision;
use crate::agent::test_provider::MockProvider;
use crate::multiplexer::state::MultiplexerState;
use codeterm_plugin::tab_state::{TabState, TabStateClassification, TabStateSnapshot};
use codeterm_types::pty::{PendingPrompt, PromptOption};

fn update_frame() -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../plugins/provider-codex/tests/fixtures/directory-trust/codex-update-available.txt");
    std::fs::read_to_string(path).expect("Codex update fixture")
}

fn menu_options(screen: &str) -> Vec<PromptOption> {
    screen
        .lines()
        .filter_map(|line| {
            let line = line.trim().strip_prefix('›').unwrap_or(line.trim()).trim_start();
            let digits = line.chars().take_while(char::is_ascii_digit).count();
            let number = line.get(..digits)?.parse::<usize>().ok()?;
            let suffix = line.get(digits..)?.trim_start();
            let label = suffix.strip_prefix('.')?.trim_start();
            (!label.is_empty()).then(|| PromptOption {
                label: label.to_string(),
                value: number.to_string(),
            })
        })
        .collect()
}

fn provider(state: TabState, prompt: PendingPrompt) -> MockProvider {
    MockProvider::new("fixture")
        .with_tab_state(TabStateClassification {
            state,
            confidence: 1.0,
            data: None,
            action: None,
        })
        .with_parsed_prompt(prompt)
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn codex_update_dialog_is_a_respondable_structured_prompt() {
    let screen = update_frame();
    let visible = menu_options(&screen);
    let parsed = PendingPrompt {
        question: "manual update choice".into(),
        options: visible.clone(),
        sections: None,
        multi_select: Some(false),
        submit_available: Some(false),
        selected_values: Some(vec!["1".into()]),
    };
    let classification = TabStateClassification {
        state: TabState::UpdatePrompt,
        confidence: 1.0,
        data: None,
        action: None,
    };
    let provider = MockProvider::with_directory_trust_response(
        "fixture",
        codeterm_plugin::provider::DirectoryTrustPromptResponse::Answer {
            step: b"\r".to_vec(),
        },
    )
    .with_tab_state(classification)
    .with_parsed_prompt(parsed.clone());
    let snapshot = TabStateSnapshot {
        tab_id: "update-tab".into(),
        agent_type: "fixture".into(),
        session_id: None,
        cwd: None,
        screen_text: screen,
    };

    let pending = parse_manual_update_prompt(&provider, &snapshot)
        .expect("classified manual prompt");

    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).expect("tab");
    tab.agent_type = Some("fixture".into());
    tab.cwd = Some("/srv/lanes/update".into());
    tab.provisioned_cwd = Some("/srv/lanes/update".into());
    tab.pty_id = Some("pty-update".into());
    tab.auto_approve = true;
    tab.permission_auto_confirm = true;
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(provider));
    assert!(super::trust_candidate::provider_classifies_non_auto_ack_dialog(
        &fixture.service,
        super::trust_candidate::ProviderDialogIdentity {
            agent_type: "fixture",
            tab_id: &tab_id,
        },
        &snapshot.screen_text,
    ));
    fixture.service.apply_prompt_decision_with_screen(
        &PromptDecision::NewPrompt {
            tab_id: tab_id.clone(),
            prompt: pending,
        },
        Some(&snapshot.screen_text),
        false,
        true,
    );

    let pending = fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .and_then(|tab| tab.pending_prompt.as_ref())
        .expect("update options remain pending for tab respond");

    assert_eq!(pending.options, visible);
    assert_eq!(
        pending
            .options
            .iter()
            .map(|option| option.value.as_str())
            .collect::<Vec<_>>(),
        ["1", "2", "3"]
    );
    assert_eq!(
        super::prompt_step_dispatch_count_for_test("pty-update"),
        0,
        "manual update prompts must never dispatch PTY keys automatically"
    );
}

#[test]
fn non_update_dialog_does_not_use_the_manual_update_prompt_path() {
    let screen = update_frame();
    let snapshot = TabStateSnapshot {
        tab_id: "update-tab".into(),
        agent_type: "fixture".into(),
        session_id: None,
        cwd: None,
        screen_text: screen.clone(),
    };
    let prompt = PendingPrompt {
        question: String::new(),
        options: menu_options(&screen),
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    };

    assert!(parse_manual_update_prompt(&provider(TabState::Working, prompt), &snapshot).is_none());
}

#[test]
fn provider_classified_trust_dialog_blocks_ga_ack_without_provenance() {
    let screen = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../plugins/provider-claude/tests/fixtures/folder-trust")
            .join("quick-safety-yes-focused.txt"),
    )
    .expect("Claude trust fixture");
    let pending = PendingPrompt {
        question: "folder trust".into(),
        options: vec![
            PromptOption {
                label: "Yes, I trust this folder".into(),
                value: "yes".into(),
            },
            PromptOption {
                label: "No, exit".into(),
                value: "no".into(),
            },
        ],
        sections: None,
        multi_select: Some(false),
        submit_available: Some(false),
        selected_values: Some(vec!["1".into()]),
    };
    let provider = MockProvider::with_directory_trust_response(
        "fixture",
        codeterm_plugin::provider::DirectoryTrustPromptResponse::Answer {
            step: b"\r".to_vec(),
        },
    )
    .with_tab_state(TabStateClassification {
        state: TabState::ClarifyingQuestion,
        confidence: 1.0,
        data: None,
        action: None,
    })
    .with_parsed_prompt(pending.clone());
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).expect("tab");
    tab.agent_type = Some("fixture".into());
    tab.cwd = Some("/srv/lanes/current".into());
    tab.provisioned_cwd = Some("/srv/lanes/provisioned".into());
    tab.pty_id = Some("pty-trust-unprovisioned".into());
    tab.auto_approve = true;
    tab.permission_auto_confirm = true;
    mux.register_alias(crate::service::general_agent::GENERAL_ALIAS, &tab_id);
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(provider));

    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .resolve_tab_id(crate::service::general_agent::GENERAL_ALIAS)
            .as_deref(),
        Some(tab_id.as_str())
    );

    let provider_classifies_trust = super::trust_candidate::candidate_for_output(
        &fixture.service,
        &tab_id,
        "fixture",
        Some(&screen),
        false,
        true,
        Some("cwd differs from provenance"),
    );
    assert!(
        provider_classifies_trust,
        "provider trust classification remains independent of provenance"
    );
    assert!(!crate::service::directory_trust::directory_trust_check_applies(
        fixture.service.mux.read().get_tab(&tab_id).expect("tab")
    ));

    assert_eq!(
        crate::service::general_agent::blocker::classify(&pending, &screen, false)
            .map(|ack| ack.kind),
        Some(crate::service::general_agent::blocker::AutoAckKind::FirstAffirmative),
        "the fixture reaches the General Agent affirmative classifier"
    );

    fixture.service.apply_prompt_decision_with_screen(
        &PromptDecision::NewPrompt {
            tab_id: tab_id.clone(),
            prompt: pending,
        },
        Some(&screen),
        false,
        false,
    );

    assert_eq!(
        super::prompt_step_dispatch_count_for_test("pty-trust-unprovisioned"),
        0,
        "provider-classified trust dialog must not reach the General Agent auto-ack writer"
    );
}
