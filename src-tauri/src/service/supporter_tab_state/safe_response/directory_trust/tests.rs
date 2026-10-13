use super::*;
use codeterm_types::decision::DecisionQuestion;

const NO_FIRST: &str = include_str!(
    "../../../../../../plugins/provider-claude/tests/fixtures/folder-trust/quick-safety-no-first.txt"
);
const YES_FOCUSED: &str = include_str!(
    "../../../../../../plugins/provider-claude/tests/fixtures/folder-trust/quick-safety-yes-focused.txt"
);
const PREAPPROVED_LOCAL_NO_FIRST: &str = include_str!(
    "../../../../../../plugins/provider-claude/tests/fixtures/folder-trust/trust-local-preapproved-no-first.txt"
);
const RULE_AFFIRMATIVE: &str = include_str!(
    "../../../../../../plugins/provider-claude/tests/fixtures/folder-trust/trust-rule-focus-affirmative.txt"
);
const TOOL_PERMISSION: &str = include_str!(
    "../../../../../../plugins/provider-claude/tests/fixtures/folder-trust/tool-permission.txt"
);
const ONBOARDING: &str = include_str!(
    "../../../../../../plugins/provider-claude/tests/fixtures/onboarding/auto-environment-captured.txt"
);
const AUTH_LOGIN: &str = include_str!(
    "../../../../../../plugins/provider-claude/tests/fixtures/tab-state/auth-login.txt"
);
const JOB_DIR: &str = "/srv/cron/nightly";

fn claude() -> codeterm_plugin::agent_claude::ClaudeProvider {
    codeterm_plugin::agent_claude::ClaudeProvider::new()
}

fn trust_target(screen: &str) -> Target {
    provider_target(&claude(), screen).expect("provider-declared trust target")
}

fn view<'a>(label: &'a str, confidence: f64, choice: Option<(&'a str, f64)>) -> VerdictView<'a> {
    VerdictView {
        label,
        live_prompt: true,
        confidence,
        safe_choice: choice,
    }
}

fn metadata_for(target: &Target) -> TabStateMetadata {
    TabStateMetadata {
        context: serde_json::json!({ "safe_response_kind": target.kind.context() }),
        session_key: "fixture".into(),
        title: "fixture".into(),
        spawning_tab_id: None,
        parent_tab_id: None,
        plan_id: None,
        task_id: None,
        attempt_id: None,
        prompt_options: target
            .prompt
            .options
            .iter()
            .enumerate()
            .map(|(index, option)| NumberedPromptOption {
                number: index + 1,
                label: option.label.clone(),
                value: option.value.clone(),
            })
            .collect(),
        declared_safe_choice: Some(target.choice.clone()),
        tab_snapshot: None,
        process_tree_key: None,
        envelope: None,
    }
}

fn unattended_tab(
    audience: crate::multiplexer::types::TabAudience,
    hidden: bool,
) -> (crate::multiplexer::state::MultiplexerState, String) {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let pty = format!("unattended-{}", uuid::Uuid::new_v4().simple());
    mux.link_pty(&tab_id, pty);
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("claude".into());
    tab.hidden = hidden;
    tab.audience = audience;
    tab.cwd = Some(JOB_DIR.into());
    tab.provisioned_cwd = Some(JOB_DIR.into());
    (mux, tab_id)
}

fn cron_tab() -> (crate::multiplexer::state::MultiplexerState, String) {
    unattended_tab(
        crate::multiplexer::types::TabAudience::UserOwnedAutomation,
        true,
    )
}

fn fixture_for(
    mux: crate::multiplexer::state::MultiplexerState,
) -> (
    crate::service::test_fixture::ServiceFixture,
    crate::service::supporter_tab_state::TestSupporterOverrideGuard,
) {
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(claude()));
    let runtime = Arc::new(SupporterRuntime::new(fixture.temp.path().join("supporter")));
    let guard = fixture
        .service
        .override_supporter_decision_for_test(SupporterConfig::default(), runtime);
    (fixture, guard)
}

fn snapshot(tab_id: &str, screen: &str) -> TabStateSnapshot {
    TabStateSnapshot {
        tab_id: tab_id.into(),
        agent_type: "claude".into(),
        session_id: None,
        cwd: Some(JOB_DIR.into()),
        screen_text: screen.into(),
    }
}

