use super::{
    newly_discovered_targets, offer_instance_name, update_consent, update_gate,
    NotificationOrigin, PushIntent, RestartTarget, UpdateConsent, UpdateGate, LATER,
};
use codeterm_types::pty::TabStatus;
use std::collections::HashSet;
use std::path::Path;

#[test]
fn provider_update_waits_for_owner_choice_and_idle_agents() {
    assert_eq!(update_consent(LATER), UpdateConsent::Declined);
    assert_eq!(
        update_gate(&[Some(TabStatus::Working)]),
        UpdateGate::WaitForIdle
    );
    assert_eq!(update_gate(&[Some(TabStatus::Idle)]), UpdateGate::Ready);
    assert_eq!(update_gate(&[None]), UpdateGate::WaitForIdle);
}

#[test]
fn only_default_production_instance_offers_without_opt_in() {
    assert_eq!(
        offer_instance_name(None, 7685, false).as_deref(),
        Some("production")
    );
    assert_eq!(offer_instance_name(None, 7686, false), None);
    assert_eq!(offer_instance_name(Some("dev-a"), 7685, false), None);
    assert_eq!(
        offer_instance_name(Some("dev-a"), 7686, true).as_deref(),
        Some("dev-a")
    );
}

#[test]
fn npm_windows_shim_resolves_to_a_direct_node_script() {
    let parent = Path::new("C:/Users/owner/AppData/Roaming/npm");
    let shim =
        "@ECHO off\n\"%_prog%\" \"%dp0%\\node_modules\\@openai\\codex\\bin\\codex.js\" %*\n";
    assert_eq!(
        super::npm_shim_script(parent, shim),
        Some(parent.join("node_modules/@openai/codex/bin/codex.js"))
    );
}

#[test]
fn post_lock_snapshot_keeps_new_tabs_and_skips_already_paused_tabs() {
    let target = |tab_id: &str| RestartTarget {
        tab_id: tab_id.into(),
        cwd: "cwd".into(),
        session_id: format!("session-{tab_id}"),
    };
    let paused = HashSet::from(["already-paused".to_string()]);
    let remaining = newly_discovered_targets(
        vec![target("already-paused"), target("opened-after-consent")],
        &paused,
    );
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].tab_id, "opened-after-consent");
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn http_answer_is_refused_then_owner_answer_queues_update() {
    use super::{Offer, ACCEPTED, OFFERS, UPDATE_NOW};
    use crate::plugin::agent_claude::ClaudeProvider;
    use crate::service::test_fixture::ServiceFixture;
    use codeterm_plugin::provider::ProviderUpdateAvailability;
    use codeterm_types::notification::CallerAction;
    use codeterm_types::pty::{BootStage, TabVerdict, TimelineState};
    use std::sync::Arc;

    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let tab_id = mux.create_tab(&workspace).unwrap();
    let pty_id = "update-consent-busy-pty";
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.pty_id = Some(pty_id.into());
    tab.agent_type = Some("claude".into());
    tab.boot_stage = Some(BootStage::AgentReady);
    tab.agent_state = Some(
        TabVerdict {
            state: TimelineState::Working,
            subtype: None,
        }
        .into(),
    );
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(ClaudeProvider::new()));
    fixture
        .service
        .agent_tracker
        .lock()
        .update_output(pty_id, [1u8; 32]);
    let id = fixture.service.create_notification_ext(
        "Update available".into(),
        Some("version 1 -> 2".into()),
        Some("info"),
        Some(vec![
            CallerAction::Answer {
                label: UPDATE_NOW.into(),
            },
            CallerAction::Answer {
                label: LATER.into(),
            },
        ]),
        Some(true),
        NotificationOrigin::default(),
        PushIntent {
            level: Some(crate::notification::push::NotifyLevel::ActionNeeded),
            event_key: Some("provider-update-test".into()),
        },
        None,
    );
    OFFERS.lock().insert(
        id.clone(),
        Offer {
            provider: Arc::new(ClaudeProvider::new()),
            version: ProviderUpdateAvailability {
                current_version: "1.0.0".into(),
                latest_version: "2.0.0".into(),
            },
            command: "codex update".into(),
        },
    );

    assert!(fixture
        .service
        .answer_notification(&id, UPDATE_NOW)
        .is_err());
    assert!(OFFERS.lock().contains_key(&id));
    fixture
        .service
        .answer_owner_notification(&id, UPDATE_NOW)
        .expect("owner answer should be recorded");
    assert!(ACCEPTED.lock().contains_key(&id));
    ACCEPTED.lock().remove(&id);
    OFFERS.lock().remove(&id);
}

