use super::*;
use crate::agent::status::TabStatus;
use crate::agent::tab_state::{
    DelayedResume, Surface, TabState, TabStateAction, TabStateClassification,
};
use crate::agent::test_provider::MockProvider;
use codeterm_types::pty::{PendingPrompt, PromptOption};

fn classification(
    state: TabState,
    confidence: f32,
    action: Option<TabStateAction>,
) -> TabStateClassification {
    TabStateClassification {
        state,
        confidence,
        data: None,
        action,
    }
}

fn keys_action(keys: &[&str]) -> TabStateAction {
    TabStateAction {
        keys: Some(keys.iter().map(|k| k.to_string()).collect()),
        delayed_resume: None,
        surface: None,
    }
}

#[test]
fn does_not_fire_while_working() {
    // Gate: even a confident, actionable verdict is ignored while streaming.
    let c = classification(
        TabState::UpdatePrompt,
        0.99,
        Some(keys_action(&["down", "enter"])),
    );
    assert_eq!(
        decide(TabStatus::Working, Some(&c)),
        ReactorDecision::Ignore
    );
}

#[test]
fn does_not_fire_when_dead() {
    let c = classification(TabState::UpdatePrompt, 0.99, Some(keys_action(&["enter"])));
    assert_eq!(decide(TabStatus::Dead, Some(&c)), ReactorDecision::Ignore);
}

#[test]
fn confidence_below_threshold_yields_no_action() {
    let c = classification(
        TabState::UpdatePrompt,
        GLOBAL_THRESHOLD - 0.01,
        Some(keys_action(&["down", "enter"])),
    );
    assert_eq!(decide(TabStatus::Idle, Some(&c)), ReactorDecision::Ignore);
}

#[test]
fn no_classification_yields_no_action() {
    assert_eq!(decide(TabStatus::Idle, None), ReactorDecision::Ignore);
    assert_eq!(decide(TabStatus::Waiting, None), ReactorDecision::Ignore);
}

#[test]
fn waiting_on_auth_is_never_auto_acted_surface_only() {
    // press a key — it surfaces only.
    let surface = Surface {
        kind: "auth".to_string(),
        payload: serde_json::json!({ "msg": "paste your token" }),
    };
    let action = TabStateAction {
        keys: Some(vec!["enter".to_string()]),
        delayed_resume: None,
        surface: Some(surface.clone()),
    };
    let c = classification(TabState::WaitingOnAuth, 0.99, Some(action));
    assert_eq!(
        decide(TabStatus::Idle, Some(&c)),
        ReactorDecision::Surface(Some(surface))
    );
}

#[test]
fn waiting_on_auth_with_no_surface_still_surfaces() {
    let action = keys_action(&["enter"]);
    let c = classification(TabState::WaitingOnAuth, 1.0, Some(action));
    assert_eq!(
        decide(TabStatus::Waiting, Some(&c)),
        ReactorDecision::Surface(None)
    );
}

#[test]
fn high_confidence_keys_action_is_executed() {
    let action = keys_action(&["down", "enter"]);
    let c = classification(TabState::IdleStuck, GLOBAL_THRESHOLD, Some(action.clone()));
    assert_eq!(
        decide(TabStatus::Idle, Some(&c)),
        ReactorDecision::Act(action)
    );
}

#[test]
fn high_confidence_action_with_keys_dispatches_key_injection_bytes() {
    // `send --actions` path injects: arrow-down then carriage return.
    let action = keys_action(&["down", "enter"]);
    let ReactorDecision::Act(decided) = decide(
        TabStatus::Waiting,
        Some(&classification(TabState::IdleStuck, 0.95, Some(action))),
    ) else {
        panic!("expected Act decision");
    };
    let writes = action_to_writes(&decided);
    assert_eq!(writes, vec![b"\x1b[B".to_vec(), b"\r".to_vec()]);
}

#[test]
fn action_with_no_keys_produces_no_writes() {
    let action = TabStateAction {
        keys: None,
        delayed_resume: Some(DelayedResume {
            at: "2026-06-19T16:20:00+03:00".to_string(),
            text: "continue your work".to_string(),
            then_keys: None,
        }),
        surface: None,
    };
    assert!(action_to_writes(&action).is_empty());
}

