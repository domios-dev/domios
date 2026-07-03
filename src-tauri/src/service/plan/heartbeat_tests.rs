use super::{NewTask, PlanStore};
use crate::plan::{AgentRef, OrchestratorRef, Plan, PlanMode};

fn store_with_plan(owner: &str) -> (tempfile::TempDir, PlanStore, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan = store
        .create_plan(
            "Heartbeat".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_pane(owner),
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

    let raw = std::fs::read_to_string(
        dir.path()
            .join(".codeterm")
            .join("plans")
            .join("active")
            .join(&plan_id)
            .join("plan.json"),
    )
    .unwrap();
    let plan: Plan = serde_json::from_str(&raw).unwrap();
    assert_eq!(plan.heartbeat.learnings.len(), 1);

    let reloaded = PlanStore::new_for_test(dir.path());
    reloaded.scan_project(dir.path()).unwrap();
    let shown = reloaded.show_heartbeat(Some(&plan_id), None, None).unwrap();
    assert_eq!(shown.heartbeat.learnings[0].text, first.learning.text);
    assert!(shown.reinjection.contains("Recent learnings"));
}

#[test]
fn heartbeat_set_writes_focus_lanes_rules_and_reinjects() {
    let (_dir, store, plan_id) = store_with_plan("orch-a");

    let result = store
        .set_heartbeat(
            Some(&plan_id),
            None,
            None,
            Some("Land lane fixes"),
            Some(vec!["lane-a: cli".into(), "  ".into()]),
            Some(vec!["never merge before gate green".into()]),
            false,
            false,
            20,
        )
        .unwrap();
    assert_eq!(
        result.heartbeat.current_focus.as_deref(),
        Some("Land lane fixes")
    );
    assert_eq!(
        result.heartbeat.active_lanes,
        vec!["lane-a: cli".to_string()]
    );
    assert_eq!(
        result.heartbeat.rules,
        vec!["never merge before gate green".to_string()]
    );

    // Auto-reinjection now carries the focus + lanes set above.
    let reinjected = store.prepend_heartbeat_for_orchestrator_pane("orch-a", "continue");
    assert!(reinjected.contains("Current focus: Land lane fixes"));
    assert!(reinjected.contains("lane-a: cli"));

    // Partial set leaves unspecified fields intact; --clear-lanes empties lanes.
    let cleared = store
        .set_heartbeat(
            Some(&plan_id),
            None,
            None,
            None,
            None,
            None,
            true,
            false,
            30,
        )
        .unwrap();
    assert_eq!(
        cleared.heartbeat.current_focus.as_deref(),
        Some("Land lane fixes")
    );
    assert!(cleared.heartbeat.active_lanes.is_empty());
    assert_eq!(
        cleared.heartbeat.rules,
        vec!["never merge before gate green".to_string()]
    );
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
        .contains(crate::prompts::HEARTBEAT_COMPETITIVE_ATTEMPTS_TITLE));
    assert!(shown
        .reinjection
        .contains(&super::heartbeat::competitive_attempt_summary_line(
            &task_id,
            "competitive lane",
            2
        )));
}

#[test]
fn planning_mode_rejects_runtime_heartbeat_state() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan = store
        .create_plan(
            "Planning".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_pane("orch-a"),
            vec![],
            10,
        )
        .unwrap();
    let err = store
        .set_heartbeat(
            Some(&plan.id),
            None,
            None,
            Some("remember this"),
            None,
            None,
            false,
            false,
            20,
        )
        .unwrap_err();
    assert!(matches!(err, super::PlanError::HeartbeatRequiresExecution));
}

#[test]
fn heartbeat_resolution_prefers_execution_plan_with_open_work() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let stale = store
        .create_plan(
            "Stale owner plan".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_pane("orch-a"),
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
            OrchestratorRef::from_pane("orch-a"),
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
    assert_eq!(shown.heartbeat.current_focus.as_deref(), Some("live focus"));

    let text = store.prepend_heartbeat_for_orchestrator_pane("orch-a", "continue");
    assert!(text.contains(&format!("plan=\"{}\"", live.id)));
    assert!(text.contains("Current focus: live focus"));
    assert!(!text.contains("old focus"));

    store
        .close_task(&live.id, &task_id, Some("done"), Default::default(), 24)
        .unwrap();
    let shown_after_close = store.show_heartbeat(None, Some("orch-a"), None).unwrap();
    assert_eq!(shown_after_close.plan_id, live.id);
}

