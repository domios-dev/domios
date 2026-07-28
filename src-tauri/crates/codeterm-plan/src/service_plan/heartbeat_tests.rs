use super::{NewTask, PlanStore};
use crate::plan::{AgentRef, OrchestratorRef, PlanMode, PlannerRef};

fn store_with_plan(owner: &str) -> (tempfile::TempDir, PlanStore, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan = store
        .create_plan(
            "Heartbeat".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab(owner),
            vec![],
            10,
        )
        .unwrap();
    store.set_mode(&plan.id, PlanMode::Execution, 11).unwrap();
    (dir, store, plan.id)
}

#[test]
fn heartbeat_learning_persists_reloads_and_dedups() {
    let (dir, store, plan_id) = store_with_plan("orch-a");
    let first = store
        .add_heartbeat_learning(
            Some(&plan_id),
            None,
            None,
            "Always verify delayed reseed text at send time.",
            Some("reseed"),
            20,
        )
        .unwrap();
    assert!(!first.deduped);

    let duplicate = store
        .add_heartbeat_learning(
            Some(&plan_id),
            None,
            None,
            "always verify delayed reseed text at send time",
            Some("reseed"),
            30,
        )
        .unwrap();
    assert!(duplicate.deduped);
    assert_eq!(duplicate.learning.id, first.learning.id);

    store.plans.write().clear();
    store.reload_from_disk(&plan_id).unwrap();
    assert_eq!(store.get(&plan_id).unwrap().heartbeat.learnings.len(), 1);

    let reloaded = PlanStore::new_for_test(dir.path());
    reloaded.scan_project(dir.path()).unwrap();
    let shown = reloaded.show_heartbeat(Some(&plan_id), None, None).unwrap();
    assert_eq!(shown.heartbeat.learnings[0].text, first.learning.text);
    assert!(shown.reinjection.contains("Recent learnings"));
}

#[test]
fn heartbeat_lists_plan_skill_with_persisted_purpose() {
    let (_dir, store, plan_id) = store_with_plan("orch-a");
    let created = super::plan_skills::create_plan_skill(
        &store,
        &plan_id,
        super::plan_skills::NewPlanSkill {
            slug: "recovery-check".into(),
            title: Some("Recovery Check".into()),
            area: Some("testing".into()),
            check: None,
            rationale: Some("captured".into()),
        },
    )
    .unwrap();
    std::fs::write(
        created.skill_md,
        "---\ntitle: Recovery Check\ndescription: Restore the fixture and verify recovery.\n---\n",
    )
    .unwrap();
    store
        .add_heartbeat_learning(
            Some(&plan_id),
            None,
            None,
            "Keep the heartbeat visible.",
            None,
            20,
        )
        .unwrap();

    let shown = store.show_heartbeat(Some(&plan_id), None, None).unwrap();

    assert!(shown
        .reinjection
        .contains("Plan skills (1): recovery-check: Restore the fixture and verify recovery."));
}

#[test]
fn heartbeat_renders_live_competitive_attempt_summary() {
    let (_dir, store, plan_id) = store_with_plan("orch-a");
    let task_id = store
        .add_task(
            &plan_id,
            NewTask {
                name: "competitive lane".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            20,
        )
        .unwrap();
    store
        .assign_task(&plan_id, &task_id, "worker-a", None, 21)
        .unwrap();
    store
        .assign_task(&plan_id, &task_id, "worker-b", None, 22)
        .unwrap();

    let shown = store.show_heartbeat(Some(&plan_id), None, None).unwrap();

    assert!(shown
        .reinjection
        .contains(super::prompts_plan::HEARTBEAT_COMPETITIVE_ATTEMPTS_TITLE));
    assert!(shown
        .reinjection
        .contains(&super::heartbeat::competitive_attempt_summary_line(
            &task_id,
            "competitive lane",
            2
        )));
}

#[test]
fn heartbeat_resolution_prefers_execution_plan_with_open_work() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let stale = store
        .create_plan(
            "Stale owner plan".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch-a"),
            vec![],
            10,
        )
        .unwrap();
    store
        .with_plan(&stale.id, 11, |plan| {
            plan.heartbeat.current_focus = Some("old focus".into());
            Ok(())
        })
        .unwrap();
    let live = store
        .create_plan(
            "Live execution plan".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch-a"),
            vec![],
            20,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &live.id,
            NewTask {
                name: "open work".into(),
                kind: "implement".into(),
                competition: None,
                ..Default::default()
            },
            21,
        )
        .unwrap();
    store.set_mode(&live.id, PlanMode::Execution, 22).unwrap();
    store
        .with_plan(&live.id, 23, |plan| {
            plan.heartbeat.current_focus = Some("live focus".into());
            Ok(())
        })
        .unwrap();

    let shown = store.show_heartbeat(None, Some("orch-a"), None).unwrap();
    assert_eq!(shown.plan_id, live.id);
    assert_eq!(shown.heartbeat.current_focus, None);

    let text = store.prepend_heartbeat_for_orchestrator_tab("orch-a", "continue");
    assert!(text.contains(&format!("plan=\"{}\"", live.id)));
    assert!(!text.contains("Current focus:"));
    assert!(!text.contains("old focus"));

    store
        .close_task(&live.id, &task_id, Some("done"), Default::default(), 24)
        .unwrap();
    let shown_after_close = store.show_heartbeat(None, Some("orch-a"), None).unwrap();
    assert_eq!(shown_after_close.plan_id, live.id);
}

