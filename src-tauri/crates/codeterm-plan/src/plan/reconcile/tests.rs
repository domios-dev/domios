use super::*;
use crate::plan::{
    OrchestratorRef, Plan, Task, TaskExecutionAttempt, TaskExecutionReportSnapshot, TaskGroup,
    TaskPriority, TaskStatus,
};

fn task(id: &str, status: TaskStatus) -> Task {
    Task {
        files: Default::default(),
        id: id.into(),
        name: id.into(),
        kind: "implement".into(),
        model: None,
        speed: None,
        model_rationale: None,
        acceptance_criteria: vec![],
        external_refs: vec![],
        test_plan: None,
        qa_plan: None,
        correctness: None,
        status,
        review_stage: Default::default(),
        qa_state: None,
        review_round: 0,
        review_oracle: None,
        review_evidence: vec![],
        key: None,
        review_override: None,
        review_audit: vec![],
        status_label: None,
        priority: TaskPriority::P2,
        created_at: 0,
        updated_at: 0,
        status_changed_at: 0,
        handle: None,
        tags: vec![],
        section_ids: vec![],
        body: String::new(),
        role_guidance: None,
        executor_role: None,
        competition: None,
        intent: None,
        execution_intent: Default::default(),
        parent_task_id: None,
        depends_on: vec![],
        assigned_tab_id: None,
        lane: Default::default(),
        taken_by_role: None,
        peer: None,
        group: None,
        canvas: Default::default(),
        milestones: vec![],
        execution_attempts: vec![],
        interruption: Default::default(),
        pause_reason: None,
        personality: None,
        personality_provenance: None,
        attachments: vec![],
    }
}

fn plan(tasks: Vec<Task>) -> Plan {
    let mut plan = Plan::new(
        "p".into(),
        "/repo".into(),
        OrchestratorRef::from_tab("orch"),
    );
    plan.id = "plan".into();
    plan.tasks = tasks;
    plan
}

fn cfg(now_ms: u64) -> ReconcileConfig {
    ReconcileConfig {
        now_ms,
        stale_review_ms: 1_000,
        current_orchestrator_tab_id: Some("orch".into()),
    }
}

#[test]
fn detects_in_progress_without_live_tab() {
    let mut t = task("t1", TaskStatus::InProgress);
    t.assigned_tab_id = Some("dead".into());
    let items = detect_plan_drift(&plan(vec![t]), &cfg(2_000), |_| TabProbe {
        exists: false,
        ..Default::default()
    });
    assert!(items
        .iter()
        .any(|item| item.kind == DriftKind::InProgressTabGone));
}

#[test]
fn review_hold_cleared_by_state_change_or_later_verdict() {
    let mut held = task("held", TaskStatus::InReview);
    held.status_changed_at = 10;
    held.review_evidence.push(crate::plan::ReviewEvidence {
        id: "hold".into(),
        from: crate::plan::AgentRef::default(),
        verdict: crate::plan::ReviewVerdict::Hold,
        note: "fixture-note".into(),
        at: 20,
        changed_executable_surfaces: false,
        distinct_oracle: false,
    });
    assert_eq!(active_review_hold_note(&held), Some("fixture-note"));
    held.status = TaskStatus::InProgress;
    assert_eq!(active_review_hold_note(&held), None);
    held.status = TaskStatus::InReview;
    held.status_changed_at = 30;
    assert_eq!(active_review_hold_note(&held), None);
    held.status_changed_at = 10;
    let mut later = held.review_evidence[0].clone();
    later.at = 25;
    later.verdict = crate::plan::ReviewVerdict::Accept;
    held.review_evidence.push(later);
    assert_eq!(active_review_hold_note(&held), None);
    held.review_evidence.last_mut().unwrap().verdict = crate::plan::ReviewVerdict::Bounce;
    assert_eq!(active_review_hold_note(&held), None);
}

#[test]
fn detects_stale_in_review() {
    let mut t = task("t1", TaskStatus::InReview);
    t.status_changed_at = 1;
    let items = detect_plan_drift(&plan(vec![t]), &cfg(2_500), |_| TabProbe::default());
    assert_eq!(items[0].kind, DriftKind::StaleInReview);
    assert_eq!(items[0].suggested_fix, STALE_REVIEW_LABEL);
}