#[test]
fn heartbeat_resolves_for_owner_pane_at_send_time() {
    let (_dir, store, plan_id) = store_with_plan("orch-a");
    let before = store.prepend_heartbeat_for_orchestrator_pane("orch-a", "continue");
    assert!(before.contains("STATE-RECONCILIATION"));
    assert!(before.contains("First run `codeterm agent reports --pane orch-a`"));
    assert!(before.contains("codeterm orchestrator status --plan"));
    assert!(before.contains("--pane orch-a --json"));
    assert!(before.contains("codeterm plan reconcile"));
    assert!(before.contains("codeterm plan task close"));
    assert!(before.contains("codeterm send \"<next concrete instruction>\" --pane"));
    assert!(before.contains("Keep the plan-owned heartbeat current"));
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
    let after = store.prepend_heartbeat_for_orchestrator_pane("orch-a", "continue");
    assert!(after.contains("<codeterm-state-reconciliation"));
    assert!(after.contains("<codeterm-orchestrator-heartbeat"));
    assert!(after.contains("Current focus: Review worker branches"));
    assert!(after.contains("Active lanes:\n- backend tests\n- frontend canvas"));
    assert!(after.contains("Rules:\n- Workers report in_review only"));
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
    let resolved_later = store.prepend_heartbeat_for_orchestrator_pane("orch-a", "continue");
    assert!(resolved_later.contains("Current focus: Integrate verified branches"));
    assert!(!resolved_later.contains("Current focus: Review worker branches"));

    let worker_text = store.prepend_heartbeat_for_orchestrator_pane("worker-a", "continue");
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
    assert!(text.contains("STATE-RECONCILIATION"));
    assert!(text.contains("execution-mode plan with unfinished work"));
    assert!(text.contains("codeterm docs ORCHESTRATION"));
    assert!(text.contains("Reconcile live panes against the plan"));
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

    assert!(text.starts_with("<codeterm-state-reconciliation"));
    assert!(text.contains("codeterm agent reports --pane orch-a"));
    assert!(text.contains(&format!(
        "codeterm orchestrator status --plan {plan_id} --pane orch-a --json"
    )));
    assert!(text.contains(&format!("codeterm plan task close {plan_id} <task_id>")));
    assert!(text.contains("codeterm send \"<next concrete instruction>\" --pane <worker_pane_id>"));
    assert!(text.contains(&format!(
        "codeterm orchestrator heartbeat set --plan {plan_id}"
    )));
    assert!(text.contains("codeterm docs ORCHESTRATION"));
    assert!(text.contains("<codeterm-orchestrator-heartbeat"));
    assert!(text.contains("Current focus: Review open lanes"));
    assert!(text.contains("Plan reminder: this pane owns unfinished execution-plan work."));
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
        .close_task(&plan_id, &task_id, Some("done"), AgentRef::from_pane("orch-a"), 30)
        .unwrap();

    let nudge_body = "Plan reminder: this pane owns unfinished execution-plan work.";
    let result = store.prepend_heartbeat_for_plan(&plan_id, nudge_body);
    assert_eq!(result, nudge_body, "complete plan must not inject reconciliation");
    assert!(!result.contains("STATE-RECONCILIATION"));
    assert!(!result.contains("codeterm-state-reconciliation"));

    let pane_result = store.prepend_heartbeat_for_orchestrator_pane("orch-a", nudge_body);
    assert_eq!(pane_result, nudge_body, "complete plan must be silent via orchestrator pane path");
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
    assert!(result.contains("STATE-RECONCILIATION"), "open work must not be silenced");
}

#[test]
fn is_plan_complete_predicate_covers_execution_all_done_and_explicit_complete() {
    use super::heartbeat::is_plan_complete;

    // Empty plan in Execution — not "complete" (no tasks done yet, just empty)
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan = store
        .create_plan("T".into(), dir.path().display().to_string(), OrchestratorRef::from_pane("orch"), vec![], 1)
        .unwrap();
    store.set_mode(&plan.id, PlanMode::Execution, 2).unwrap();
    let empty_plan = store.get(&plan.id).unwrap();
    assert!(!is_plan_complete(&empty_plan), "empty Execution plan is not complete");

    // Add a task and close it — now complete
    let task_id = store
        .add_task(&plan.id, NewTask { name: "t".into(), kind: "impl".into(), ..Default::default() }, 3)
        .unwrap();
    store.close_task(&plan.id, &task_id, None, AgentRef::from_pane("orch"), 4).unwrap();
    let done_plan = store.get(&plan.id).unwrap();
    assert!(is_plan_complete(&done_plan), "all-done Execution plan is complete");

    // Explicit Complete mode
    store.set_mode(&plan.id, PlanMode::Complete, 5).unwrap();
    let complete_plan = store.get(&plan.id).unwrap();
    assert!(is_plan_complete(&complete_plan), "Complete mode is always complete");
}