#[test]
fn paused_plan_is_not_resolved_or_prepended_for_heartbeat() {
    let (_dir, store, plan_id) = store_with_plan("orch-paused");
    let task_id = store
        .add_task(
            &plan_id,
            NewTask {
                name: "open work".into(),
                kind: "implement".into(),
                competition: None,
                ..Default::default()
            },
            20,
        )
        .unwrap();
    store
        .assign_task(&plan_id, &task_id, "worker", None, 21)
        .unwrap();
    store
        .pause_plan(
            &plan_id,
            "waiting on quota",
            AgentRef::from_tab("orch-paused"),
            22,
        )
        .unwrap();

    assert_eq!(
        store.prepend_heartbeat_for_orchestrator_tab("orch-paused", "continue"),
        "continue"
    );
    assert_eq!(
        store.prepend_heartbeat_for_plan(&plan_id, "continue"),
        "continue"
    );
    assert!(store
        .resolve_active_plan_for_orchestrator(None, Some("orch-paused"), None)
        .is_err());
}

#[test]
fn heartbeat_resolves_for_owner_tab_at_send_time() {
    let (_dir, store, plan_id) = store_with_plan("orch-a");
    let before = store.prepend_heartbeat_for_orchestrator_tab("orch-a", "continue");
    assert!(before.starts_with("<codeterm-role-identity"));
    assert!(before.contains(&format!("You are the Orchestrator for plan {plan_id}")));
    assert!(before.contains("STATE-RECONCILIATION"));
    assert!(before.contains("Worker reports are pushed into this tab automatically"));
    assert!(!before.contains("codeterm agent reports"));
    assert!(before.contains("codeterm orchestrator status --plan"));
    assert!(before.contains("--tab orch-a"));
    assert!(!before.contains("--tab orch-a --json"));
    assert!(before.contains("codeterm orchestrator inspect --plan"));
    assert!(before.contains("codeterm plan reconcile"));
    assert!(before.contains("codeterm plan task close"));
    assert!(before.contains("codeterm send \"<next concrete instruction>\" --tab"));
    assert!(before.contains("Heartbeat is derived automatically"));
    assert!(before.contains("codeterm docs ORCHESTRATION"));
    assert!(!before.contains("Re-arm exactly one heartbeat"));
    assert!(before.ends_with("continue"));

    store
        .with_plan(&plan_id, 10, |plan| {
            plan.heartbeat.current_focus = Some("Review worker branches".into());
            plan.heartbeat.active_lanes = vec!["backend tests".into(), "frontend canvas".into()];
            plan.heartbeat.rules = vec!["Workers report in_review only".into()];
            Ok(())
        })
        .unwrap();
    store
        .add_heartbeat_learning(
            Some(&plan_id),
            None,
            None,
            "Workers report in_review, orchestrator verifies done.",
            None,
            20,
        )
        .unwrap();
    store
        .add_heartbeat_learning(
            Some(&plan_id),
            None,
            None,
            "workers report in review orchestrator verifies done",
            None,
            30,
        )
        .unwrap();
    let after = store.prepend_heartbeat_for_orchestrator_tab("orch-a", "continue");
    assert!(after.contains("<codeterm-state-reconciliation"));
    assert!(after.contains("<codeterm-orchestrator-heartbeat"));
    assert!(!after.contains("Current focus:"));
    assert!(!after.contains("Active lanes:"));
    assert!(!after.contains("Rules:"));
    assert!(after.contains("Workers report in_review"));
    assert_eq!(
        after
            .matches("- Workers report in_review, orchestrator verifies done.")
            .count(),
        1
    );
    assert!(after.ends_with("continue"));

    store
        .with_plan(&plan_id, 40, |plan| {
            plan.heartbeat.current_focus = Some("Integrate verified branches".into());
            Ok(())
        })
        .unwrap();
    let resolved_later = store.prepend_heartbeat_for_orchestrator_tab("orch-a", "continue");
    assert!(!resolved_later.contains("Current focus:"));

    let worker_text = store.prepend_heartbeat_for_orchestrator_tab("worker-a", "continue");
    assert_eq!(worker_text, "continue");
}

