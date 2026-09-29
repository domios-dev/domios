use std::collections::{BTreeMap, HashSet};

use serde::Serialize;

use crate::plan::cleanup_debt::CleanupDebtTask;
use crate::plan::coverage::CoverageReport;
use crate::plan::peer::{resolve_planner_ref, PeerResolution};
use crate::plan::reconcile::{DriftItem, TabProbe};
use crate::plan::{
    Goal, LoopStatus, OrchestratorRef, Plan, PlanMode, PlanStatus, PlannerRef, PlanningState,
    QaState, RegressionGateOverride, ReviewStage, TaskStatus,
};

use super::next_actions::{compute_next_actions, NextAction};

pub mod analytics;
pub mod lane_rows;
pub mod mission_control;

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
    pub paused: usize,
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
    pub parent_task_id: Option<String>,
    pub subtasks: Vec<super::task_queries::TaskLabel>,
    pub status: TaskStatus,
    pub executor_role: Option<String>,
    pub competition: Option<String>,
    pub live_competition_attempts: usize,
    pub review_stage: ReviewStage,
    pub qa_state: Option<QaState>,
    pub status_label: Option<String>,
    pub clears_review_capacity: bool,
    pub presentation: TaskPresentation,
    pub waiting_on_dependencies: bool,
    pub dependency_waits: Vec<DependencyWait>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskPresentation {
    BlockedOnSubtasks,
    HandoffRequired,
    ActionablePending,
    WaitingOnDependencies,
    InProgress,
    InReview,
    QueuedForQa,
    Blocked,
    Paused,
    Terminal,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DependencyWait {
    pub task_id: String,
    pub name: String,
    pub status: Option<TaskStatus>,
    pub missing: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TaskProjection {
    pub presentation: TaskPresentation,
    pub dependency_waits: Vec<DependencyWait>,
}

pub(crate) fn task_presentation(
    status: &TaskStatus,
    dependency_waits: &[DependencyWait],
    review_stage: &ReviewStage,
    qa_state: &Option<QaState>,
) -> TaskPresentation {
    match status {
        TaskStatus::Pending if dependency_waits.is_empty() => TaskPresentation::ActionablePending,
        TaskStatus::Pending => TaskPresentation::WaitingOnDependencies,
        TaskStatus::InProgress => TaskPresentation::InProgress,
        TaskStatus::InReview
            if *review_stage == ReviewStage::Qa && *qa_state == Some(QaState::Queued) =>
        {
            TaskPresentation::QueuedForQa
        }
        TaskStatus::InReview => TaskPresentation::InReview,
        TaskStatus::Blocked => TaskPresentation::Blocked,
        TaskStatus::Paused => TaskPresentation::Paused,
        TaskStatus::Done | TaskStatus::Cancelled => TaskPresentation::Terminal,
    }
}

pub fn task_projection(plan: &Plan, task: &crate::plan::Task) -> TaskProjection {
    let dependency_waits = dependency_waits(plan, task);
    let has_open_subtasks = plan.tasks.iter().any(|candidate| {
        candidate.parent_task_id.as_deref() == Some(&task.id)
            && !matches!(candidate.status, TaskStatus::Done | TaskStatus::Cancelled)
    });
    let base_presentation = if task.interruption.handoff_required {
        TaskPresentation::HandoffRequired
    } else {
        task_presentation(
            &task.status,
            &dependency_waits,
            &task.review_stage,
            &task.qa_state,
        )
    };
    let presentation = if has_open_subtasks
        && matches!(
            &base_presentation,
            TaskPresentation::ActionablePending | TaskPresentation::InProgress
        ) {
        TaskPresentation::BlockedOnSubtasks
    } else {
        base_presentation
    };
    TaskProjection {
        presentation,
        dependency_waits,
    }
}

impl TaskProjection {
    // A wait on a missing task never clears by itself, so it stays the owner's to replan.
    pub fn needs_orchestrator_action(&self) -> bool {
        match self.presentation {
            TaskPresentation::WaitingOnDependencies => {
                self.dependency_waits.iter().any(|wait| wait.missing)
            }
            TaskPresentation::Paused | TaskPresentation::Terminal => false,
            _ => true,
        }
    }
}

/// Tasks the plan owner can act on now, in plan order. Dependency-waiting and
/// machine-paused tasks advance without the owner and are excluded.
pub fn orchestrator_actionable_task_ids(plan: &Plan) -> Vec<String> {
    plan.tasks
        .iter()
        .filter(|task| task_projection(plan, task).needs_orchestrator_action())
        .map(|task| task.id.clone())
        .collect()
}

pub fn plan_needs_orchestrator_action(plan: &Plan) -> bool {
    plan.tasks
        .iter()
        .any(|task| task_projection(plan, task).needs_orchestrator_action())
}

/// The single count of tasks the Orchestrator still owes attention to beyond
/// ordinary dispatch: every task explicitly `Blocked`, plus every `Pending`
/// task still gated on an unmet dependency. `plan next`, the orchestrator
/// status projection, and the heartbeat all call this one function instead
/// of each deciding independently what "blocked" means — a fixture change
/// here moves all three surfaces together.
pub fn outstanding_blocked_count(plan: &Plan) -> usize {
    plan.tasks
        .iter()
        .filter(|task| {
            task.status == TaskStatus::Blocked
                || (task.status == TaskStatus::Pending
                    && !task_projection(plan, task).dependency_waits.is_empty())
        })
        .count()
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LiveAgent {
    pub task_id: String,
    pub name: String,
    pub tab_id: String,
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
    pub planner_dead: bool,
    pub undelivered_request_count: usize,
    pub planner_recovery_action: Option<String>,
    pub goal: Option<Goal>,
    pub task_counts: TaskCounts,
    pub percent_done: f64,
    pub coverage: CoverageReport,
    pub drift: DriftSummary,
    pub cleanup_debt: Vec<CleanupDebtTask>,
    pub loops: Vec<LoopSnapshot>,
    pub heartbeat: HeartbeatSnapshot,
    pub open_lanes: Vec<OpenLane>,
    pub lanes: Vec<lane_rows::LaneRow>,
    pub lane_history_count: usize,
    pub blocked_lanes: Vec<OpenLane>,
    // Blocked-status tasks plus dependency-gated pending tasks — see
    // `outstanding_blocked_count`. Always >= blocked_lanes.len().
    pub blocked_or_gated_count: usize,
    pub live_agents: Vec<LiveAgent>,
    pub recent_strides: Vec<StrideSnapshot>,
    pub pending_regression_gates: Vec<super::milestone_gates::RegressionGate>,
    // Pending gates minus authorized waivers — the set that actually blocks dispatch.
    pub blocking_regression_gates: Vec<super::milestone_gates::RegressionGate>,
    // Terminal groups whose retrospective is missing or predates their last close.
    pub pending_group_retrospectives: Vec<super::milestone_gates::RegressionGate>,
    pub regression_overrides: Vec<RegressionGateOverride>,
    pub review_gate: super::review_gate::ReviewGateStatus,
    // Stored-field Mission Control projection. The KPI and machine-spend blocks
    // arrive not-recording; the service layer overlays ledger and usage rows.
    pub mission_control: mission_control::MissionControl,
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
            TaskStatus::Paused => c.paused += 1,
            TaskStatus::Done => c.done += 1,
            TaskStatus::Cancelled => c.cancelled += 1,
        }
    }
    c
}

// Completion uses the same all-task denominator exposed beside the percentage.
fn percent_done(counts: &TaskCounts) -> f64 {
    if counts.total == 0 {
        0.0
    } else {
        counts.done as f64 / counts.total as f64
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

pub(crate) fn dependency_waits(plan: &Plan, task: &crate::plan::Task) -> Vec<DependencyWait> {
    task.depends_on
        .iter()
        .filter_map(|dependency_id| {
            match plan
                .tasks
                .iter()
                .find(|candidate| candidate.id == *dependency_id)
            {
                Some(dependency) if !dependency.status.is_terminal() => Some(DependencyWait {
                    task_id: dependency.id.clone(),
                    name: dependency.name.clone(),
                    status: Some(dependency.status.clone()),
                    missing: false,
                }),
                Some(_) => None,
                None => Some(DependencyWait {
                    task_id: dependency_id.clone(),
                    name: format!("missing dependency {}", dependency_id),
                    status: None,
                    missing: true,
                }),
            }
        })
        .collect()
}

/// Readiness gate for advancing planning_state to approved. Empty = ready.
/// The approval doctor and the planner's advance both consult this.
pub fn planning_approval_blockers(readiness: &PlanningReadiness, plan_id: &str) -> Vec<String> {
    let mut blockers = Vec::new();
    if !readiness.has_goal {
        blockers.push(format!(
            "no GOAL vector set — fix: codeterm plan goal set {plan_id} --vector \"<how success is proven>\""
        ));
    }
    if !readiness.has_plan_sections {
        blockers.push(format!(
            "plan has no sections (HOW/execution phases) — fix: codeterm plan section add {plan_id} --title \"<phase>\" --body \"<how>\""
        ));
    }
    if !readiness.has_spec_source {
        blockers.push(format!(
            "no spec source linked (WHAT/WHY) — fix: codeterm spec create --title \"<what>\" then codeterm spec link add --spec <spec_id> --spec-section <sec> --plan-section {plan_id}:<sec> (prose in `plan update --spec-file` does NOT count — a spec entity + coverage edge is required)"
        ));
    }
    if !readiness.has_verification_hint {
        blockers.push(format!(
            "no verification/test-plan hint in plan sections — fix: name a test surface in a section body (build.sh / cargo test / playwright): codeterm plan section add {plan_id} --title Verification --body \"<test commands>\""
        ));
    }
    blockers
}

pub fn build_plan_state_snapshot(
    plan: &Plan,
    coverage: CoverageReport,
    drift: Vec<DriftItem>,
    cleanup_debt: Vec<CleanupDebtTask>,
    lanes: Vec<lane_rows::LaneRow>,
    live_agent_tabs: &HashSet<String>,
    resolved_planner: Option<PeerResolution>,
    now_ms: u64,
) -> PlanStateSnapshot {
    let bound_session_id = match &plan.planner {
        Some(PlannerRef::Tab { session_id, .. }) => session_id.clone(),
        _ => None,
    };
    let planner_resolution = resolved_planner.unwrap_or_else(|| {
        resolve_planner_ref(&plan.planner, &plan.owner, |tab_id| TabProbe {
            exists: true,
            live_agent: live_agent_tabs.contains(tab_id),
            agent_session_id: bound_session_id.clone(),
            session_binding_valid: live_agent_tabs.contains(tab_id),
            chat_delivery_target: live_agent_tabs.contains(tab_id),
            agent_accepting_work: live_agent_tabs.contains(tab_id),
            role: None,
            parent_tab_id: None,
        })
    });
    let counts = task_counts(plan);
    let pct = percent_done(&counts);
    let readiness = planning_readiness(plan);
    let approval_blockers = planning_approval_blockers(&readiness, &plan.id);
    let undelivered_request_count = plan
        .planner_requests
        .iter()
        .filter(|request| !matches!(request.state, crate::plan::PlannerRequestState::Completed))
        .count();
    let planner_dead = matches!(
        &planner_resolution,
        PeerResolution::OrchestratorFallback { .. }
    );
    let delivery_uncertain = plan
        .planner_inbox
        .iter()
        .any(|entry| entry.phase == crate::plan::PlannerDeliveryPhase::DeliveryUncertain);

    let open_lanes = plan
        .tasks
        .iter()
        .filter(|t| {
            matches!(
                t.status,
                TaskStatus::Pending | TaskStatus::InProgress | TaskStatus::InReview
            )
        })
        .map(|t| {
            let projection = task_projection(plan, t);
            let waiting_on_dependencies = !projection.dependency_waits.is_empty();
            OpenLane {
                task_id: t.id.clone(),
                name: t.name.clone(),
                parent_task_id: t.parent_task_id.clone(),
                subtasks: plan
                    .tasks
                    .iter()
                    .filter(|child| child.parent_task_id.as_deref() == Some(&t.id))
                    .map(|child| super::task_queries::TaskLabel {
                        id: child.id.clone(),
                        display_key: child.display_key(),
                        name: child.name.clone(),
                        status: child.status.clone(),
                    })
                    .collect(),
                status: t.status.clone(),
                executor_role: t.executor_role.clone(),
                competition: super::competition::competition_summary(t),
                live_competition_attempts: super::competition::live_competition_attempt_count(t),
                review_stage: t.review_stage.clone(),
                qa_state: t.qa_state.clone(),
                status_label: t.status_label.clone(),
                clears_review_capacity: super::review_gate::clears_review_capacity(plan, t),
                presentation: projection.presentation,
                waiting_on_dependencies,
                dependency_waits: projection.dependency_waits,
            }
        })
        .collect();

    let blocked_lanes = plan
        .tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Blocked)
        .map(|t| {
            let projection = task_projection(plan, t);
            let waiting_on_dependencies = !projection.dependency_waits.is_empty();
            OpenLane {
                task_id: t.id.clone(),
                name: t.name.clone(),
                parent_task_id: t.parent_task_id.clone(),
                subtasks: plan
                    .tasks
                    .iter()
                    .filter(|child| child.parent_task_id.as_deref() == Some(&t.id))
                    .map(|child| super::task_queries::TaskLabel {
                        id: child.id.clone(),
                        display_key: child.display_key(),
                        name: child.name.clone(),
                        status: child.status.clone(),
                    })
                    .collect(),
                status: t.status.clone(),
                executor_role: t.executor_role.clone(),
                competition: super::competition::competition_summary(t),
                live_competition_attempts: super::competition::live_competition_attempt_count(t),
                review_stage: t.review_stage.clone(),
                qa_state: t.qa_state.clone(),
                status_label: t.status_label.clone(),
                clears_review_capacity: super::review_gate::clears_review_capacity(plan, t),
                presentation: projection.presentation,
                waiting_on_dependencies,
                dependency_waits: projection.dependency_waits,
            }
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
    let lane_history_count = plan
        .tasks
        .iter()
        .filter(|task| {
            !task.execution_attempts.is_empty()
                || task.assigned_tab_id.is_some()
                || task.lane.tab_id.is_some()
                || task.lane.branch.is_some()
                || task.lane.worktree_path.is_some()
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
            let tab = t.assigned_tab_id.as_deref()?;
            live_agent_tabs.contains(tab).then(|| LiveAgent {
                task_id: t.id.clone(),
                name: t.name.clone(),
                tab_id: tab.to_string(),
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
        planner_dead,
        undelivered_request_count,
        planner_recovery_action: if delivery_uncertain {
            Some("resolve_delivery_uncertain".into())
        } else if planner_dead {
            Some("replace_or_rebind_planner".into())
        } else {
            None
        },
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
        lanes,
        lane_history_count,
        blocked_or_gated_count: outstanding_blocked_count(plan),
        blocked_lanes,
        live_agents,
        recent_strides: recent_strides(plan),
        pending_regression_gates: super::milestone_gates::pending_regression_gates(plan),
        blocking_regression_gates: super::milestone_gates::blocking_regression_gates(plan),
        pending_group_retrospectives: super::milestone_gates::pending_group_retrospectives(plan),
        regression_overrides: plan.regression_gate_overrides.clone(),
        review_gate: super::review_gate::review_gate_status(plan),
        mission_control: mission_control::build_mission_control(plan, now_ms),
        next_actions: vec![],
    };
    snapshot.next_actions = compute_next_actions(&snapshot);
    snapshot
}
#[cfg(test)]
mod tests;
