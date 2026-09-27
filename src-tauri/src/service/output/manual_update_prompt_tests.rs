use super::{
    claim_manual_update_occurrence, clear_manual_update_occurrence, parse_manual_update_prompt,
    parse_manual_update_prompt_with_response, resolve_classified_manual_update_prompt,
};
use crate::agent::prompt::PromptDecision;
use crate::agent::test_provider::MockProvider;
use crate::multiplexer::state::MultiplexerState;
use codeterm_plugin::tab_state::{TabState, TabStateClassification, TabStateSnapshot};
use codeterm_types::pty::{PendingPrompt, PromptOption};

fn update_frame() -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../plugins/provider-codex/tests/fixtures/directory-trust/codex-update-available.txt",
    );
    std::fs::read_to_string(path).expect("Codex update fixture")
}

fn menu_options(screen: &str) -> Vec<PromptOption> {
    screen
        .lines()
        .filter_map(|line| {
            let line = line
                .trim()
                .strip_prefix('›')
                .unwrap_or(line.trim())
                .trim_start();
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
fn codex_update_dialog_resolves_to_skip_with_one_notification() {
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
    let provider = MockProvider::new("fixture")
        .with_tab_state(classification)
        .with_parsed_prompt(parsed.clone())
        .with_classified_prompt_auto_response(
            codeterm_plugin::provider::ClassifiedPromptOptionRole::Skip,
        );
    let snapshot = TabStateSnapshot {
        tab_id: "update-tab".into(),
        agent_type: "fixture".into(),
        session_id: None,
        cwd: None,
        screen_text: screen,
    };

    let (pending, skip_value) = parse_manual_update_prompt_with_response(&provider, &snapshot)
        .expect("classified manual prompt and declared Skip response");
    assert_eq!(skip_value.as_deref(), Some("2"));

    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).expect("tab");
    tab.agent_type = Some("fixture".into());
    tab.cwd = Some("/srv/lanes/update".into());
    tab.provisioned_cwd = Some("/srv/lanes/update".into());
    tab.pty_id = Some("pty-update".into());
    tab.auto_approve = true;
    tab.permission_auto_confirm = true;
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(provider));
    assert!(
        super::trust_candidate::provider_classifies_non_auto_ack_dialog(
            &fixture.service,
            super::trust_candidate::ProviderDialogIdentity {
                agent_type: "fixture",
                tab_id: &tab_id,
            },
            &snapshot.screen_text,
        )
    );
    let mut snapshot = snapshot;
    snapshot.tab_id = tab_id.clone();
    fixture.service.output_buf.lock().append("pty-update", snapshot.screen_text.as_bytes());
    let tab_title = fixture.service.mux.read().get_tab(&tab_id).unwrap().title.clone();
    let occurrence = serde_json::to_string(&pending.options).unwrap();
    let response = resolve_classified_manual_update_prompt(
        &fixture.service,
        &snapshot,
        pending.clone(),
        skip_value,
    );
    assert_eq!(response, Some(crate::service::tab::RespondPromptOutcome::Sent));
    assert!(fixture.service.mux.read().get_tab(&tab_id).unwrap().pending_prompt.is_none());

    {
        let notif = fixture.service.notif.lock();
        let notifications = notif.list(false);
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].title, format!("{tab_title}: update skipped"));
    }

    let second_render = resolve_classified_manual_update_prompt(
        &fixture.service,
        &snapshot,
        pending,
        Some("2".into()),
    );
    assert_eq!(second_render, None);
    assert_eq!(fixture.service.notif.lock().list(false).len(), 1);
    assert!(!claim_manual_update_occurrence(&tab_id, &occurrence));
    clear_manual_update_occurrence(&tab_id);
    assert!(claim_manual_update_occurrence(&tab_id, &occurrence));
    clear_manual_update_occurrence(&tab_id);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn codex_update_dialog_is_a_respondable_structured_prompt() {
    let screen = update_frame();
    let prompt = PendingPrompt {
        question: "manual update choice".into(),
        options: menu_options(&screen),
        sections: None,
        multi_select: Some(false),
        submit_available: Some(false),
        selected_values: Some(vec!["1".into()]),
    };
    let provider = provider(TabState::UpdatePrompt, prompt.clone());
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).expect("tab");
    tab.agent_type = Some("fixture".into());
    tab.pty_id = Some("pty-update-respondable".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(provider));
    fixture.service.apply_prompt_decision_with_kind_and_notice(
        &PromptDecision::NewPrompt {
            tab_id: tab_id.clone(),
            prompt: prompt.clone(),
        },
        Some(&screen),
        codeterm_types::pty::PromptKind::ManualUpdate,
        true,
        true,
    );

    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&tab_id).expect("tab");
    assert_eq!(tab.pending_prompt.as_ref(), Some(&prompt));
    assert_eq!(tab.pending_prompt_kind, Some(codeterm_types::pty::PromptKind::ManualUpdate));
    assert_eq!(super::prompt_step_dispatch_count_for_test("pty-update-respondable"), 0);
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
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(provider));

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
    assert!(
        !crate::service::directory_trust::directory_trust_check_applies(
            fixture.service.mux.read().get_tab(&tab_id).expect("tab")
        )
    );

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

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn user_dir_codex_trust_prompt_is_pending_and_respondable_without_auto_answer() {
    let screen = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../plugins/provider-codex/tests/fixtures/directory-trust/trust-folder-access-0.157.1.txt"),
    )
    .expect("Codex user directory trust fixture");
    let visible = menu_options(&screen);
    let parsed = PendingPrompt {
        question: "folder access choice".into(),
        options: visible.clone(),
        sections: None,
        multi_select: Some(false),
        submit_available: Some(false),
        selected_values: Some(vec!["1".into()]),
    };
    let provider = MockProvider::new("fixture").with_parsed_prompt(parsed);
    let pending = super::parse_directory_trust_prompt(&provider, &screen)
        .expect("supported folder trust dialog is parsed without provenance");

    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).expect("tab");
    tab.agent_type = Some("fixture".into());
    tab.cwd = Some("C:\\Users\\dev\\workspace".into());
    tab.pty_id = Some("pty-user-dir-trust".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(provider));
    fixture.service.apply_prompt_decision_with_screen(
        &PromptDecision::NewPrompt {
            tab_id: tab_id.clone(),
            prompt: pending,
        },
        Some(&screen),
        true,
        false,
    );

    let mux = fixture.service.mux.read();
    let pending = mux
        .get_tab(&tab_id)
        .and_then(|tab| tab.pending_prompt.as_ref())
        .expect("folder trust options remain available to tab respond");
    assert_eq!(pending.options, visible);
    assert_eq!(
        pending
            .options
            .iter()
            .map(|option| option.value.as_str())
            .collect::<Vec<_>>(),
        ["1", "2"]
    );
    assert_eq!(
        super::prompt_step_dispatch_count_for_test("pty-user-dir-trust"),
        0,
        "a user-directory trust dialog stays user controlled"
    );
}