#[test]
fn notifier_delivery_uses_same_state_reconciliation_prelude() {
    let (_dir, store, plan_id) = store_with_plan("orch-a");
    let text = store.prepend_heartbeat_for_plan(
        &plan_id,
        "[task in_review] 'Backend' (task-a) -> in_review by worker-a\nReview it.",
    );

    assert!(text.contains("<codeterm-state-reconciliation"));
    assert!(text.contains("<codeterm-role-identity"));
    assert!(text.contains("STATE-RECONCILIATION"));
    assert!(text.contains("execution-mode plan with unfinished work"));
    assert!(text.contains("codeterm docs ORCHESTRATION"));
    assert!(text.contains("Reconcile live tabs against the plan"));
    assert!(text.contains("codeterm plan task close"));
    assert!(text.contains("codeterm plan task update"));
    assert!(text.contains("[task in_review]"));
}

#[test]
fn owner_plan_nudge_prepends_heartbeat_and_exact_recovery_commands() {
    let (_dir, store, plan_id) = store_with_plan("orch-a");
    store
        .with_plan(&plan_id, 20, |plan| {
            plan.heartbeat.current_focus = Some("Review open lanes".into());
            plan.heartbeat.active_lanes = vec!["owner-only".into()];
            Ok(())
        })
        .unwrap();

    let text = store.prepend_heartbeat_for_plan(
        &plan_id,
        &super::reminders::nudge_text(super::reminders::NudgeKind::Orchestrator),
    );

    assert!(text.starts_with("<codeterm-role-identity"));
    assert!(text.contains(&format!("You are the Orchestrator for plan {plan_id}")));
    assert!(text.contains("Worker reports are pushed into this tab automatically"));
    assert!(!text.contains("codeterm agent reports"));
    assert!(text.contains(&format!(
        "codeterm orchestrator status --plan {plan_id} --tab orch-a"
    )));
    assert!(text.contains(&format!("codeterm plan task close {plan_id} <task_id>")));
    assert!(text.contains("codeterm send \"<next concrete instruction>\" --tab <worker_tab_id>"));
    assert!(text.contains("Heartbeat is derived automatically"));
    assert!(text.contains("codeterm docs ORCHESTRATION"));
    assert!(!text.contains("<codeterm-orchestrator-heartbeat"));
    assert!(!text.contains("Current focus:"));
    assert!(text.contains("Plan reminder: this tab owns unfinished execution-plan work."));
}

#[test]
fn role_identity_names_planner_peer_across_reinjection() {
    let (_dir, store, plan_id) = store_with_plan("orch-a");
    store
        .with_plan(&plan_id, 20, |plan| {
            plan.planner = Some(PlannerRef::Tab {
                tab_id: "planner-a".into(),
                session_id: None,
                title: Some("Planner".into()),
            });
            Ok(())
        })
        .unwrap();

    let text = store.prepend_heartbeat_for_plan(&plan_id, "resume");

    assert!(text.contains(&format!("You are the Orchestrator for plan {plan_id}")));
    assert!(text.contains("peer tab Planner is tab planner-a"));
    assert!(text.contains("<codeterm-state-reconciliation"));
}