#[test]
fn unknown_keys_are_skipped_not_injected() {
    let action = keys_action(&["down", "frobnicate", "enter"]);
    let writes = action_to_writes(&action);
    assert_eq!(writes, vec![b"\x1b[B".to_vec(), b"\r".to_vec()]);
}

#[test]
fn update_prompt_classification_is_a_manual_prompt_guard() {
    let prompt = classification(TabState::UpdatePrompt, GLOBAL_THRESHOLD, None);
    let weak = classification(TabState::UpdatePrompt, GLOBAL_THRESHOLD - 0.01, None);
    let working = classification(TabState::Working, 1.0, None);
    assert!(is_manual_update_prompt(Some(&prompt)));
    assert!(!is_manual_update_prompt(Some(&weak)));
    assert!(!is_manual_update_prompt(Some(&working)));
    assert!(!is_manual_update_prompt(None));
}

#[test]
fn update_prompt_without_an_explicit_update_action_is_never_acted() {
    let prompt = classification(TabState::UpdatePrompt, 1.0, None);
    assert_eq!(
        decide(TabStatus::Idle, Some(&prompt)),
        ReactorDecision::Ignore
    );
    let keyed = classification(TabState::UpdatePrompt, 1.0, Some(keys_action(&["enter"])));
    assert_eq!(
        decide(TabStatus::Idle, Some(&keyed)),
        ReactorDecision::Ignore
    );
}

#[test]
fn rfc3339_reset_time_parses_to_epoch_ms() {
    // The +03:00 offset is honored: same instant as 13:20:00 UTC.
    let ms = parse_iso_to_ms("2026-06-19T16:20:00+03:00").expect("parse");
    let expected = chrono::DateTime::parse_from_rfc3339("2026-06-19T13:20:00Z")
        .unwrap()
        .timestamp_millis() as u64;
    assert_eq!(ms, expected);
}

#[test]
fn garbage_reset_time_is_rejected() {
    assert_eq!(parse_iso_to_ms("4:20pm (Europe/Riga)"), None);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn live_codex_trust_frame_becomes_pending_prompt_through_state_reactor() {
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../plugins/provider-codex/tests/fixtures/directory-trust/trust-folder-access-0.157.1.txt");
    let screen = std::fs::read_to_string(fixture_path).expect("captured Codex folder trust frame");
    let options = screen
        .lines()
        .filter_map(|line| {
            let row = line.trim().strip_prefix('›').unwrap_or(line.trim()).trim_start();
            let digits = row.chars().take_while(char::is_ascii_digit).count();
            let value = row.get(..digits)?.parse::<usize>().ok()?;
            let label = row.get(digits..)?.trim_start().strip_prefix('.')?.trim_start();
            (!label.is_empty()).then(|| PromptOption {
                label: label.to_string(),
                value: value.to_string(),
            })
        })
        .collect::<Vec<_>>();
    let prompt = PendingPrompt {
        question: "folder access choice".into(),
        options,
        sections: None,
        multi_select: Some(false),
        submit_available: Some(false),
        selected_values: Some(vec!["1".into()]),
    };
    let provider = MockProvider::new("trust-fixture")
        .with_tab_state(TabStateClassification {
            state: TabState::ClarifyingQuestion,
            confidence: 1.0,
            data: None,
            action: Some(TabStateAction {
                keys: None,
                delayed_resume: None,
                surface: Some(Surface {
                    kind: "clarifying_question".into(),
                    payload: serde_json::json!({"questions": ["folder access choice"]}),
                }),
            }),
        })
        .with_parsed_prompt(prompt.clone());
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let pty_id = "pty-live-trust-frame";
    mux.link_pty(&tab_id, pty_id.into());
    let tab = mux.get_tab_mut(&tab_id).expect("tab");
    tab.agent_type = Some("trust-fixture".into());
    tab.cwd = Some("C:\\Users\\User\\workspace".into());
    tab.auto_approve = true;
    tab.permission_auto_confirm = true;

    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(provider));
    fixture
        .service
        .screen_state
        .lock()
        .process(pty_id, screen.as_bytes());
    let service = std::sync::Arc::new(fixture.service.clone());

    react(&service, pty_id, TabStatus::Waiting);

    let pending = fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .and_then(|tab| tab.pending_prompt.clone())
        .expect("the state reactor routes provider choices into tab prompt state");
    assert_eq!(pending, prompt);
}