#[test]
fn provider_ready_idle_user_dir_trust_prompt_appears_without_provenance_on_unchanged_screen() {
    let screen = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../plugins/provider-codex/tests/fixtures/directory-trust/trust-folder-access-0.157.1.txt"),
    )
    .expect("Codex user directory trust fixture");
    let prompt = PendingPrompt {
        question: "folder access choice".into(),
        options: menu_options(&screen),
        sections: None,
        multi_select: Some(false),
        submit_available: Some(false),
        selected_values: Some(vec!["1".into()]),
    };
    let provider = MockProvider::new("codex").with_parsed_prompt(prompt.clone());
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.link_pty(&tab_id, "pty-user-dir-no-change".into());
    let tab = mux.get_tab_mut(&tab_id).expect("tab");
    tab.agent_type = Some("codex".into());
    tab.cwd = Some("C:\\Users\\dev\\workspace".into());
    tab.boot_stage = Some(crate::service::boot_stage::BootStage::ProviderReady);
    tab.agent_state = Some(
        crate::agent::offline_state_classifier::TabVerdict {
            state: crate::agent::offline_state_classifier::TimelineState::Idle,
            subtype: None,
        }
        .into(),
    );
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(provider));
    {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).expect("Codex tab");
        assert_eq!(
            tab.boot_stage,
            Some(crate::service::boot_stage::BootStage::ProviderReady)
        );
        assert_eq!(
            tab.agent_state.as_ref().map(|state| state.state),
            Some(crate::agent::offline_state_classifier::TimelineState::Idle)
        );
        assert!(tab.provisioned_cwd.is_none());
        assert!(tab.agent_session_id.is_none());
        assert!(!crate::service::directory_trust::directory_trust_check_applies(tab));
    }
    fixture
        .service
        .screen_state
        .lock()
        .process("pty-user-dir-no-change", screen.as_bytes());
    let status_rows = crate::agent::classify_input::StatusBarRows::resolve(
        &*fixture.service.agent_registry,
        "codex",
    )
    .expect("Codex status rows");
    let screen_hash = fixture
        .service
        .screen_state
        .lock()
        .get_content_hash("pty-user-dir-no-change", status_rows.get())
        .expect("trust frame hash");
    fixture
        .service
        .agent_tracker
        .lock()
        .update_output("pty-user-dir-no-change", screen_hash);
    assert!(!fixture
        .service
        .agent_tracker
        .lock()
        .update_output("pty-user-dir-no-change", screen_hash));

    fixture
        .service
        .process_pty_output("pty-user-dir-no-change", b"\x1b[0m");
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .and_then(|tab| tab.pending_prompt.clone()),
        Some(prompt.clone())
    );
    assert_eq!(
        super::prompt_step_dispatch_count_for_test("pty-user-dir-no-change"),
        0,
        "an unprovenanced user-directory trust prompt is never answered automatically"
    );

    fixture
        .service
        .process_pty_output("pty-user-dir-no-change", b"\x1b[0m");

    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .and_then(|tab| tab.pending_prompt.clone()),
        Some(prompt)
    );
}