#[test]
fn all_tasks_done_plan_produces_no_reconciliation_prepend() {
    let (_dir, store, plan_id) = store_with_plan("orch-a");
    let task_id = store
        .add_task(
            &plan_id,
            NewTask {
                name: "the only task".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            20,
        )
        .unwrap();
    store
        .close_task(
            &plan_id,
            &task_id,
            Some("done"),
            AgentRef::from_tab("orch-a"),
            30,
        )
        .unwrap();

    let nudge_body = "Plan reminder: this tab owns unfinished execution-plan work.";
    let result = store.prepend_heartbeat_for_plan(&plan_id, nudge_body);
    assert_eq!(
        result, nudge_body,
        "complete plan must not inject reconciliation"
    );
    assert!(!result.contains("STATE-RECONCILIATION"));
    assert!(!result.contains("codeterm-state-reconciliation"));

    let tab_result = store.prepend_heartbeat_for_orchestrator_tab("orch-a", nudge_body);
    assert_eq!(
        tab_result, nudge_body,
        "complete plan must be silent via orchestrator tab path"
    );
}

#[test]
fn explicit_complete_mode_plan_produces_no_reconciliation_prepend() {
    let (_dir, store, plan_id) = store_with_plan("orch-a");
    store.set_mode(&plan_id, PlanMode::Complete, 20).unwrap();

    let nudge_body = "some message";
    let result = store.prepend_heartbeat_for_plan(&plan_id, nudge_body);
    assert_eq!(result, nudge_body);
    assert!(!result.contains("STATE-RECONCILIATION"));
}

#[test]
fn open_work_plan_still_gets_reconciliation_prepend() {
    let (_dir, store, plan_id) = store_with_plan("orch-a");
    store
        .add_task(
            &plan_id,
            NewTask {
                name: "open task".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            20,
        )
        .unwrap();

    // Open work (pending) keeps the plan non-complete so reconciliation still fires.
    let result = store.prepend_heartbeat_for_plan(&plan_id, "text");
    assert!(
        result.contains("STATE-RECONCILIATION"),
        "open work must not be silenced"
    );
}

#[test]
fn is_plan_complete_predicate_covers_execution_all_done_and_explicit_complete() {
    use super::heartbeat::is_plan_complete;

    // Empty plan in Execution — not "complete" (no tasks done yet, just empty)
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan = store
        .create_plan(
            "T".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            vec![],
            1,
        )
        .unwrap();
    store.set_mode(&plan.id, PlanMode::Execution, 2).unwrap();
    let empty_plan = store.get(&plan.id).unwrap();
    assert!(
        !is_plan_complete(&empty_plan),
        "empty Execution plan is not complete"
    );

    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "t".into(),
                kind: "impl".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    store
        .close_task(&plan.id, &task_id, None, AgentRef::from_tab("orch"), 4)
        .unwrap();
    let done_plan = store.get(&plan.id).unwrap();
    assert!(
        is_plan_complete(&done_plan),
        "all-done Execution plan is complete"
    );

    store.set_mode(&plan.id, PlanMode::Complete, 5).unwrap();
    let complete_plan = store.get(&plan.id).unwrap();
    assert!(
        is_plan_complete(&complete_plan),
        "Complete mode with terminal tasks is complete"
    );
}

#[test]
fn is_plan_complete_rejects_paused_and_blocked_tasks() {
    use super::heartbeat::is_plan_complete;

    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan = store
        .create_plan(
            "Deferred".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            vec![],
            1,
        )
        .unwrap();
    store.set_mode(&plan.id, PlanMode::Execution, 2).unwrap();
    let paused_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "paused".into(),
                kind: "impl".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    store
        .pause_task(
            &plan.id,
            &paused_id,
            "waiting for quota reset",
            AgentRef::from_tab("orch"),
            4,
        )
        .unwrap();

    let paused_plan = store.get(&plan.id).unwrap();
    assert!(
        !is_plan_complete(&paused_plan),
        "paused work is deferred, not complete"
    );

    let blocked_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "blocked".into(),
                kind: "impl".into(),
                ..Default::default()
            },
            5,
        )
        .unwrap();
    store
        .update_task(
            &plan.id,
            &blocked_id,
            None,
            None,
            None,
            None,
            Some("blocked"),
            Some(Some("waiting on external input")),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            None,
            None,
            6,
        )
        .unwrap();

    let blocked_plan = store.get(&plan.id).unwrap();
    assert!(
        !is_plan_complete(&blocked_plan),
        "blocked work is waiting, not complete"
    );
}

#[test]
fn heartbeat_renders_digest_lines_without_other_state() {
    let (_dir, store, plan_id) = store_with_plan("orch-a");
    let digest_lines = vec![
        "5 reports await your verdict, oldest 2h — judge".into(),
        "claude: 62% weekly".into(),
    ];
    let text = store.prepend_heartbeat_for_plan_with_digests(&plan_id, "continue", &digest_lines);
    assert!(text.contains("Digests:"));
    assert!(text.contains("5 reports await your verdict"));
    assert!(text.contains("claude: 62% weekly"));
}