fn adapter_verdict(label: &str, choice: Option<&str>, confidence: f64) -> CandidateVerdict {
    let answer = |choice: &str| DecisionAnswer::Choice {
        choice: choice.into(),
        probabilities: BTreeMap::new(),
        confidence: Some(confidence),
    };
    let mut answers = BTreeMap::from([
        ("label".to_string(), answer(label)),
        (
            "prompt_visible".to_string(),
            DecisionAnswer::Noul { p: confidence },
        ),
        ("input_area".to_string(), answer("active_modal")),
    ]);
    if let Some(choice) = choice {
        answers.insert("safe_choice".into(), answer(choice));
    }
    let (verdict, source) = candidates::select(Ok(DecisionResponse {
        answers,
        usage: None,
    }));
    assert_eq!(source, identity::DecisionSource::Adapter);
    verdict.expect("adapter verdict")
}

fn pty_of(fixture: &crate::service::test_fixture::ServiceFixture, tab_id: &str) -> String {
    fixture
        .service
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.pty_id.clone())
        .unwrap()
}

fn journal_rows(fixture: &crate::service::test_fixture::ServiceFixture) -> Vec<serde_json::Value> {
    std::fs::read_to_string(fixture.temp.path().join("supporter/journal.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

#[test]
fn unattended_launch_prompt_jev_chooses_among_declared_trust_options() {
    let provider = claude();
    for screen in [
        NO_FIRST,
        YES_FOCUSED,
        RULE_AFFIRMATIVE,
        PREAPPROVED_LOCAL_NO_FIRST,
    ] {
        let target = trust_target(screen);
        assert_eq!(target.kind, Kind::DirectoryTrust);
        let index = target
            .prompt
            .options
            .iter()
            .position(|option| option.value == target.choice)
            .unwrap();
        let steps = provider
            .live_prompt_response(index, screen)
            .expect("provider-declared trust key sequence");
        assert_eq!(
            provider.directory_trust_prompt_response(screen),
            Some(DirectoryTrustPromptResponse::Answer {
                step: steps[0].clone()
            }),
            "the first classified key identifies the provider-declared option"
        );
        assert_eq!(
            steps[0], target.step,
            "the target contains only the next classifier step"
        );
        let DecisionQuestion::Choice { options } =
            safe_choice::safe_choice_question(&metadata_for(&target)).expect("choice question")
        else {
            panic!("safe choice is a choice question");
        };
        let mut declared = target
            .prompt
            .options
            .iter()
            .map(|option| option.value.as_str())
            .chain([OTHER])
            .collect::<Vec<_>>();
        declared.sort_unstable();
        assert_eq!(
            options.keys().map(String::as_str).collect::<Vec<_>>(),
            declared
        );
        assert_eq!(
            route(
                Some(&target),
                &view(
                    TRUST_OR_PERMISSION_PROMPT,
                    0.95,
                    Some((target.choice.as_str(), 0.93))
                ),
            ),
            UnattendedRoute::Model
        );
    }
    assert_eq!(trust_target(NO_FIRST).step, b"j");
    assert_eq!(trust_target(YES_FOCUSED).step, b"\r");
    assert_eq!(
        trust_target(NO_FIRST).choice,
        trust_target(YES_FOCUSED).choice
    );
}

#[test]
fn owned_prompt_model_may_choose_any_declared_option() {
    let target = trust_target(NO_FIRST);
    let refusal = target
        .prompt
        .options
        .iter()
        .find(|option| option.value != target.choice)
        .unwrap()
        .value
        .clone();
    assert_eq!(
        route(
            Some(&target),
            &view(
                TRUST_OR_PERMISSION_PROMPT,
                0.80,
                Some((refusal.as_str(), 0.80))
            ),
        ),
        UnattendedRoute::Model
    );
    assert_eq!(
        route(
            Some(&target),
            &view(
                TRUST_OR_PERMISSION_PROMPT,
                0.99,
                Some(("not-a-declared-value", 0.99))
            ),
        ),
        UnattendedRoute::Fallback("abstain")
    );
}

#[test]
fn structurally_detected_onboarding_uses_its_declared_classification() {
    let target = super::super::provider_target(&claude(), ONBOARDING).unwrap();
    assert_eq!(target.kind, Kind::Onboarding);
    assert_eq!(
        route(
            Some(&target),
            &view(
                LOGIN_OR_AUTH_REQUIRED,
                0.99,
                Some((target.choice.as_str(), 0.99))
            ),
        ),
        UnattendedRoute::Model
    );
    assert!(super::super::provider_target(&claude(), AUTH_LOGIN).is_none());
    assert_eq!(
        route(None, &view(LOGIN_OR_AUTH_REQUIRED, 0.99, None)),
        UnattendedRoute::Fallback("no_candidate")
    );
}

#[test]
fn unattended_launch_prompt_other_or_low_confidence_falls_back() {
    let target = trust_target(NO_FIRST);
    let choice = target.choice.as_str();
    for (verdict, expected) in [
        (
            view(TRUST_OR_PERMISSION_PROMPT, 0.95, Some((OTHER, 0.95))),
            UnattendedRoute::Fallback("abstain"),
        ),
        (
            view(TRUST_OR_PERMISSION_PROMPT, 0.95, None),
            UnattendedRoute::Fallback("abstain"),
        ),
        (
            view(TRUST_OR_PERMISSION_PROMPT, 0.79, Some((choice, 0.99))),
            UnattendedRoute::Fallback("low_confidence"),
        ),
        (
            view(TRUST_OR_PERMISSION_PROMPT, 0.99, Some((choice, 0.79))),
            UnattendedRoute::Fallback("low_confidence"),
        ),
        (
            view(TRUST_OR_PERMISSION_PROMPT, f64::NAN, Some((choice, 0.99))),
            UnattendedRoute::Fallback("low_confidence"),
        ),
        (
            view(READY_IDLE, 0.70, Some((choice, 0.99))),
            UnattendedRoute::Fallback("not_prompt"),
        ),
        (
            view(TRUST_OR_PERMISSION_PROMPT, 0.95, Some((choice, 0.95))),
            UnattendedRoute::Model,
        ),
        (
            view(READY_IDLE, 0.95, Some((choice, 0.95))),
            UnattendedRoute::Fallback("not_prompt"),
        ),
    ] {
        assert_eq!(route(Some(&target), &verdict), expected);
    }
    let mut composer = view(TRUST_OR_PERMISSION_PROMPT, 0.95, Some((choice, 0.95)));
    composer.live_prompt = false;
    assert_eq!(
        route(Some(&target), &composer),
        UnattendedRoute::Fallback("not_prompt")
    );
    assert_eq!(
        route(None, &view(TRUST_OR_PERMISSION_PROMPT, 0.99, None)),
        UnattendedRoute::Fallback("no_candidate")
    );
}

#[test]
fn planner_completion_onboarding_enters_the_owned_candidate_path() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    mux.link_pty(&tab_id, "planner-onboarding".into());
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("claude".into());
    tab.role = Some(codeterm_types::agent::AgentRole::Manager);
    tab.role_profile = Some("planner".into());
    let (fixture, _guard) = fixture_for(mux);
    let target = fixture
        .service
        .model_safe_response_target(&tab_id, ONBOARDING)
        .expect("planner completion keeps the provider-declared onboarding option");
    assert_eq!(target.kind, Kind::Onboarding);
    let gate = fixture
        .service
        .unattended_launch_gate(&tab_id, ONBOARDING)
        .expect("planner onboarding is an owned launch prompt");
    assert_eq!(gate.kind, Kind::Onboarding);
    assert_eq!(gate.target, Some(target));
    assert!(fixture
        .service
        .model_safe_response_target(&tab_id, AUTH_LOGIN)
        .is_none());
    assert!(fixture
        .service
        .unattended_launch_gate(&tab_id, AUTH_LOGIN)
        .is_none());
}

#[test]
fn owned_prompt_target_does_not_require_folder_provenance() {
    let (mut mux, tab_id) = cron_tab();
    assert!(target_applies(mux.get_tab(&tab_id).unwrap()));
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = Some("bound-session".into());
        tab.session_binding_nonce = Some("launch-nonce".into());
        tab.reported_session_binding_nonce = Some("launch-nonce".into());
    }
    assert!(target_applies(mux.get_tab(&tab_id).unwrap()));
    mux.get_tab_mut(&tab_id)
        .unwrap()
        .reported_session_binding_nonce = None;
    assert!(target_applies(mux.get_tab(&tab_id).unwrap()));
    mux.get_tab_mut(&tab_id).unwrap().cwd = Some("/srv/cron/elsewhere".into());
    assert!(target_applies(mux.get_tab(&tab_id).unwrap()));

    let (mut worker, worker_id) =
        unattended_tab(crate::multiplexer::types::TabAudience::Automation, true);
    let worker = worker.get_tab_mut(&worker_id).unwrap();
    worker.agent_session_id = Some("bound-session".into());
    worker.session_binding_nonce = Some("launch-nonce".into());
    worker.reported_session_binding_nonce = Some("launch-nonce".into());
    assert!(!target_applies(worker));

    let (visible, visible_id) = unattended_tab(crate::multiplexer::types::TabAudience::User, false);
    assert!(!target_applies(visible.get_tab(&visible_id).unwrap()));
}

#[test]
fn launch_onboarding_heuristic_scope_includes_worker_and_cron() {
    let (cron, cron_id) = cron_tab();
    let (worker, worker_id) =
        unattended_tab(crate::multiplexer::types::TabAudience::Automation, true);
    let (visible, visible_id) = unattended_tab(crate::multiplexer::types::TabAudience::User, false);
    let applies = |mux: &crate::multiplexer::state::MultiplexerState, id: &str| {
        crate::service::directory_trust::launch_onboarding_check_applies(
            mux.get_tab(id).unwrap(),
            "claude",
        )
    };
    assert!(applies(&cron, &cron_id));
    assert!(applies(&worker, &worker_id));
    assert!(!applies(&visible, &visible_id));
}

#[test]
fn provider_permission_target_includes_declared_answers() {
    let provider = claude();
    assert!(super::super::provider_target(&provider, TOOL_PERMISSION).is_none());
    let prompt = provider.parse_prompt(TOOL_PERMISSION).unwrap();
    let target = permission_target(&provider, TOOL_PERMISSION, prompt, "Bash")
        .expect("provider permission target");
    assert_eq!(target.kind, Kind::Permission);
    assert!(target.prompt.options.len() >= 2);
}

#[test]
fn unattended_launch_prompt_fallback_runs_the_deterministic_responder() {
    let (mux, tab_id) = cron_tab();
    let (fixture, _guard) = fixture_for(mux);
    let pty = pty_of(&fixture, &tab_id);
    {
        let mut screen_state = fixture.service.screen_state.lock();
        screen_state.resize(&pty, 120, 40);
        screen_state.process(&pty, NO_FIRST.as_bytes());
    }
    let declared = trust_target(NO_FIRST).choice;
    for verdict in [
        adapter_verdict(TRUST_OR_PERMISSION_PROMPT, Some(OTHER), 0.95),
        adapter_verdict(TRUST_OR_PERMISSION_PROMPT, Some(declared.as_str()), 0.70),
    ] {
        assert!(fixture
            .service
            .route_unattended_launch_prompt(&snapshot(&tab_id, NO_FIRST), &verdict));
    }
    assert_eq!(
        crate::service::output::prompt_step_dispatch_count_for_test(&pty),
        1,
        "an abstaining model hands the gate to the provenance-bound responder, one key per frame"
    );
    let rows = journal_rows(&fixture)
        .into_iter()
        .filter(|row| row["phase"] == "owned_fallback")
        .collect::<Vec<_>>();
    assert_eq!(
        rows.iter()
            .map(|row| row["decided_by"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["fallback(abstain)", "fallback(low_confidence)"]
    );
    let ids = rows
        .iter()
        .map(|row| row["observation_id"].as_u64().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        ids.len(),
        rows.len(),
        "every decision has its own observation id"
    );
}

#[test]
fn unattended_launch_prompt_no_adapter_path_is_identical() {
    let (mut mux, tab_id) = cron_tab();
    let workspace = mux.list_workspaces()[0].id.clone();
    let visible_id = mux.create_tab(&workspace).unwrap();
    {
        let tab = mux.get_tab_mut(&visible_id).unwrap();
        tab.agent_type = Some("claude".into());
        tab.cwd = Some(JOB_DIR.into());
        tab.provisioned_cwd = Some(JOB_DIR.into());
    }
    mux.link_pty(
        &visible_id,
        format!("visible-{}", uuid::Uuid::new_v4().simple()),
    );
    let (fixture, _guard) = fixture_for(mux);
    assert!(!fixture.service.scrape_model_connected());
    assert!(!fixture.service.observe_scrape_candidate_from(
        snapshot(&tab_id, NO_FIRST),
        |_| panic!("an unconnected model never reaches a fallback"),
        ObservationTrigger::OutputCandidate,
        None,
        None,
    ));
    let unattended = fixture
        .service
        .respond_to_directory_trust(&tab_id, Some(NO_FIRST));
    let visible = fixture
        .service
        .respond_to_directory_trust(&visible_id, Some(NO_FIRST));
    assert_eq!(unattended, visible);
    assert_eq!(
        unattended,
        crate::service::directory_trust::TrustDecision::Answer {
            step: trust_target(NO_FIRST).step
        }
    );
    for id in [&tab_id, &visible_id] {
        assert_eq!(
            crate::service::output::prompt_step_dispatch_count_for_test(&pty_of(&fixture, id)),
            1
        );
    }
    assert!(journal_rows(&fixture)
        .iter()
        .all(|row| row["action"] != "owned_prompt_answer"));
}

#[test]
fn owner_launched_directory_trust_prompt_remains_owner_facing() {
    let (mux, tab_id) = unattended_tab(crate::multiplexer::types::TabAudience::User, false);
    let (fixture, _guard) = fixture_for(mux);
    let pty = pty_of(&fixture, &tab_id);
    let target = trust_target(PREAPPROVED_LOCAL_NO_FIRST);
    let verdict = adapter_verdict(
        TRUST_OR_PERMISSION_PROMPT,
        Some(target.choice.as_str()),
        0.99,
    );
    assert!(!fixture
        .service
        .route_unattended_launch_prompt(&snapshot(&tab_id, PREAPPROVED_LOCAL_NO_FIRST), &verdict));
    assert!(fixture
        .service
        .model_safe_response_target(&tab_id, PREAPPROVED_LOCAL_NO_FIRST)
        .is_none());
    fixture
        .service
        .resolve_unobserved_launch_gate(&snapshot(&tab_id, PREAPPROVED_LOCAL_NO_FIRST));
    assert_eq!(
        crate::service::output::prompt_step_dispatch_count_for_test(&pty),
        0
    );
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert!(tab.pending_prompt.is_some());
    assert_eq!(
        tab.pending_prompt_kind,
        Some(codeterm_types::pty::PromptKind::DirectoryTrust)
    );
    assert!(journal_rows(&fixture).is_empty());
}

#[test]
fn unattended_launch_prompt_permission_prompt_untouched() {
    let (mux, tab_id) = cron_tab();
    let (fixture, _guard) = fixture_for(mux);
    let pty = pty_of(&fixture, &tab_id);
    let verdict = adapter_verdict(TRUST_OR_PERMISSION_PROMPT, None, 0.99);
    assert!(!fixture
        .service
        .route_unattended_launch_prompt(&snapshot(&tab_id, TOOL_PERMISSION), &verdict));
    fixture
        .service
        .resolve_unobserved_launch_gate(&snapshot(&tab_id, TOOL_PERMISSION));
    assert!(fixture
        .service
        .model_safe_response_target(&tab_id, TOOL_PERMISSION)
        .is_none());
    assert_eq!(
        crate::service::output::prompt_step_dispatch_count_for_test(&pty),
        0
    );
    assert!(journal_rows(&fixture).is_empty());
}

#[test]
fn sentinel_respond_prompt_permission_uses_provider_declared_target() {
    use crate::agent::permission_request::PermissionRequestSignal;

    let (mut mux, tab_id) = cron_tab();
    let prompt = claude().parse_prompt(TOOL_PERMISSION).unwrap();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.pending_prompt = Some(prompt.clone());
    tab.pending_prompt_kind = Some(codeterm_types::pty::PromptKind::Provider);
    let (fixture, _guard) = fixture_for(mux);
    fixture
        .service
        .prompt_detector
        .lock()
        .hook_detected_permission_request(
            &tab_id,
            PermissionRequestSignal {
                tool_name: "Bash".into(),
                tool_input: serde_json::json!({}),
                permission_suggestions: Vec::new(),
            },
        );
    let target = fixture
        .service
        .sentinel_safe_response_target(&tab_id, TOOL_PERMISSION, Kind::Permission)
        .unwrap();
    assert_eq!(target.kind, Kind::Permission);
    assert_eq!(target.prompt, prompt);
    assert!(authorized(
        &target,
        &Evidence {
            observations: 2,
            confidence: 0.99,
            qualified: true,
            current: true,
            eligible: true,
            model_choice: Some((&target.choice, 0.99)),
        }
    ));
}

#[test]
fn sentinel_respond_prompt_preapproved_folder_trust_uses_declared_target() {
    let (mut mux, tab_id) = cron_tab();
    let prompt = claude()
        .parse_directory_trust_prompt(PREAPPROVED_LOCAL_NO_FIRST)
        .expect("pre-approved folder trust choices");
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.pending_prompt = Some(prompt.clone());
    tab.pending_prompt_kind = Some(codeterm_types::pty::PromptKind::DirectoryTrust);
    let (fixture, _guard) = fixture_for(mux);
    let pty = pty_of(&fixture, &tab_id);
    fixture.service.screen_state.lock().resize(&pty, 120, 40);
    fixture
        .service
        .screen_state
        .lock()
        .process(&pty, PREAPPROVED_LOCAL_NO_FIRST.as_bytes());
    let target = fixture
        .service
        .sentinel_safe_response_target(&tab_id, PREAPPROVED_LOCAL_NO_FIRST, Kind::DirectoryTrust)
        .expect("Sentinel resolves a provider-declared folder trust target");
    assert_eq!(target.kind, Kind::DirectoryTrust);
    assert_eq!(target.prompt, prompt);
    let affirmative = target
        .prompt
        .options
        .iter()
        .position(|option| {
            option.label.to_ascii_lowercase().starts_with("yes")
                && option.label.to_ascii_lowercase().contains("trust")
        })
        .expect("declared affirmative option");
    assert_eq!(target.prompt.options[affirmative].value, target.choice);
    assert_eq!(target.step, b"j");

    let expected = serde_json::json!({
        "kind": codeterm_types::pty::PromptKind::DirectoryTrust,
        "question": prompt.question,
        "options": prompt.options,
        "multi_select": prompt.multi_select,
    });
    assert!(matches!(
        fixture.service.request_safe_prompt_response(
            &tab_id,
            &crate::sentinel::machine::SafeKind::DirectoryTrust,
            &expected,
            "sentinel_fixture",
        ),
        Ok(crate::service::supporter_tab_state::sentinel_request::RequestEffect::Held(_))
    ));
    assert_eq!(
        crate::service::output::prompt_step_dispatch_count_for_test(&pty),
        1,
        "Sentinel's trust handler reaches the owned prompt responder once"
    );
}

#[test]
fn owner_launched_sentinel_respond_prompt_is_refused_before_dispatch() {
    let (mut mux, tab_id) = unattended_tab(crate::multiplexer::types::TabAudience::User, false);
    let prompt = claude().parse_prompt(TOOL_PERMISSION).unwrap();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.pending_prompt = Some(prompt.clone());
    tab.pending_prompt_kind = Some(codeterm_types::pty::PromptKind::Provider);
    let (fixture, _guard) = fixture_for(mux);
    let expected = serde_json::json!({
        "kind": codeterm_types::pty::PromptKind::Provider,
        "question": prompt.question,
        "options": prompt.options,
        "multi_select": prompt.multi_select,
    });
    assert_eq!(
        fixture.service.request_safe_prompt_response(
            &tab_id,
            &crate::sentinel::machine::SafeKind::Permission,
            &expected,
            "sentinel_fixture",
        ),
        Err(crate::service::supporter_tab_state::sentinel_request::RequestRefusal::UnsafeTarget)
    );
    assert_eq!(
        crate::service::output::prompt_step_dispatch_count_for_test(&pty_of(&fixture, &tab_id)),
        0
    );
}

#[test]
fn sentinel_respond_prompt_login_is_refused_before_dispatch() {
    let (mut mux, tab_id) = cron_tab();
    let pty = pty_of_raw(&mux, &tab_id);
    let prompt = claude().parse_prompt(ONBOARDING).unwrap();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.pending_prompt = Some(prompt.clone());
    tab.pending_prompt_kind = Some(codeterm_types::pty::PromptKind::Provider);
    let (fixture, _guard) = fixture_for(mux);
    fixture.service.screen_state.lock().resize(&pty, 120, 40);
    fixture.service.screen_state.lock().process(
        &pty,
        include_str!(
            "../../../../../../plugins/provider-claude/tests/fixtures/tab-state/auth-login.txt"
        )
        .as_bytes(),
    );
    let expected = serde_json::json!({
        "kind": codeterm_types::pty::PromptKind::Provider,
        "question": prompt.question,
        "options": prompt.options,
        "multi_select": prompt.multi_select,
    });
    assert_eq!(
        fixture.service.request_safe_prompt_response(
            &tab_id,
            &crate::sentinel::machine::SafeKind::Onboarding,
            &expected,
            "sentinel_fixture",
        ),
        Err(crate::service::supporter_tab_state::sentinel_request::RequestRefusal::UnsafeTarget)
    );
    assert_eq!(
        crate::service::output::prompt_step_dispatch_count_for_test(&pty),
        0
    );
}

fn pty_of_raw(mux: &crate::multiplexer::state::MultiplexerState, tab_id: &str) -> String {
    mux.get_tab(tab_id)
        .and_then(|tab| tab.pty_id.clone())
        .unwrap()
}
