use std::cell::Cell;

use super::*;
use crate::plan::{ElementCanvas, OrchestratorRef, Task, TaskLaneMetadata, TaskPriority};

fn plan(tasks: Vec<Task>) -> Plan {
    let mut plan = Plan::new(
        "Stalled team".into(),
        "/tmp/project".into(),
        OrchestratorRef::from_tab("orchestrator"),
    );
    plan.id = "plan-id".into();
    plan.mode = PlanMode::Execution;
    plan.tasks = tasks;
    plan
}

fn task(id: &str, status: TaskStatus, worker: Option<&str>) -> Task {
    Task {
        id: id.into(),
        name: id.into(),
        kind: "implementation".into(),
        model: None,
        model_rationale: None,
        acceptance_criteria: Vec::new(),
        external_refs: Vec::new(),
        test_plan: None,
        qa_plan: None,
        correctness: None,
        status,
        review_stage: Default::default(),
        qa_state: None,
        review_round: 0,
        review_oracle: None,
        review_evidence: Vec::new(),
        key: None,
        review_override: None,
        review_audit: Vec::new(),
        status_label: None,
        priority: TaskPriority::P2,
        created_at: 0,
        updated_at: 0,
        status_changed_at: 0,
        handle: None,
        tags: Vec::new(),
        section_ids: Vec::new(),
        body: String::new(),
        role_guidance: None,
        executor_role: None,
        competition: None,
        intent: None,
        execution_intent: Default::default(),
        parent_task_id: None,
        depends_on: Vec::new(),
        assigned_tab_id: worker.map(str::to_string),
        lane: TaskLaneMetadata::default(),
        taken_by_role: None,
        peer: None,
        group: None,
        canvas: ElementCanvas::default(),
        milestones: Vec::new(),
        execution_attempts: Vec::new(),
        pause_reason: None,
        interruption: Default::default(),
        personality: None,
        personality_provenance: None,
        attachments: Vec::new(),
    }
}

fn nudge(
    plan: &Plan,
    live_worker_tabs: &HashSet<String>,
    orchestrator_idle: bool,
    tracker: &StallEpisodeTracker,
    deliveries: &mut Vec<StallKey>,
) {
    let _ = maybe_nudge_stalled_team(plan, live_worker_tabs, orchestrator_idle, tracker, |key| {
        deliveries.push(key.clone());
        Ok(())
    });
}

#[test]
fn stalled_team_predicate_requires_actionable_unpaused_execution_plan() {
    let actionable = plan(vec![
        task("z-task", TaskStatus::Pending, None),
        task("a-task", TaskStatus::Blocked, None),
    ]);
    let empty_live_set = HashSet::new();
    assert_eq!(
        stall_episode(&actionable, &empty_live_set),
        Some(StallKey {
            plan_id: "plan-id".into(),
            actionable_task_ids: vec!["a-task".into(), "z-task".into()],
        })
    );

    let mut planning = actionable.clone();
    planning.mode = PlanMode::Planning;
    assert_eq!(stall_episode(&planning, &empty_live_set), None);

    let mut paused = actionable.clone();
    paused.mode = PlanMode::Paused;
    assert_eq!(stall_episode(&paused, &empty_live_set), None);

    let archived = Plan {
        status: PlanStatus::Archived,
        ..actionable.clone()
    };
    assert_eq!(stall_episode(&archived, &empty_live_set), None);
    assert_eq!(stall_episode(&plan(Vec::new()), &empty_live_set), None);
}

#[test]
fn stalled_team_predicate_none_with_live_worker() {
    let plan = plan(vec![task("task", TaskStatus::InProgress, Some("worker"))]);
    let unrelated_live_tab = HashSet::from(["unrelated".to_string()]);
    assert!(stall_episode(&plan, &unrelated_live_tab).is_some());

    let live_worker = HashSet::from(["worker".to_string()]);
    assert_eq!(stall_episode(&plan, &live_worker), None);
}

#[test]
fn stalled_team_nudges_once_per_key() {
    let plan = plan(vec![task("task", TaskStatus::Pending, None)]);
    let tracker = StallEpisodeTracker::default();
    let mut deliveries = Vec::new();
    let no_live_workers = HashSet::new();

    nudge(&plan, &no_live_workers, true, &tracker, &mut deliveries);
    nudge(&plan, &no_live_workers, true, &tracker, &mut deliveries);

    assert_eq!(deliveries.len(), 1);
}

#[test]
fn stalled_team_rearms_on_live_worker_or_changed_actionable_set() {
    let first = plan(vec![task("first", TaskStatus::Pending, Some("worker"))]);
    let tracker = StallEpisodeTracker::default();
    let mut deliveries = Vec::new();
    let no_live_workers = HashSet::new();
    nudge(&first, &no_live_workers, true, &tracker, &mut deliveries);

    let live_worker = HashSet::from(["worker".to_string()]);
    nudge(&first, &live_worker, true, &tracker, &mut deliveries);
    nudge(&first, &no_live_workers, true, &tracker, &mut deliveries);

    let changed = plan(vec![
        task("first", TaskStatus::Pending, Some("worker")),
        task("second", TaskStatus::Pending, None),
    ]);
    nudge(&changed, &no_live_workers, true, &tracker, &mut deliveries);
    assert_eq!(deliveries.len(), 3);
    assert_ne!(deliveries[1], deliveries[2]);
}

#[test]
fn stalled_team_gone_tab_nudges_only_idle_orchestrator() {
    let plan = plan(vec![task("task", TaskStatus::Pending, Some("gone-worker"))]);
    let tracker = StallEpisodeTracker::default();
    let mut deliveries = Vec::new();
    let no_live_workers = HashSet::new();

    nudge(&plan, &no_live_workers, false, &tracker, &mut deliveries);
    assert!(deliveries.is_empty());

    nudge(&plan, &no_live_workers, true, &tracker, &mut deliveries);
    nudge(&plan, &no_live_workers, true, &tracker, &mut deliveries);
    assert_eq!(deliveries.len(), 1);
}

#[test]
fn stalled_team_non_orchestrator_idle_skips_plan_store() {
    let plan_store_read = Cell::new(false);

    let plans = idle_orchestrator_candidates(false, || {
        plan_store_read.set(true);
        Vec::new()
    });

    assert!(plans.is_empty());
    assert!(!plan_store_read.get());
}
