use std::path::Path;

#[test]
fn only_default_production_instance_updates_without_opt_in() {
    assert_eq!(
        super::update_instance_name(None, 7685, false).as_deref(),
        Some("production")
    );
    assert_eq!(super::update_instance_name(None, 7686, false), None);
    assert_eq!(
        super::update_instance_name(Some("dev-a"), 7685, false),
        None
    );
    assert_eq!(
        super::update_instance_name(Some("dev-a"), 7686, true).as_deref(),
        Some("dev-a")
    );
}

#[test]
fn npm_windows_shim_resolves_to_a_direct_node_script() {
    let parent = Path::new("C:/Users/owner/AppData/Roaming/npm");
    let shim = "@ECHO off\n\"%_prog%\" \"%dp0%\\node_modules\\@openai\\codex\\bin\\codex.js\" %*\n";
    assert_eq!(
        super::command::npm_shim_script(parent, shim),
        Some(parent.join("node_modules/@openai/codex/bin/codex.js"))
    );
}

use super::state::{Admission, Coordinator, RestartState, RunOutcome, TabReadiness};
fn idle() -> TabReadiness {
    TabReadiness {
        idle: true,
        active_turn: false,
        active_attempt: false,
    }
}

#[test]
fn restart_guard_update_defers_until_roster_drain_event() {
    use codeterm_types::agent::background_work::{
        BackgroundRoster, BackgroundWorkObservation, BackgroundWorkState,
    };
    let mut roster = BackgroundRoster::default();
    let mut state = succeeded(&["tab"]);
    roster.observe(
        &BackgroundWorkObservation {
            id: "agent".into(),
            state: BackgroundWorkState::Running,
        },
        "",
    );
    assert!(!state.claim_restart(
        "fixture:2",
        "tab",
        idle().with_background_work(!roster.is_empty())
    ));
    roster.observe(
        &BackgroundWorkObservation {
            id: "agent".into(),
            state: BackgroundWorkState::Completed,
        },
        "",
    );
    assert!(state.claim_restart(
        "fixture:2",
        "tab",
        idle().with_background_work(!roster.is_empty())
    ));
    assert!(!state.claim_restart("fixture:2", "tab", idle()));
}
fn succeeded(tabs: &[&str]) -> Coordinator {
    let mut state = Coordinator::default();
    assert_eq!(state.begin("fixture", "1", "2"), Admission::Started);
    state.finish(
        "fixture:2",
        Ok(()),
        &tabs.iter().map(|id| id.to_string()).collect::<Vec<_>>(),
    );
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
    assert!(!state.claim_restart(
        "fixture:2",
        "busy",
        TabReadiness {
            idle: false,
            ..idle()
        }
    ));
    assert!(!state.claim_restart(
        "fixture:2",
        "busy",
        TabReadiness {
            active_turn: true,
            ..idle()
        }
    ));
    assert!(!state.claim_restart(
        "fixture:2",
        "busy",
        TabReadiness {
            active_attempt: true,
            ..idle()
        }
    ));
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
    assert_eq!(
        state.runs["fixture:2"].outcome,
        RunOutcome::Failed("captured error".into())
    );
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
    let mut restored: Coordinator =
        serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    assert!(!restored.claim_restart("fixture:2", "idle", idle()));
}

#[test]
fn update_coordinator_success_waits_for_command_completion() {
    let mut state = Coordinator::default();
    state.begin("fixture", "1", "2");
    state.enroll("fixture:2", &["idle".into()]);
    assert!(!state.claim_restart("fixture:2", "idle", idle()));
    assert!(!state.claim_notice("fixture:2"));
    state.finish("fixture:2", Ok(()), &["during-update".into()]);
    assert!(state.claim_restart("fixture:2", "idle", idle()));
    assert!(state.claim_restart("fixture:2", "during-update", idle()));
}

#[test]
fn update_coordinator_provider_scopes_are_independent() {
    let mut state = Coordinator::default();
    assert_eq!(state.begin("first", "1", "2"), Admission::Started);
    assert_eq!(state.begin("second", "1", "2"), Admission::Started);
    state.finish("first:2", Err("failure".into()), &[]);
    assert_eq!(state.begin("first", "1", "2"), Admission::Duplicate);
    assert_eq!(state.begin("first", "1", "3"), Admission::Started);
}

