use super::*;
use crate::plan::lane_liveness::{DEFAULT_T_OVERDUE_MS, DEFAULT_T_STALL_MS};
use crate::plan::reconcile::PaneProbe;
use crate::plan::{OrchestratorRef, Plan, TaskLaneMetadata, TaskStatus};

fn plan_with(tasks: Vec<Task>) -> Plan {
    let mut p = Plan::new("Lanes".into(), "/repo".into(), OrchestratorRef::default());
    p.tasks = tasks;
    p
}

fn lane_task(id: &str, status: TaskStatus, lane: TaskLaneMetadata, changed: u64) -> Task {
    Task {
        id: id.into(),
        name: format!("title-{id}"),
        kind: "implement".into(),
        model: None,
        model_rationale: None,
        acceptance_criteria: vec![],
        test_plan: None,
        correctness: None,
        status,
        review_stage: Default::default(),
        qa_state: None,
        review_round: 0,
        review_oracle: None,
        review_evidence: vec![],
        review_override: None,
        review_audit: vec![],
        status_label: None,
        priority: Default::default(),
        created_at: 0,
        updated_at: 0,
        status_changed_at: changed,
        handle: Some(id.into()),
        tags: vec![],
        section_ids: vec![],
        body: String::new(),
        role_guidance: None,
        executor_role: None,
        competition: None,
        intent: None,
        parent_task_id: None,
        depends_on: vec![],
        assigned_pane_id: None,
        lane,
        taken_by_role: None,
        peer: None,
        group: None,
        canvas: Default::default(),
        milestones: vec![],
        execution_attempts: vec![],
        pause_reason: None,
        personality: None,
        personality_provenance: None,
        attachments: vec![],
    }
}

fn probe_map(present: &[&str], working: &[&str]) -> impl FnMut(&str) -> PaneProbe {
    let present: Vec<String> = present.iter().map(|s| s.to_string()).collect();
    let working: Vec<String> = working.iter().map(|s| s.to_string()).collect();
    move |pane: &str| PaneProbe {
        exists: present.iter().any(|p| p == pane),
        live_agent: working.iter().any(|p| p == pane),
        role: None,
        parent_pane_id: None,
        ..Default::default()
    }
}

#[test]
fn alive_lane_reads_working() {
    let lane = TaskLaneMetadata {
        pane_id: Some("pane-a".into()),
        branch: Some("lane/a".into()),
        worktree_path: Some("/wt/a".into()),
        ..Default::default()
    };
    let plan = plan_with(vec![lane_task("t_a", TaskStatus::InProgress, lane, 100)]);
    let rows = build_lane_rows(&plan, probe_map(&["pane-a"], &["pane-a"]), 200);
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!(r.task_id, "t_a");
    assert_eq!(r.liveness, LaneLiveness::Working);
    assert_eq!(r.branch.as_deref(), Some("lane/a"));
    assert_eq!(r.worktree.as_deref(), Some("/wt/a"));
    assert!(!r.uncleaned);
}

#[test]
fn absent_local_pane_reads_gone() {
    let lane = TaskLaneMetadata {
        pane_id: Some("pane-dead".into()),
        branch: Some("lane/d".into()),
        ..Default::default()
    };
    let plan = plan_with(vec![lane_task("t_d", TaskStatus::InProgress, lane, 0)]);
    let rows = build_lane_rows(&plan, probe_map(&[], &[]), 1_000);
    assert_eq!(rows[0].liveness, LaneLiveness::PaneGone);
}

#[test]
fn idle_present_pane_past_stall_reads_idle_stalled() {
    let lane = TaskLaneMetadata {
        pane_id: Some("pane-i".into()),
        ..Default::default()
    };
    let now = DEFAULT_T_STALL_MS + 5_000;
    let plan = plan_with(vec![lane_task("t_i", TaskStatus::InReview, lane, 0)]);
    // present but not working -> idle; activity age > stall threshold.
    let rows = build_lane_rows(&plan, probe_map(&["pane-i"], &[]), now);
    assert_eq!(rows[0].liveness, LaneLiveness::IdleStalled);
}

#[test]
fn working_pane_past_overdue_reads_report_overdue() {
    let lane = TaskLaneMetadata {
        pane_id: Some("pane-o".into()),
        ..Default::default()
    };
    let now = DEFAULT_T_OVERDUE_MS + 5_000;
    let plan = plan_with(vec![lane_task("t_o", TaskStatus::InProgress, lane, 0)]);
    let rows = build_lane_rows(&plan, probe_map(&["pane-o"], &["pane-o"]), now);
    assert_eq!(rows[0].liveness, LaneLiveness::ReportOverdue);
}

#[test]
fn peer_lane_parks_offline_without_probe() {
    let lane = TaskLaneMetadata {
        pane_id: Some("pane-p".into()),
        ..Default::default()
    };
    let mut task = lane_task("t_p", TaskStatus::InProgress, lane, 0);
    task.peer = Some("peer-x".into());
    let plan = plan_with(vec![task]);
    let rows = build_lane_rows(&plan, probe_map(&[], &[]), 1_000);
    assert_eq!(rows[0].liveness, LaneLiveness::PeerOffline);
}

#[test]
fn no_pane_lane_reads_no_pane() {
    let plan = plan_with(vec![lane_task(
        "t_n",
        TaskStatus::Pending,
        TaskLaneMetadata::default(),
        0,
    )]);
    let rows = build_lane_rows(&plan, probe_map(&[], &[]), 1_000);
    assert_eq!(rows[0].liveness, LaneLiveness::NoPane);
    assert!(!rows[0].uncleaned);
}

#[test]
fn terminal_lanes_are_excluded_but_uncleaned_flag_on_open() {
    let lane = TaskLaneMetadata {
        pane_id: Some("pane-u".into()),
        branch: Some("lane/u".into()),
        worktree_path: Some("/wt/u".into()),
        cleanup_status: Some("pending".into()),
        ..Default::default()
    };
    let plan = plan_with(vec![
        lane_task("t_open", TaskStatus::InReview, lane, 0),
        lane_task("t_done", TaskStatus::Done, TaskLaneMetadata::default(), 0),
    ]);
    let rows = build_lane_rows(&plan, probe_map(&["pane-u"], &["pane-u"]), 100);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].task_id, "t_open");
    assert!(rows[0].uncleaned);
}