#[test]
fn report_delivery_is_drift_until_written() {
    let now_ms = REPORT_TARGET_WRITE_GRACE_MS + 101;
    let mut review = task("t1", TaskStatus::InReview);
    review.status_changed_at = now_ms;
    review.execution_attempts.push(TaskExecutionAttempt {
        id: "attempt-1".into(),
        executor_tab_id: "worker".into(),
        parent_tab_id: Some("target-tab".into()),
        reported_at: Some(100),
        report: Some(TaskExecutionReportSnapshot {
            status: Some("in_review".into()),
            ..Default::default()
        }),
        ..Default::default()
    });
    let mut plan = plan(vec![review]);

    let before_grace = detect_plan_drift(&plan, &cfg(100 + REPORT_TARGET_WRITE_GRACE_MS), |_| {
        TabProbe::default()
    });
    assert!(before_grace
        .iter()
        .all(|item| item.kind != DriftKind::ReportTargetWriteMissing));

    let drift = detect_plan_drift(&plan, &cfg(now_ms), |_| TabProbe::default());
    let item = drift
        .iter()
        .find(|item| item.kind == DriftKind::ReportTargetWriteMissing)
        .unwrap();
    assert_eq!(item.task_id, "t1");
    assert!(item.detail.contains("attempt-1"));
    assert!(item.detail.contains("target-tab"));

    plan.tasks[0].execution_attempts[0].report_target_written_at = Some(now_ms);
    assert!(
        detect_plan_drift(&plan, &cfg(now_ms), |_| TabProbe::default())
            .iter()
            .all(|item| item.kind != DriftKind::ReportTargetWriteMissing)
    );
}

#[test]
fn detects_stale_pending_unstaffed() {
    let mut t = task("t1", TaskStatus::Pending);
    t.status_changed_at = 1;
    let now = PENDING_UNSTAFFED_STALE_MS + 10;
    let items = detect_plan_drift(&plan(vec![t]), &cfg(now), |_| TabProbe::default());
    assert!(items
        .iter()
        .any(|item| item.kind == DriftKind::StalePendingUnstaffed));
}

#[test]
fn later_group_pending_task_is_not_stale_drift() {
    let mut active = task("active", TaskStatus::InProgress);
    active.group = Some("release".into());
    active.assigned_tab_id = Some("worker".into());
    let mut later = task("later", TaskStatus::Pending);
    later.group = Some("after-release".into());
    later.status_changed_at = 1;
    let mut plan = plan(vec![active, later]);
    plan.task_groups = vec![
        TaskGroup {
            key: "release".into(),
            label: "Release".into(),
            color: None,
        },
        TaskGroup {
            key: "after-release".into(),
            label: "After release".into(),
            color: None,
        },
    ];
    let now = PENDING_UNSTAFFED_STALE_MS + 10;
    let probe: fn(&str) -> TabProbe = |_| TabProbe {
        exists: true,
        live_agent: true,
        ..Default::default()
    };
    let items = detect_plan_drift(&plan, &cfg(now), probe);

    assert!(items
        .iter()
        .all(|item| { item.task_id != "later" || item.kind != DriftKind::StalePendingUnstaffed }));
    assert!(aged_open_tasks(&plan, now)
        .iter()
        .all(|item| item.task_id != "later"));
}

#[test]
fn dependency_waiting_pending_is_not_stale_unstaffed_or_aged() {
    let dependency = task("dependency", TaskStatus::InProgress);
    let mut waiting = task("waiting", TaskStatus::Pending);
    waiting.depends_on = vec![dependency.id.clone()];
    waiting.status_changed_at = 1;
    let plan = plan(vec![dependency, waiting]);
    let now = PENDING_UNSTAFFED_STALE_MS + 10;

    assert!(aged_open_tasks(&plan, now).is_empty());
    assert!(detect_plan_drift(&plan, &cfg(now), |_| TabProbe::default())
        .iter()
        .all(|item| item.kind != DriftKind::StalePendingUnstaffed));
}

#[test]
fn missing_dependency_remains_waiting_and_is_not_stale_actionable() {
    let mut waiting = task("waiting", TaskStatus::Pending);
    waiting.depends_on = vec!["missing".into()];
    waiting.status_changed_at = 1;
    let plan = plan(vec![waiting]);
    let now = PENDING_UNSTAFFED_STALE_MS + 10;

    assert!(aged_open_tasks(&plan, now).is_empty());
    assert!(detect_plan_drift(&plan, &cfg(now), |_| TabProbe::default())
        .iter()
        .all(|item| item.kind != DriftKind::StalePendingUnstaffed));
}

#[test]
fn aged_open_tasks_names_stale_work_and_skips_paused() {
    let mut pending = task("p1", TaskStatus::Pending);
    pending.status_changed_at = 1;
    let mut paused = task("k1", TaskStatus::Paused);
    paused.status_changed_at = 1;
    let mut fresh = task("f1", TaskStatus::InReview);
    fresh.status_changed_at = PENDING_UNSTAFFED_STALE_MS;
    let now = PENDING_UNSTAFFED_STALE_MS + 10;
    let aged = aged_open_tasks(&plan(vec![pending, paused, fresh]), now);
    assert_eq!(aged.len(), 1);
    assert_eq!(aged[0].task_id, "p1");
}

#[test]
fn detects_bound_to_closed_tab() {
    let mut t = task("t1", TaskStatus::Pending);
    t.assigned_tab_id = Some("closed".into());
    let items = detect_plan_drift(&plan(vec![t]), &cfg(2_000), |_| TabProbe {
        exists: false,
        ..Default::default()
    });
    assert_eq!(items[0].kind, DriftKind::BoundToClosedTab);
}