#[test]
fn update_coordinator_refused_busy_restart_returns_to_pending() {
    let mut state = succeeded(&["idle"]);
    assert!(state.claim_restart("fixture:2", "idle", idle()));
    state.defer_restart("fixture:2", "idle");
    assert!(state.claim_restart("fixture:2", "idle", idle()));
    state.settle_restart("fixture:2", "idle", RestartState::Resumed);
    state.defer_restart("fixture:2", "idle");
    assert!(!state.claim_restart("fixture:2", "idle", idle()));
}

#[test]
fn update_coordinator_prompt_tab_answers_safe_choice() {
    use codeterm_plugin::provider::AgentProvider;
    use codeterm_types::pty::{PendingPrompt, PromptOption};
    let make_prompt = |options: Vec<(&str, &str)>| PendingPrompt {
        question: "opaque".into(),
        options: options
            .into_iter()
            .map(|(label, value)| PromptOption {
                label: label.into(),
                value: value.into(),
            })
            .collect(),
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    };
    let provider = crate::plugin::agent_claude::ClaudeProvider::new();
    let prompt = make_prompt(vec![("Update now", "apply"), ("Skip", "defer")]);
    assert_eq!(
        provider.update_prompt_safe_choice(&prompt).as_deref(),
        Some("defer")
    );
    let missing = make_prompt(vec![("Update now", "apply")]);
    assert_eq!(provider.update_prompt_safe_choice(&missing), None);
}

#[test]
fn update_coordinator_unstopped_updater_blocks_further_runs() {
    let mut state = Coordinator::default();
    state.begin("fixture", "1", "2");
    state.finish(
        "fixture:2",
        Err("termination unconfirmed".into()),
        &["idle".into()],
    );
    state.block_unstopped("fixture:2");
    assert_eq!(state.begin("fixture", "1", "3"), Admission::ProviderBusy);
    assert!(!state.claim_restart("fixture:2", "idle", idle()));
    assert!(state.claim_notice("fixture:2"));
    assert!(!state.claim_notice("fixture:2"));
}

#[test]
fn update_coordinator_journal_correlates_provider_target_and_tab() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = crate::service::supporter::SupporterRuntime::new(dir.path().into());
    runtime
        .provider_update_action(
            "fixture",
            Some("2"),
            crate::service::supporter::ActionRecord {
                kind: crate::service::supporter::VerdictKind::TabState,
                tab: Some("tab-a"),
                label: "resumed",
                confidence: 1.0,
                action: crate::service::supporter::Action::Recovery,
                reason: "provider_update_restart",
                asked: None,
            },
        )
        .unwrap();
    let contents = std::fs::read_to_string(dir.path().join("journal.jsonl")).unwrap();
    let entry: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
    assert_eq!(entry["provider"], "fixture");
    assert_eq!(entry["target_version"], "2");
    assert_eq!(entry["tab"], "tab-a");
    assert_eq!(entry["label"], "resumed");
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn update_coordinator_service_idle_general_and_busy_projection() {
    use crate::plugin::agent_claude::ClaudeProvider;
    use crate::service::test_fixture::ServiceFixture;
    use codeterm_types::pty::{BootStage, TabVerdict, TimelineState};
    let (mut mux, root) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    mux.set_workspace_protected(&workspace, true);
    crate::service::general_agent::ensure_general_workspace(&mut mux, false);
    assert_eq!(
        mux.resolve_tab_id(crate::service::general_agent::GENERAL_ALIAS),
        Some(root.clone())
    );
    let busy = mux.create_tab(&workspace).unwrap();
    for (id, pty, state) in [
        (&root, "update-root-pty", TimelineState::Idle),
        (&busy, "update-busy-pty", TimelineState::Working),
    ] {
        let tab = mux.get_tab_mut(id).unwrap();
        tab.agent_type = Some("claude".into());
        tab.pty_id = Some(pty.into());
        tab.boot_stage = Some(BootStage::AgentReady);
        tab.agent_state = Some(
            TabVerdict {
                state,
                subtype: None,
            }
            .into(),
        );
    }
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(ClaudeProvider::new()));
    for pty in ["update-root-pty", "update-busy-pty"] {
        fixture
            .service
            .agent_tracker
            .lock()
            .update_output(pty, [1u8; 32]);
    }
    fixture
        .service
        .agent_tracker
        .lock()
        .set_idle("update-root-pty");
    assert!(super::restarts::provider_tabs(&fixture.service, "claude").contains(&root));
    assert!(super::restarts::readiness(&fixture.service, &root).can_restart());
    assert!(!super::restarts::readiness(&fixture.service, &busy).can_restart());
    assert!(!super::restart_eligible(&fixture.service, "absent"));
    let mut plan = crate::plan::Plan::new(
        "fixture".into(),
        fixture.temp.path().display().to_string(),
        crate::plan::OrchestratorRef {
            tab_id: None,
            session_id: None,
            title: None,
        },
    );
    plan.status = crate::plan::PlanStatus::Active;
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan.id,
            crate::service::plan::NewTask {
                name: "fixture".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    plan = fixture.service.plan_store.get(&plan.id).unwrap();
    let task = plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.status = crate::plan::TaskStatus::InProgress;
    task.execution_attempts
        .push(crate::plan::TaskExecutionAttempt {
            executor_tab_id: root.clone(),
            ..Default::default()
        });
    fixture.service.plan_store.save(&mut plan, 3).unwrap();
    assert!(!super::restarts::readiness(&fixture.service, &root).can_restart());
    plan.tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap()
        .status = crate::plan::TaskStatus::InReview;
    fixture.service.plan_store.save(&mut plan, 4).unwrap();
    assert!(super::restarts::readiness(&fixture.service, &root).can_restart());
}

