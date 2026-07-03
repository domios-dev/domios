use std::collections::{BTreeMap, HashSet};

use serde::Serialize;

use crate::plan::cleanup_debt::CleanupDebtTask;
use crate::plan::coverage::{compute_coverage_report, CoverageReport};
use crate::plan::peer::{resolve_planner_ref, PeerResolution};
use crate::plan::reconcile::{DriftItem, PaneProbe};
use crate::plan::{
    Goal, LoopStatus, OrchestratorRef, Plan, PlanMode, PlanStatus, PlannerRef, PlanningState,
    QaState, ReviewStage, TaskStatus,
};
use crate::service::plan::PlanError;
use crate::service::ServiceContext;

use super::next_actions::{compute_next_actions, NextAction};

const RECENT_STRIDE_LIMIT: usize = 10;

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct PlanningReadiness {
    pub has_goal: bool,
    pub has_spec_source: bool,
    pub has_plan_sections: bool,
    pub spec_link_coverage: f64,
    pub linked_plan_sections: usize,
    pub total_plan_sections: usize,
    pub has_approval_gate_hint: bool,
    pub has_verification_hint: bool,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct TaskCounts {
    pub pending: usize,
    pub in_progress: usize,
    pub in_review: usize,
    pub blocked: usize,
    pub parked: usize,
    pub done: usize,
    pub cancelled: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct DriftSummary {
    pub total: usize,
    pub by_kind: BTreeMap<String, usize>,
    pub items: Vec<DriftItem>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LoopSnapshot {
    pub id: String,
    pub target: String,
    pub budget: u32,
    pub count: u32,
    pub stop_when: Option<String>,
    pub status: LoopStatus,
    pub stagnation: u32,
    pub deviation: Option<String>,
    pub last_iteration_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct HeartbeatSnapshot {
    pub current_focus: Option<String>,
    pub active_lanes: Vec<String>,
    pub qa_queued: usize,
    pub qa_running_task_id: Option<String>,
    pub qa_running_age_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OpenLane {
    pub task_id: String,
    pub name: String,
    pub status: TaskStatus,
    pub executor_role: Option<String>,
    pub competition: Option<String>,
    pub live_competition_attempts: usize,
    pub review_stage: ReviewStage,
    pub qa_state: Option<QaState>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LiveAgent {
    pub task_id: String,
    pub name: String,
    pub pane_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StrideSnapshot {
    pub id: String,
    // Empty for plan-level (task-less) strides; see `plan_level`.
    pub task_id: String,
    #[serde(default)]
    pub plan_level: bool,
    pub kind: String,
    pub text: String,
    pub ts: u64,
    pub loop_id: Option<String>,
    pub iteration: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanStateSnapshot {
    pub plan_id: String,
    pub title: String,
    pub status: PlanStatus,
    pub mode: PlanMode,
    pub planning_state: PlanningState,
    pub execution_tasks_unlocked: bool,
    pub planning_approval_blockers: Vec<String>,
    pub canonical_project_path: String,
    pub planning_readiness: PlanningReadiness,
    pub owner: OrchestratorRef,
    pub planner: Option<PlannerRef>,
    pub planner_resolution: PeerResolution,
    pub goal: Option<Goal>,
    pub task_counts: TaskCounts,
    pub percent_done: f64,
    pub coverage: CoverageReport,
    pub drift: DriftSummary,
    pub cleanup_debt: Vec<CleanupDebtTask>,
    pub loops: Vec<LoopSnapshot>,
    pub heartbeat: HeartbeatSnapshot,
    pub open_lanes: Vec<OpenLane>,
    pub blocked_lanes: Vec<OpenLane>,
    pub live_agents: Vec<LiveAgent>,
    pub recent_strides: Vec<StrideSnapshot>,
    pub next_actions: Vec<NextAction>,
}

fn task_counts(plan: &Plan) -> TaskCounts {
    let mut c = TaskCounts::default();
    for task in &plan.tasks {
        c.total += 1;
        match task.status {
            TaskStatus::Pending => c.pending += 1,
            TaskStatus::InProgress => c.in_progress += 1,
            TaskStatus::InReview => c.in_review += 1,
            TaskStatus::Blocked => c.blocked += 1,
            TaskStatus::Parked => c.parked += 1,
            TaskStatus::Done => c.done += 1,
            TaskStatus::Cancelled => c.cancelled += 1,
        }
    }
    c
}

// Completion over non-cancelled, non-parked tasks: both are out of active scope.
fn percent_done(counts: &TaskCounts) -> f64 {
    let denom = counts
        .total
        .saturating_sub(counts.cancelled)
        .saturating_sub(counts.parked);
    if denom == 0 {
        0.0
    } else {
        counts.done as f64 / denom as f64
    }
}

fn drift_summary(drift: Vec<DriftItem>) -> DriftSummary {
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    for item in &drift {
        let key = serde_json::to_value(&item.kind)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| format!("{:?}", item.kind));
        *by_kind.entry(key).or_insert(0) += 1;
    }
    DriftSummary {
        total: drift.len(),
        by_kind,
        items: drift,
    }
}

fn recent_strides(plan: &Plan) -> Vec<StrideSnapshot> {
    let mut strides: Vec<StrideSnapshot> = plan
        .tasks
        .iter()
        .flat_map(|task| {
            task.milestones.iter().map(move |m| StrideSnapshot {
                id: m.id.clone(),
                task_id: task.id.clone(),
                plan_level: false,
                kind: m.kind.clone(),
                text: m.text.clone(),
                ts: m.ts,
                loop_id: m.loop_id.clone(),
                iteration: m.iteration,
            })
        })
        .collect();
    strides.extend(plan.strides.iter().map(|m| StrideSnapshot {
        id: m.id.clone(),
        task_id: String::new(),
        plan_level: true,
        kind: m.kind.clone(),
        text: m.text.clone(),
        ts: m.ts,
        loop_id: m.loop_id.clone(),
        iteration: m.iteration,
    }));
    strides.sort_by(|a, b| b.ts.cmp(&a.ts));
    strides.truncate(RECENT_STRIDE_LIMIT);
    strides
}

fn section_count(sections: &[crate::plan::Section]) -> usize {
    sections
        .iter()
        .map(|section| 1 + section_count(&section.subsections))
        .sum()
}

fn section_text(sections: &[crate::plan::Section], out: &mut String) {
    for section in sections {
        out.push_str(&section.title.to_lowercase());
        out.push('\n');
        out.push_str(&section.body.to_lowercase());
        out.push('\n');
        section_text(&section.subsections, out);
    }
}

fn planning_readiness(plan: &Plan) -> PlanningReadiness {
    let total_plan_sections = section_count(&plan.sections);
    let linked_plan_sections = plan
        .spec_links
        .iter()
        .map(|link| link.plan_section_id.as_str())
        .collect::<HashSet<_>>()
        .len();
    let spec_link_coverage = if total_plan_sections == 0 {
        0.0
    } else {
        linked_plan_sections.min(total_plan_sections) as f64 / total_plan_sections as f64
    };
    let mut text = String::new();
    section_text(&plan.sections, &mut text);
    PlanningReadiness {
        has_goal: plan
            .goal
            .as_ref()
            .is_some_and(|goal| !goal.vector.trim().is_empty()),
        has_spec_source: plan.primary_spec_id.is_some()
            || !plan.spec_refs.is_empty()
            || !plan.spec_links.is_empty(),
        has_plan_sections: !plan.sections.is_empty(),
        spec_link_coverage,
        linked_plan_sections,
        total_plan_sections,
        has_approval_gate_hint: text.contains("approval")
            || text.contains("self-review")
            || text.contains("review gate"),
        has_verification_hint: text.contains("verification")
            || text.contains("test plan")
            || text.contains("build.sh")
            || text.contains("cargo test")
            || text.contains("playwright"),
    }
}

/// Readiness gate for advancing planning_state to approved. Empty = ready.
/// The approval doctor and the planner's advance both consult this.
pub fn planning_approval_blockers(readiness: &PlanningReadiness) -> Vec<String> {
    let mut blockers = Vec::new();
    if !readiness.has_goal {
        blockers.push("no GOAL vector set".into());
    }
    if !readiness.has_plan_sections {
        blockers.push("plan has no sections (HOW/execution phases)".into());
    }
    if !readiness.has_spec_source {
        blockers.push("no spec source linked (WHAT/WHY)".into());
    }
    if !readiness.has_verification_hint {
        blockers.push("no verification/test-plan hint in plan sections".into());
    }
    blockers
}

pub fn build_plan_state_snapshot(
    plan: &Plan,
    coverage: CoverageReport,
    drift: Vec<DriftItem>,
    cleanup_debt: Vec<CleanupDebtTask>,
    live_agent_panes: &HashSet<String>,
    now_ms: u64,
) -> PlanStateSnapshot {
    let planner_resolution = resolve_planner_ref(&plan.planner, &plan.owner, |pane_id| PaneProbe {
        exists: true,
        live_agent: live_agent_panes.contains(pane_id),
        role: None,
        parent_pane_id: None,
    });
    let counts = task_counts(plan);
    let pct = percent_done(&counts);
    let readiness = planning_readiness(plan);
    let approval_blockers = planning_approval_blockers(&readiness);

    let open_lanes = plan
        .tasks
        .iter()
        .filter(|t| {
            matches!(
                t.status,
                TaskStatus::Pending | TaskStatus::InProgress | TaskStatus::InReview
            )
        })
        .map(|t| OpenLane {
            task_id: t.id.clone(),
            name: t.name.clone(),
            status: t.status.clone(),
            executor_role: t.executor_role.clone(),
            competition: super::competition::competition_summary(t),
            live_competition_attempts: super::competition::live_competition_attempt_count(t),
            review_stage: t.review_stage.clone(),
            qa_state: t.qa_state.clone(),
        })
        .collect();

    let blocked_lanes = plan
        .tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Blocked)
        .map(|t| OpenLane {
            task_id: t.id.clone(),
            name: t.name.clone(),
            status: t.status.clone(),
            executor_role: t.executor_role.clone(),
            competition: super::competition::competition_summary(t),
            live_competition_attempts: super::competition::live_competition_attempt_count(t),
            review_stage: t.review_stage.clone(),
            qa_state: t.qa_state.clone(),
        })
        .collect();
    let qa_queued = plan
        .tasks
        .iter()
        .filter(|task| {
            task.status == TaskStatus::InReview
                && task.review_stage == ReviewStage::Qa
                && task.qa_state == Some(QaState::Queued)
        })
        .count();
    let qa_running = plan.tasks.iter().find(|task| {
        task.status == TaskStatus::InReview
            && task.review_stage == ReviewStage::Qa
            && task.qa_state == Some(QaState::Running)
    });

    let live_agents = plan
        .tasks
        .iter()
        .filter(|t| !matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled))
        .filter_map(|t| {
            let pane = t.assigned_pane_id.as_deref()?;
            live_agent_panes.contains(pane).then(|| LiveAgent {
                task_id: t.id.clone(),
                name: t.name.clone(),
                pane_id: pane.to_string(),
            })
        })
        .collect();

    let loops = plan
        .loops
        .iter()
        .map(|l| LoopSnapshot {
            id: l.id.clone(),
            target: l.target.clone(),
            budget: l.budget,
            count: l.count,
            stop_when: l.stop_when.clone(),
            status: l.status.clone(),
            stagnation: l.stagnation,
            deviation: l.deviation.clone(),
            last_iteration_at: l.last_iteration_at,
        })
        .collect();

    let mut snapshot = PlanStateSnapshot {
        plan_id: plan.id.clone(),
        title: plan.title.clone(),
        status: plan.status.clone(),
        mode: plan.mode.clone(),
        planning_state: plan.planning_state,
        execution_tasks_unlocked: plan.execution_tasks_unlocked(),
        planning_approval_blockers: approval_blockers,
        canonical_project_path: if plan.canonical_project_path.trim().is_empty() {
            plan.project_path.clone()
        } else {
            plan.canonical_project_path.clone()
        },
        planning_readiness: readiness,
        owner: plan.owner.clone(),
        planner: plan.planner.clone(),
        planner_resolution,
        goal: plan.goal.clone(),
        task_counts: counts,
        percent_done: pct,
        coverage,
        drift: drift_summary(drift),
        cleanup_debt,
        loops,
        heartbeat: HeartbeatSnapshot {
            current_focus: plan.heartbeat.current_focus.clone(),
            active_lanes: plan.heartbeat.active_lanes.clone(),
            qa_queued,
            qa_running_task_id: qa_running.map(|task| task.id.clone()),
            qa_running_age_ms: qa_running.map(|task| now_ms.saturating_sub(task.status_changed_at)),
        },
        open_lanes,
        blocked_lanes,
        live_agents,
        recent_strides: recent_strides(plan),
        next_actions: vec![],
    };
    snapshot.next_actions = compute_next_actions(&snapshot);
    snapshot
}

impl ServiceContext {
    /// Single read-only state snapshot for a plan. Reuses coverage + drift +
    /// pane probing; never mutates the persisted plan.
    pub fn build_plan_state(
        &self,
        plan_id: &str,
        now_ms: u64,
    ) -> Result<PlanStateSnapshot, PlanError> {
        let plan = self.plan_store.get(plan_id).ok_or(PlanError::NotFound)?;
        let coverage = compute_coverage_report(&plan);
        let reconcile = self.reconcile_plan(plan_id, false, None, now_ms)?;
        let drift = reconcile.drift;
        let cleanup_debt = reconcile.cleanup_debt;

        let mut live_panes: HashSet<String> = HashSet::new();
        let mut probed: HashSet<String> = HashSet::new();
        for task in &plan.tasks {
            if let Some(pane) = task.assigned_pane_id.as_deref() {
                if probed.insert(pane.to_string()) && self.pane_probe(pane).live_agent {
                    live_panes.insert(pane.to_string());
                }
            }
        }
        // Probe the planner pane so its liveness is reflected in the snapshot.
        if let Some(PlannerRef::Pane { pane_id, .. }) = &plan.planner {
            if probed.insert(pane_id.clone()) && self.pane_probe(pane_id).live_agent {
                live_panes.insert(pane_id.clone());
            }
        }

        Ok(build_plan_state_snapshot(
            &plan,
            coverage,
            drift,
            cleanup_debt,
            &live_panes,
            now_ms,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            test_plan: None,
            correctness: None,
            status,
            review_stage: Default::default(),
            qa_state: None,
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
            parent_task_id: None,
            assigned_pane_id: None,
            lane: Default::default(),
            taken_by_role: None,
            peer: None,
            group: None,
            canvas: Default::default(),
            milestones: vec![],
            execution_attempts: vec![],
            park_reason: None,
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
        in_review.assigned_pane_id = Some("pane_live".into());
        in_review.milestones = vec![milestone("m3", 200)];
        let mut in_prog = task("t_prog", TaskStatus::InProgress, &["s2"]);
        in_prog.assigned_pane_id = Some("pane_dead".into());
        let pending = task("t_pending", TaskStatus::Pending, &[]);
        let cancelled = task("t_cancel", TaskStatus::Cancelled, &["s2"]);

        p.tasks = vec![done, in_review, in_prog, pending, cancelled];
        p
    }

    #[test]
    fn snapshot_counts_coverage_lanes_agents_and_strides() {
        let plan = fixture();
        let coverage = compute_coverage_report(&plan);
        let drift = vec![DriftItem {
            task_id: "t_prog".into(),
            plan_id: plan.id.clone(),
            kind: DriftKind::InProgressPaneGone,
            detail: "gone".into(),
            suggested_fix: "needs-reassign".into(),
            not_mine: false,
        }];
        let live: HashSet<String> = ["pane_live".to_string()].into_iter().collect();

        let snap = build_plan_state_snapshot(&plan, coverage, drift, vec![], &live, 1);

        assert_eq!(snap.task_counts.total, 5);
        assert_eq!(snap.task_counts.done, 1);
        assert_eq!(snap.task_counts.in_review, 1);
        assert_eq!(snap.task_counts.in_progress, 1);
        assert_eq!(snap.task_counts.pending, 1);
        assert_eq!(snap.task_counts.cancelled, 1);
        // done / (total - cancelled) = 1 / 4
        assert!((snap.percent_done - 0.25).abs() < 1e-9);

        // coverage: s1 covered, s2 covered -> fraction 1.0 (both have tasks)
        assert!((snap.coverage.plan_covered_fraction - 1.0).abs() < 1e-9);

        // open lanes: pending + in_review + in_progress (not done / cancelled / blocked)
        let lane_ids: Vec<&str> = snap.open_lanes.iter().map(|l| l.task_id.as_str()).collect();
        assert_eq!(lane_ids.len(), 3);
        assert!(lane_ids.contains(&"t_review"));
        assert!(lane_ids.contains(&"t_pending"));
        assert!(lane_ids.contains(&"t_prog"));

        // live agents: only the task bound to a live pane
        assert_eq!(snap.live_agents.len(), 1);
        assert_eq!(snap.live_agents[0].task_id, "t_review");
        assert_eq!(snap.live_agents[0].pane_id, "pane_live");

        // drift summary
        assert_eq!(snap.drift.total, 1);
        assert_eq!(snap.drift.by_kind.get("in_progress_pane_gone"), Some(&1));

        // loops
        assert_eq!(snap.loops.len(), 1);
        assert_eq!(snap.loops[0].budget, 5);
        assert_eq!(snap.loops[0].count, 2);

        // heartbeat
        assert_eq!(snap.heartbeat.current_focus.as_deref(), Some("foundation"));
        assert_eq!(snap.heartbeat.active_lanes, vec!["loop-state".to_string()]);

        // strides: newest first across tasks
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
        let snap = build_plan_state_snapshot(&plan, coverage, vec![], vec![], &HashSet::new(), 1);
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
        let snap = build_plan_state_snapshot(&plan, coverage, vec![], vec![], &HashSet::new(), 1);
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
        let _ = build_plan_state_snapshot(&plan, coverage, vec![], vec![], &HashSet::new(), 1);
        assert_eq!(plan, before);
    }
}
