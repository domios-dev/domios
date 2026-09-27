use super::*;
use crate::plan::coverage::compute_coverage_report;
use crate::plan::reconcile::DriftKind;
use crate::plan::types::{LoopSpec, Milestone, Section, Task};

fn section(id: &str) -> Section {
    Section {
        id: id.into(),
        title: id.into(),
        handle: None,
        tags: vec![],
        body: String::new(),
        subsections: vec![],
        canvas: Default::default(),
        attachments: vec![],
    }
}

fn milestone(id: &str, ts: u64) -> Milestone {
    Milestone {
        id: id.into(),
        from: Default::default(),
        kind: "decision".into(),
        text: format!("stride {id}"),
        ts,
        links: vec![],
        loop_id: None,
        iteration: None,
        canvas: Default::default(),
        attachments: vec![],
    }
}

fn task(id: &str, status: TaskStatus, sections: &[&str]) -> Task {
    Task {
        id: id.into(),
        name: format!("name-{id}"),
        kind: "implement".into(),
        model: None,
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
        priority: Default::default(),
        created_at: 0,
        updated_at: 0,
        status_changed_at: 0,
        handle: None,
        tags: vec![],
        section_ids: sections.iter().map(|s| s.to_string()).collect(),
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

fn fixture() -> Plan {
    let mut p = Plan::new("Loop State".into(), "/repo".into(), Default::default());
    p.sections = vec![section("s1"), section("s2")];
    p.goal = Some(Goal {
        vector: "ship the loop".into(),
        ..Default::default()
    });
    p.heartbeat.current_focus = Some("foundation".into());
    p.heartbeat.active_lanes = vec!["loop-state".into()];
    p.loops = vec![LoopSpec {
        id: "lp_1".into(),
        target: "harden".into(),
        budget: 5,
        count: 2,
        stop_when: Some("0 bugs".into()),
        status: LoopStatus::Running,
        stagnation: 0,
        deviation: None,
        last_iteration_at: Some(300),
    }];

    let mut done = task("t_done", TaskStatus::Done, &["s1"]);
    done.milestones = vec![milestone("m1", 100), milestone("m2", 300)];
    let mut in_review = task("t_review", TaskStatus::InReview, &["s1"]);
    in_review.assigned_tab_id = Some("tab_live".into());
    in_review.milestones = vec![milestone("m3", 200)];
    let mut in_prog = task("t_prog", TaskStatus::InProgress, &["s2"]);
    in_prog.assigned_tab_id = Some("tab_dead".into());
    let pending = task("t_pending", TaskStatus::Pending, &[]);
    let cancelled = task("t_cancel", TaskStatus::Cancelled, &["s2"]);

    p.tasks = vec![done, in_review, in_prog, pending, cancelled];
    p
}

#[test]
fn queued_qa_task_presents_distinctly_from_bare_in_review() {
    assert_eq!(
        task_presentation(&TaskStatus::InReview, &[], &ReviewStage::Review, &None),
        TaskPresentation::InReview
    );
    assert_eq!(
        task_presentation(
            &TaskStatus::InReview,
            &[],
            &ReviewStage::Qa,
            &Some(QaState::Queued)
        ),
        TaskPresentation::QueuedForQa
    );
    assert_eq!(
        task_presentation(
            &TaskStatus::InReview,
            &[],
            &ReviewStage::Qa,
            &Some(QaState::Running)
        ),
        TaskPresentation::InReview
    );
}

#[test]
fn open_lane_carries_qa_state_and_status_label_for_queued_task() {
    let mut plan = fixture();
    let mut queued = task("t_queued_qa", TaskStatus::InReview, &["s1"]);
    queued.review_stage = ReviewStage::Qa;
    queued.qa_state = Some(QaState::Queued);
    queued.status_label = Some("blocked: reviewer host unavailable overnight".into());
    plan.tasks.push(queued);
    let coverage = compute_coverage_report(&plan);
    let snap = build_plan_state_snapshot(
        &plan,
        coverage,
        vec![],
        vec![],
        vec![],
        &HashSet::new(),
        None,
        1,
    );
    let lane = snap
        .open_lanes
        .iter()
        .find(|l| l.task_id == "t_queued_qa")
        .expect("queued lane");
    assert_eq!(lane.presentation, TaskPresentation::QueuedForQa);
    assert_eq!(
        lane.status_label.as_deref(),
        Some("blocked: reviewer host unavailable overnight")
    );
}

#[test]
fn snapshot_counts_coverage_lanes_agents_and_strides() {
    let plan = fixture();
    let coverage = compute_coverage_report(&plan);
    let drift = vec![DriftItem {
        task_id: "t_prog".into(),
        plan_id: plan.id.clone(),
        kind: DriftKind::InProgressTabGone,
        detail: "gone".into(),
        suggested_fix: "needs-reassign".into(),
        not_mine: false,
    }];
    let live: HashSet<String> = ["tab_live".to_string()].into_iter().collect();

    let snap =
        build_plan_state_snapshot(&plan, coverage, drift, vec![], vec![], &live, None, 1);

    assert_eq!(snap.task_counts.total, 5);
    assert_eq!(snap.task_counts.done, 1);
    assert_eq!(snap.task_counts.in_review, 1);
    assert_eq!(snap.task_counts.in_progress, 1);
    assert_eq!(snap.task_counts.pending, 1);
    assert_eq!(snap.task_counts.cancelled, 1);
    assert!((snap.percent_done - 0.2).abs() < 1e-9);

    // coverage: s1 covered, s2 covered -> fraction 1.0 (both have tasks)
    assert!((snap.coverage.plan_covered_fraction - 1.0).abs() < 1e-9);

    // open lanes: pending + in_review + in_progress (not done / cancelled / blocked)
    let lane_ids: Vec<&str> = snap.open_lanes.iter().map(|l| l.task_id.as_str()).collect();
    assert_eq!(lane_ids.len(), 3);
    assert!(lane_ids.contains(&"t_review"));
    assert!(lane_ids.contains(&"t_pending"));
    assert!(lane_ids.contains(&"t_prog"));

    assert_eq!(snap.live_agents.len(), 1);
    assert_eq!(snap.live_agents[0].task_id, "t_review");
    assert_eq!(snap.live_agents[0].tab_id, "tab_live");

    assert_eq!(snap.drift.total, 1);
    assert_eq!(snap.drift.by_kind.get("in_progress_tab_gone"), Some(&1));

    assert_eq!(snap.loops.len(), 1);
    assert_eq!(snap.loops[0].budget, 5);
    assert_eq!(snap.loops[0].count, 2);

    assert_eq!(snap.heartbeat.current_focus.as_deref(), Some("foundation"));
    assert_eq!(snap.heartbeat.active_lanes, vec!["loop-state".to_string()]);

    let ids: Vec<&str> = snap.recent_strides.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["m2", "m3", "m1"]);
}

#[test]
fn recent_strides_capped_at_ten_newest() {
    let mut plan = fixture();
    for task in &mut plan.tasks {
        task.milestones.clear();
    }
    plan.tasks[0].milestones = (0..15)
        .map(|i| milestone(&format!("k{i}"), i as u64))
        .collect();
    let coverage = compute_coverage_report(&plan);
    let snap = build_plan_state_snapshot(
        &plan,
        coverage,
        vec![],
        vec![],
        vec![],
        &HashSet::new(),
        None,
        1,
    );
    assert_eq!(snap.recent_strides.len(), RECENT_STRIDE_LIMIT);
    assert_eq!(snap.recent_strides[0].id, "k14");
    assert_eq!(snap.recent_strides[9].id, "k5");
}

#[test]
fn plan_level_strides_surface_marked_and_task_less() {
    let mut plan = fixture();
    for task in &mut plan.tasks {
        task.milestones.clear();
    }
    plan.tasks[0].milestones = vec![milestone("task_ms", 100)];
    plan.strides = vec![milestone("plan_ms", 200)];
    let coverage = compute_coverage_report(&plan);
    let snap = build_plan_state_snapshot(
        &plan,
        coverage,
        vec![],
        vec![],
        vec![],
        &HashSet::new(),
        None,
        1,
    );
    let plan_level: Vec<_> = snap
        .recent_strides
        .iter()
        .filter(|s| s.plan_level)
        .collect();
    assert_eq!(plan_level.len(), 1);
    assert_eq!(plan_level[0].id, "plan_ms");
    assert!(plan_level[0].task_id.is_empty());
    // Newest first: plan-level (ts 200) precedes the task milestone (ts 100).
    assert_eq!(snap.recent_strides[0].id, "plan_ms");
    assert!(snap
        .recent_strides
        .iter()
        .any(|s| s.id == "task_ms" && !s.plan_level));
}

#[test]
fn snapshot_is_read_only() {
    let plan = fixture();
    let before = plan.clone();
    let coverage = compute_coverage_report(&plan);
    let _ = build_plan_state_snapshot(
        &plan,
        coverage,
        vec![],
        vec![],
        vec![],
        &HashSet::new(),
        None,
        1,
    );
    assert_eq!(plan, before);
}

#[test]
fn explicit_dependency_waits_without_becoming_paused() {
    let mut plan = fixture();
    let dependency = task("t_dependency", TaskStatus::InProgress, &["s1"]);
    let mut child = task("t_child", TaskStatus::Pending, &["s1"]);
    child.depends_on = vec!["t_dependency".into()];
    plan.tasks = vec![dependency, child];

    let coverage = compute_coverage_report(&plan);
    let snap = build_plan_state_snapshot(
        &plan,
        coverage,
        vec![],
        vec![],
        vec![],
        &HashSet::new(),
        None,
        1,
    );

    assert_eq!(snap.task_counts.pending, 1);
    assert_eq!(snap.task_counts.paused, 0);
    let child_lane = snap
        .open_lanes
        .iter()
        .find(|lane| lane.task_id == "t_child")
        .expect("child lane");
    assert_eq!(child_lane.status, TaskStatus::Pending);
    assert_eq!(
        child_lane.presentation,
        TaskPresentation::WaitingOnDependencies
    );
    assert!(child_lane.waiting_on_dependencies);
    assert_eq!(child_lane.dependency_waits.len(), 1);
    assert_eq!(child_lane.dependency_waits[0].task_id, "t_dependency");
    assert_eq!(
        child_lane.dependency_waits[0].status,
        Some(TaskStatus::InProgress)
    );
    assert!(!child_lane.dependency_waits[0].missing);
}

#[test]
fn hierarchy_without_explicit_dependency_does_not_wait() {
    let mut plan = fixture();
    let parent = task("t_parent", TaskStatus::InProgress, &["s1"]);
    let mut child = task("t_child", TaskStatus::Pending, &["s1"]);
    child.parent_task_id = Some("t_parent".into());
    plan.tasks = vec![parent, child];
    let coverage = compute_coverage_report(&plan);
    let snap = build_plan_state_snapshot(
        &plan,
        coverage,
        vec![],
        vec![],
        vec![],
        &HashSet::new(),
        None,
        1,
    );
    let child_lane = snap
        .open_lanes
        .iter()
        .find(|lane| lane.task_id == "t_child")
        .expect("child lane");
    assert!(!child_lane.waiting_on_dependencies);
    assert_eq!(child_lane.presentation, TaskPresentation::ActionablePending);
    assert!(child_lane.dependency_waits.is_empty());
}

#[test]
fn parent_rolls_up_open_child_statuses_in_plan_state() {
    let mut plan = fixture();
    let mut parent = task("t_parent", TaskStatus::InProgress, &[]);
    parent.assigned_tab_id = Some("tab_parent".into());
    let mut child = task("t_child", TaskStatus::Pending, &[]);
    child.parent_task_id = Some(parent.id.clone());
    plan.tasks = vec![parent, child];
    let snapshot = build_plan_state_snapshot(&plan, compute_coverage_report(&plan), vec![], vec![], vec![], &HashSet::new(), None, 1);
    let parent_lane = snapshot.open_lanes.iter().find(|lane| lane.task_id == "t_parent").unwrap();
    assert_eq!(parent_lane.presentation, TaskPresentation::BlockedOnSubtasks);
    assert_eq!(parent_lane.subtasks.len(), 1);
    assert_eq!(parent_lane.subtasks[0].status, TaskStatus::Pending);
    assert!(task_projection(&plan, &plan.tasks[0]).needs_orchestrator_action());
}

#[test]
fn child_rollup_preserves_handoff_review_qa_and_terminal_presentations() {
    let mut plan = fixture();
    let mut parent = task("t_parent", TaskStatus::InProgress, &[]);
    let mut child = task("t_child", TaskStatus::Pending, &[]);
    child.parent_task_id = Some(parent.id.clone());
    parent.interruption.handoff_required = true;
    plan.tasks = vec![parent.clone(), child];
    assert_eq!(task_projection(&plan, &parent).presentation, TaskPresentation::HandoffRequired);
    parent.interruption.handoff_required = false;
    parent.status = TaskStatus::InReview;
    parent.review_stage = ReviewStage::Qa;
    parent.qa_state = Some(QaState::Queued);
    plan.tasks[0] = parent.clone();
    assert_eq!(task_projection(&plan, &parent).presentation, TaskPresentation::QueuedForQa);
    parent.status = TaskStatus::Cancelled;
    plan.tasks[0] = parent.clone();
    assert_eq!(task_projection(&plan, &parent).presentation, TaskPresentation::Terminal);
}

#[test]
fn terminal_dependencies_do_not_block_waiting() {
    let mut plan = fixture();
    let mut child = task("t_child", TaskStatus::Pending, &["s1"]);
    child.depends_on = vec!["t_done".into(), "t_cancel".into()];
    let done = task("t_done", TaskStatus::Done, &["s1"]);
    let cancelled = task("t_cancel", TaskStatus::Cancelled, &["s1"]);
    plan.tasks = vec![done, cancelled, child];
    let coverage = compute_coverage_report(&plan);
    let snap = build_plan_state_snapshot(
        &plan,
        coverage,
        vec![],
        vec![],
        vec![],
        &HashSet::new(),
        None,
        1,
    );
    let child_lane = snap
        .open_lanes
        .iter()
        .find(|lane| lane.task_id == "t_child")
        .expect("child lane");
    assert!(!child_lane.waiting_on_dependencies);
    assert!(child_lane.dependency_waits.is_empty());
}

#[test]
fn dangling_dependency_stays_waiting_and_visible() {
    let mut plan = fixture();
    let mut child = task("t_child", TaskStatus::Pending, &["s1"]);
    child.depends_on = vec!["t_missing".into()];
    plan.tasks = vec![child];
    let coverage = compute_coverage_report(&plan);
    let snap = build_plan_state_snapshot(
        &plan,
        coverage,
        vec![],
        vec![],
        vec![],
        &HashSet::new(),
        None,
        1,
    );
    let child_lane = snap
        .open_lanes
        .iter()
        .find(|lane| lane.task_id == "t_child")
        .expect("child lane");
    assert!(child_lane.waiting_on_dependencies);
    assert_eq!(child_lane.dependency_waits.len(), 1);
    assert_eq!(child_lane.dependency_waits[0].task_id, "t_missing");
    assert!(child_lane.dependency_waits[0].missing);
    assert_eq!(child_lane.dependency_waits[0].status, None);
    assert_eq!(
        child_lane.dependency_waits[0].name,
        "missing dependency t_missing"
    );
}

#[test]
fn queued_request_marks_stale_planner_dead_with_recovery_action() {
    let mut plan = fixture();
    plan.planner = Some(crate::plan::PlannerRef::Tab {
        tab_id: "dead-planner".into(),
        session_id: None,
        title: None,
    });
    plan.planner_requests.push(crate::plan::PlannerRequest {
        request_id: "req-1".into(),
        payload: "audit".into(),
        payload_blob: None,
        caller_tab_id: Some("caller".into()),
        caller_session_id: Some("caller-session".into()),
        state: crate::plan::PlannerRequestState::Queued,
        created_at: 1,
        accepted_at: None,
        lease_expires_at: None,
        delivery_attempts: 0,
        completed_at: None,
        failed_at: None,
        delivered_to_tab_id: None,
        delivered_to_session_id: None,
        delivered_with_binding_nonce: None,
    });
    let coverage = compute_coverage_report(&plan);
    let snapshot = build_plan_state_snapshot(
        &plan,
        coverage,
        vec![],
        vec![],
        vec![],
        &HashSet::new(),
        None,
        2,
    );

    assert!(snapshot.planner_dead);
    assert_eq!(snapshot.undelivered_request_count, 1);
    assert_eq!(
        snapshot.planner_recovery_action.as_deref(),
        Some("replace_or_rebind_planner")
    );
}