#[test]
fn update_coordinator_interrupted_claims_never_replay() {
    let mut state = succeeded(&["idle"]);
    assert!(state.claim_restart("fixture:2", "idle", idle()));
    let interrupted = state.recover_interrupted(|_| false);
    assert_eq!(interrupted.len(), 1);
    assert_eq!(state.runs["fixture:2"].tabs["idle"], RestartState::Failed);
    assert!(!state.claim_restart("fixture:2", "idle", idle()));
    assert!(state.recover_interrupted(|_| false).is_empty());
    assert_eq!(state.begin("fixture", "2", "3"), Admission::Started);
    assert_eq!(state.recover_interrupted(|_| false).len(), 1);
    assert_eq!(state.begin("fixture", "2", "3"), Admission::Duplicate);
    assert_eq!(state.begin("fixture", "2", "4"), Admission::ProviderBusy);
}

#[test]
fn update_coordinator_machine_claim_dedupes_concurrent_tabs_and_disk_reload() {
    use std::sync::{Arc, Barrier};
    let dir = tempfile::tempdir().unwrap();
    let barrier = Arc::new(Barrier::new(4));
    let threads = (0..4)
        .map(|index| {
            let barrier = barrier.clone();
            let path = dir.path().to_path_buf();
            std::thread::spawn(move || {
                barrier.wait();
                super::storage::transact_in(&path, |state| {
                    let admission = state.begin("fixture", "1", "2");
                    state.enroll("fixture:2", &[format!("tab-{index}")]);
                    admission
                })
                .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let admissions = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        admissions
            .iter()
            .filter(|admission| **admission == Admission::Started)
            .count(),
        1
    );
    assert_eq!(
        admissions
            .iter()
            .filter(|admission| **admission == Admission::Duplicate)
            .count(),
        3
    );
    super::storage::transact_in(dir.path(), |state| {
        assert_eq!(state.runs["fixture:2"].tabs.len(), 4);
        assert_eq!(state.begin("fixture", "1", "2"), Admission::Duplicate);
        assert_eq!(state.begin("fixture", "1", "3"), Admission::ProviderBusy);
        state.finish("fixture:2", Ok(()), &[]);
    })
    .unwrap();
    super::storage::transact_in(dir.path(), |state| {
        assert_eq!(state.begin("fixture", "1", "2"), Admission::Duplicate);
        assert!(state.claim_restart("fixture:2", "tab-0", idle()));
        state.settle_restart("fixture:2", "tab-0", RestartState::Resumed);
    })
    .unwrap();
    super::storage::transact_in(dir.path(), |state| {
        assert!(!state.claim_restart("fixture:2", "tab-0", idle()));
    })
    .unwrap();
}

#[test]
fn update_coordinator_corrupt_durable_claim_refuses_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(super::storage::STATE_FILE), b"invalid-json").unwrap();
    let called = std::sync::atomic::AtomicBool::new(false);
    let result = super::storage::transact_in(dir.path(), |_| {
        called.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    assert!(result.is_err());
    assert!(!called.load(std::sync::atomic::Ordering::Relaxed));
}