use super::state::{Admission, Coordinator, RestartState, RunOutcome, TabReadiness};
fn idle() -> TabReadiness {
    TabReadiness { idle: true, active_turn: false, active_attempt: false }
}
fn succeeded(tabs: &[&str]) -> Coordinator {
    let mut state = Coordinator::default();
    assert_eq!(state.begin("fixture", "1", "2"), Admission::Started);
    state.finish("fixture:2", Ok(()), &tabs.iter().map(|id| id.to_string()).collect::<Vec<_>>());
    state
}

#[test]
fn update_coordinator_idle_tab_restarts_with_resume() {
    let mut state = succeeded(&["idle"]);
    assert!(state.claim_restart("fixture:2", "idle", idle()));
    state.settle_restart("fixture:2", "idle", RestartState::Resumed);
    assert_eq!(state.runs["fixture:2"].counts(), (1, 0));
}
#[test]
fn update_coordinator_busy_tab_pending_then_restarts_on_idle_event() {
    let mut state = succeeded(&["busy"]);
    assert!(!state.claim_restart("fixture:2", "busy", TabReadiness { idle: false, ..idle() }));
    assert!(!state.claim_restart("fixture:2", "busy", TabReadiness { active_turn: true, ..idle() }));
    assert!(!state.claim_restart("fixture:2", "busy", TabReadiness { active_attempt: true, ..idle() }));
    assert_eq!(state.runs["fixture:2"].counts(), (0, 1));
    assert!(state.claim_restart("fixture:2", "busy", idle()));
    state.settle_restart("fixture:2", "busy", RestartState::FreshLaunch);
    assert_eq!(state.runs["fixture:2"].counts(), (1, 0));
}
#[test]
fn update_coordinator_failure_restarts_nothing_and_notifies_once() {
    let mut state = Coordinator::default();
    state.begin("fixture", "1", "2");
    state.finish("fixture:2", Err("captured error".into()), &["idle".into()]);
    assert!(!state.claim_restart("fixture:2", "idle", idle()));
    assert_eq!(state.runs["fixture:2"].outcome, RunOutcome::Failed("captured error".into()));
    assert!(state.claim_notice("fixture:2"));
    assert!(!state.claim_notice("fixture:2"));
}
#[test]
fn update_coordinator_dedupes_across_tabs_and_restarts() {
    let mut state = Coordinator::default();
    assert_eq!(state.begin("fixture", "1", "2"), Admission::Started);
    assert_eq!(state.begin("fixture", "1", "2"), Admission::Duplicate);
    assert_eq!(state.begin("fixture", "1", "3"), Admission::ProviderBusy);
    let bytes = serde_json::to_vec(&state).unwrap();
    let mut restored: Coordinator = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(restored.begin("fixture", "1", "2"), Admission::Duplicate);
    restored.finish("fixture:2", Ok(()), &[]);
    assert_eq!(restored.begin("fixture", "2", "3"), Admission::Started);
}
#[test]
fn update_coordinator_includes_general_agent() {
    let mut state = succeeded(&["general", "project"]);
    assert!(state.claim_restart("fixture:2", "general", idle()));
    state.settle_restart("fixture:2", "general", RestartState::Resumed);
    assert_eq!(state.runs["fixture:2"].counts(), (1, 1));
}
#[test]
fn update_coordinator_one_restart_per_tab() {
    let mut state = succeeded(&["idle"]);
    assert!(state.claim_restart("fixture:2", "idle", idle()));
    assert!(!state.claim_restart("fixture:2", "idle", idle()));
    state.settle_restart("fixture:2", "idle", RestartState::Failed);
    let mut restored: Coordinator = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    assert!(!restored.claim_restart("fixture:2", "idle", idle()));
}