#[test]
fn ignores_empty_and_terminal_closed_tab_bindings() {
    let mut empty = task("empty", TaskStatus::Pending);
    empty.assigned_tab_id = Some(" ".into());
    let mut done = task("done", TaskStatus::Done);
    done.assigned_tab_id = Some("closed".into());
    let mut cancelled = task("cancelled", TaskStatus::Cancelled);
    cancelled.assigned_tab_id = Some("closed".into());
    let items = detect_plan_drift(&plan(vec![empty, done, cancelled]), &cfg(2_000), |_| {
        TabProbe {
            exists: false,
            ..Default::default()
        }
    });
    assert!(items.is_empty());
    assert!(detect_tab_gone(&plan(vec![]), "").is_empty());
}

#[test]
fn accepted_but_not_done_is_inert_without_acceptance_signal() {
    let mut t = task("t1", TaskStatus::InReview);
    t.milestones.push(crate::plan::Milestone {
        id: "m1".into(),
        from: Default::default(),
        kind: "review".into(),
        text: "looks good".into(),
        ts: 1,
        links: vec![],
        loop_id: None,
        iteration: None,
        canvas: Default::default(),
        attachments: vec![],
    });
    let items = detect_plan_drift(&plan(vec![t]), &cfg(2), |_| TabProbe::default());
    assert!(!items
        .iter()
        .any(|item| item.kind == DriftKind::AcceptedButNotDone));
}

#[test]
fn flags_not_mine_for_other_orchestrator_worker() {
    let mut t = task("t1", TaskStatus::InProgress);
    t.assigned_tab_id = Some("worker".into());
    let items = detect_plan_drift(&plan(vec![t]), &cfg(2_000), |_| TabProbe {
        exists: true,
        live_agent: false,
        role: Some(TabRole::Worker),
        parent_tab_id: Some("other-orch".into()),
        ..Default::default()
    });
    assert!(items[0].not_mine);
}

fn lane_thresholds() -> LaneThresholds {
    LaneThresholds {
        t_stall_ms: 100,
        t_overdue_ms: 1_000,
    }
}

fn in_progress_lane(id: &str, tab: &str, status_changed_at: u64) -> Task {
    let mut t = task(id, TaskStatus::InProgress);
    t.assigned_tab_id = Some(tab.into());
    t.status_changed_at = status_changed_at;
    t
}

#[test]
fn demotes_dead_lane_past_grace() {
    let t = in_progress_lane("t1", "dead", 1);
    let demotions = detect_lane_demotions(
        &(&plan(vec![t])).into(),
        &cfg(5_000),
        &lane_thresholds(),
        |_| TabProbe {
            exists: false,
            ..Default::default()
        },
    );
    assert_eq!(demotions.len(), 1);
    assert_eq!(demotions[0].task_id, "t1");
}

#[test]
fn leaves_live_lane_untouched() {
    let t = in_progress_lane("t1", "live", 4_900);
    let demotions = detect_lane_demotions(
        &(&plan(vec![t])).into(),
        &cfg(5_000),
        &lane_thresholds(),
        |_| TabProbe {
            exists: true,
            live_agent: true,
            ..Default::default()
        },
    );
    assert!(demotions.is_empty());
}

#[test]
fn never_demotes_report_overdue_lane() {
    // Tab present but silent past overdue: honest slow work, not gone.
    let t = in_progress_lane("t1", "live", 1);
    let demotions = detect_lane_demotions(
        &(&plan(vec![t])).into(),
        &cfg(5_000),
        &lane_thresholds(),
        |_| TabProbe {
            exists: true,
            live_agent: true,
            ..Default::default()
        },
    );
    assert!(demotions.is_empty());
}

#[test]
fn grace_window_spares_recently_active_dead_lane() {
    let t = in_progress_lane("t1", "dead", 4_900);
    let demotions = detect_lane_demotions(
        &(&plan(vec![t])).into(),
        &cfg(5_000),
        &lane_thresholds(),
        |_| TabProbe {
            exists: false,
            ..Default::default()
        },
    );
    assert!(demotions.is_empty());
}

#[test]
fn never_demotes_other_orchestrator_lane() {
    let t = in_progress_lane("t1", "worker", 1);
    let demotions = detect_lane_demotions(
        &(&plan(vec![t])).into(),
        &cfg(5_000),
        &lane_thresholds(),
        |_| TabProbe {
            exists: false,
            live_agent: false,
            role: Some(TabRole::Worker),
            parent_tab_id: Some("other-orch".into()),
            ..Default::default()
        },
    );
    assert!(demotions.is_empty());
}

#[test]
fn tab_gone_detector_is_bound_to_requested_tab() {
    let mut t1 = task("t1", TaskStatus::InProgress);
    t1.assigned_tab_id = Some("gone".into());
    let mut t2 = task("t2", TaskStatus::InProgress);
    t2.assigned_tab_id = Some("live".into());
    let items = detect_tab_gone(&plan(vec![t1, t2]), "gone");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].task_id, "t1");
}
