use serde::Serialize;

use crate::plan::{LoopStatus, QaState, ReviewStage, TaskStatus};

use super::state::{LoopSnapshot, PlanStateSnapshot};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NextActionKind {
    ApprovePlan,
    Gate,
    Advance,
    Reconcile,
    ConsiderStop,
    Dispatch,
    ClaimQa,
    SpawnTester,
    RegressionGate,
    Continue,
    ReviewWipBlocked,
    WaitingOnDependency,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NextAction {
    pub priority: u8,
    pub kind: NextActionKind,
    pub task_id: Option<String>,
    pub loop_id: Option<String>,
    pub message: String,
}

// name and surface any approval blockers. Shared wording lives in prompts.
fn planning_next_action_message(s: &PlanStateSnapshot) -> String {
    let peer = crate::service_plan::peer_mandate::planner_peer_label(&s.planner_resolution);
    let mut msg = super::prompts_plan::orch_defer_structure_line(&peer);
    if s.planning_approval_blockers.is_empty() {
        msg.push_str(" Structure looks ready — run the quality doctor, then approve.");
    } else {
        msg.push_str(" Approval blockers: ");
        msg.push_str(&s.planning_approval_blockers.join("; "));
        msg.push('.');
    }
    msg
}

fn dependency_wait_names(waits: &[super::state::DependencyWait]) -> String {
    waits
        .iter()
        .map(|wait| {
            if wait.missing {
                format!("{} (missing)", wait.task_id)
            } else {
                let status = wait
                    .status
                    .as_ref()
                    .map(|status| format!("{status:?}"))
                    .unwrap_or_else(|| "unknown".to_string());
                format!("{} ({status})", wait.task_id)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// Running loop with no headroom left: at or one short of budget, or past 80%.
fn loop_near_limit(l: &LoopSnapshot) -> bool {
    if l.status != LoopStatus::Running || l.budget == 0 {
        return false;
    }
    l.count + 1 >= l.budget || (l.count as f64) >= 0.8 * (l.budget as f64)
}

/// Deterministic, advisory next-action ranking over a plan state snapshot.
/// Priority ascending (1 = act first). Never mutates anything.
pub fn compute_next_actions(s: &PlanStateSnapshot) -> Vec<NextAction> {
    let mut actions = Vec::new();

    if !s.execution_tasks_unlocked {
        actions.push(NextAction {
            priority: 1,
            kind: NextActionKind::ApprovePlan,
            task_id: None,
            loop_id: None,
            message: planning_next_action_message(s),
        });
    }

    for lane in &s.open_lanes {
        if lane.status == TaskStatus::InReview {
            if lane.review_stage == ReviewStage::Qa {
                let kind = if lane.qa_state == Some(QaState::Queued) {
                    NextActionKind::ClaimQa
                } else {
                    NextActionKind::SpawnTester
                };
                let message = if lane.qa_state == Some(QaState::Queued) {
                    format!(
                        "claim QA {} ({}) with `codeterm plan qa next {} --claim --note \"<scope>\"`",
                        lane.task_id, lane.name, s.plan_id
                    )
                } else {
                    format!("QA running for {} ({})", lane.task_id, lane.name)
                };
                actions.push(NextAction {
                    priority: 1,
                    kind,
                    task_id: Some(lane.task_id.clone()),
                    loop_id: None,
                    message,
                });
                continue;
            }
            actions.push(NextAction {
                priority: 1,
                kind: NextActionKind::Gate,
                task_id: Some(lane.task_id.clone()),
                loop_id: None,
                message: format!("gate {} ({})", lane.task_id, lane.name),
            });
        }
    }

    for gate in &s.pending_regression_gates {
        actions.push(NextAction {
            priority: 2,
            kind: NextActionKind::RegressionGate,
            task_id: None,
            loop_id: None,
            message: super::prompts_plan::milestone_gate_suggestion(&gate.key, &gate.label),
        });
    }

    for lane in &s.blocked_lanes {
        actions.push(NextAction {
            priority: 2,
            kind: NextActionKind::Advance,
            task_id: Some(lane.task_id.clone()),
            loop_id: None,
            message: format!("advance/unblock {} ({})", lane.task_id, lane.name),
        });
    }

    // Dependency-gated pending lanes never reach ordinary dispatch below, so
    // without this the whole gated tail of a plan reports as "nothing to do"
    // once its unblocking task goes in_progress — codeterm#397.
    for lane in s
        .open_lanes
        .iter()
        .filter(|lane| lane.waiting_on_dependencies)
    {
        actions.push(NextAction {
            priority: 2,
            kind: NextActionKind::WaitingOnDependency,
            task_id: Some(lane.task_id.clone()),
            loop_id: None,
            message: format!(
                "{} ({}) waiting on: {}",
                lane.task_id,
                lane.name,
                dependency_wait_names(&lane.dependency_waits)
            ),
        });
    }

    if s.drift.total > 0 {
        actions.push(NextAction {
            priority: 3,
            kind: NextActionKind::Reconcile,
            task_id: None,
            loop_id: None,
            message: format!("reconcile ({} drift item(s))", s.drift.total),
        });
    }

    let has_unblocked_work = s
        .open_lanes
        .iter()
        .any(|lane| lane.status == TaskStatus::Pending);
    let qa_only = !s.open_lanes.is_empty()
        && s.open_lanes.iter().all(|lane| {
            lane.status == TaskStatus::InReview && lane.review_stage == ReviewStage::Qa
        });
    if s.execution_tasks_unlocked && has_unblocked_work && s.live_agents.is_empty() && !qa_only {
        actions.push(NextAction {
            priority: 1,
            kind: NextActionKind::Continue,
            task_id: None,
            loop_id: None,
            message: format!(
                "continue plan {}: open unblocked work has zero live workers",
                s.plan_id
            ),
        });
    }

    for l in &s.loops {
        if loop_near_limit(l) {
            actions.push(NextAction {
                priority: 4,
                kind: NextActionKind::ConsiderStop,
                task_id: None,
                loop_id: Some(l.id.clone()),
                message: format!(
                    "consider stop: loop {} nearly exhausted ({}/{})",
                    l.id, l.count, l.budget
                ),
            });
        }
    }

    let review_debt_only = s.review_gate.blocked
        && s.review_gate.wip_count < s.review_gate.wip_limit as usize
        && s.review_gate.credit <= -(s.review_gate.wip_limit as i64);
    let pending_lanes = s
        .open_lanes
        .iter()
        .filter(|lane| lane.status == TaskStatus::Pending && !lane.waiting_on_dependencies)
        .collect::<Vec<_>>();
    let has_review_blocked_lane = pending_lanes
        .iter()
        .any(|lane| !review_debt_only || !lane.clears_review_capacity);
    if has_review_blocked_lane && s.review_gate.blocked {
        actions.push(NextAction {
            priority: 1,
            kind: NextActionKind::ReviewWipBlocked,
            task_id: None,
            loop_id: None,
            message: s
                .review_gate
                .reason
                .clone()
                .unwrap_or_else(|| "review_wip_exceeded".into()),
        });
    }
    for lane in pending_lanes {
        if !s.review_gate.blocked || (review_debt_only && lane.clears_review_capacity) {
            let message = if let Some(summary) = lane.competition.as_deref() {
                format!(
                    "{} {} ({summary}): plan.task.compete route for plan {} task {}",
                    crate::service_plan::competition::COMPETITIVE_DISPATCH_NUDGE,
                    lane.task_id,
                    s.plan_id,
                    lane.task_id
                )
            } else {
                format!("dispatch {} ({})", lane.task_id, lane.name)
            };
            let message = if s.blocking_regression_gates.is_empty() {
                message
            } else {
                let gates = s
                    .blocking_regression_gates
                    .iter()
                    .map(|g| g.key.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{message} — BLOCKED by pending regression gate(s): {gates}; clear the gate, tag the task release-blocking/regression-gate, or record an authorized override")
            };
            actions.push(NextAction {
                priority: 5,
                kind: NextActionKind::Dispatch,
                task_id: Some(lane.task_id.clone()),
                loop_id: None,
                message,
            });
        }
    }

    actions.sort_by_key(|a| a.priority);
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service_plan::state::{DriftSummary, LiveAgent, OpenLane, TaskPresentation};

    fn lane(id: &str, status: TaskStatus) -> OpenLane {
        OpenLane {
            task_id: id.into(),
            name: format!("name-{id}"),
            status: status.clone(),
            executor_role: None,
            competition: None,
            live_competition_attempts: 0,
            review_stage: ReviewStage::Review,
            qa_state: None,
            status_label: None,
            presentation: if status == TaskStatus::Pending {
                TaskPresentation::ActionablePending
            } else {
                TaskPresentation::InReview
            },
            clears_review_capacity: false,
            waiting_on_dependencies: false,
            dependency_waits: vec![],
        }
    }

    fn loop_snap(id: &str, count: u32, budget: u32, status: LoopStatus) -> LoopSnapshot {
        LoopSnapshot {
            id: id.into(),
            target: "t".into(),
            budget,
            count,
            stop_when: None,
            status,
            stagnation: 0,
            deviation: None,
            last_iteration_at: None,
        }
    }

    fn empty() -> PlanStateSnapshot {
        PlanStateSnapshot {
            plan_id: "pl".into(),
            title: "T".into(),
            status: crate::plan::PlanStatus::Active,
            mode: crate::plan::PlanMode::Execution,
            planning_state: crate::plan::PlanningState::Approved,
            execution_tasks_unlocked: true,
            planning_approval_blockers: vec![],
            canonical_project_path: "/repo".into(),
            planning_readiness: Default::default(),
            owner: Default::default(),
            planner: None,
            planner_resolution: crate::plan::peer::PeerResolution::Unbound,
            planner_dead: false,
            undelivered_request_count: 0,
            planner_recovery_action: None,
            goal: None,
            task_counts: Default::default(),
            percent_done: 0.0,
            coverage: crate::plan::coverage::CoverageReport {
                plan_covered_fraction: 0.0,
                sections: vec![],
                gaps: vec![],
                unlinked_count: 0,
                unlinked_tasks: vec![],
            },
            drift: DriftSummary::default(),
            cleanup_debt: vec![],
            loops: vec![],
            heartbeat: Default::default(),
            open_lanes: vec![],
            lanes: vec![],
            lane_history_count: 0,
            blocked_or_gated_count: 0,
            blocked_lanes: vec![],
            live_agents: vec![],
            recent_strides: vec![],
            pending_regression_gates: vec![],
            blocking_regression_gates: vec![],
            regression_overrides: vec![],
            review_gate: Default::default(),
            next_actions: vec![],
        }
    }

    #[test]
    fn gate_rule_fires_and_is_silent() {
        let mut s = empty();
        s.open_lanes = vec![lane("t1", TaskStatus::InReview)];
        let a = compute_next_actions(&s);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].kind, NextActionKind::Gate);
        assert_eq!(a[0].task_id.as_deref(), Some("t1"));

        s.open_lanes = vec![lane("t1", TaskStatus::Pending)];
        assert!(compute_next_actions(&s)
            .iter()
            .all(|x| x.kind != NextActionKind::Gate));
    }

    #[test]
    fn advance_rule_fires_and_is_silent() {
        let mut s = empty();
        s.blocked_lanes = vec![lane("tb", TaskStatus::Blocked)];
        let a = compute_next_actions(&s);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].kind, NextActionKind::Advance);
        assert_eq!(a[0].task_id.as_deref(), Some("tb"));

        s.blocked_lanes = vec![];
        assert!(compute_next_actions(&s).is_empty());
    }

    #[test]
    fn reconcile_rule_fires_only_on_drift() {
        let mut s = empty();
        assert!(compute_next_actions(&s).is_empty());
        s.drift.total = 3;
        let a = compute_next_actions(&s);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].kind, NextActionKind::Reconcile);
        assert!(a[0].message.contains("3 drift"));
    }

    #[test]
    fn consider_stop_only_for_running_near_limit() {
        let mut s = empty();
        s.loops = vec![loop_snap("lp", 4, 5, LoopStatus::Running)];
        let a = compute_next_actions(&s);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].kind, NextActionKind::ConsiderStop);
        assert_eq!(a[0].loop_id.as_deref(), Some("lp"));

        s.loops = vec![loop_snap("lp", 1, 5, LoopStatus::Running)];
        assert!(compute_next_actions(&s).is_empty());

        s.loops = vec![loop_snap("lp", 5, 5, LoopStatus::Stopped)];
        assert!(compute_next_actions(&s).is_empty());
    }

    #[test]
    fn dispatch_rule_fires_for_pending() {
        let mut s = empty();
        s.open_lanes = vec![lane("tp", TaskStatus::Pending)];
        let a = compute_next_actions(&s);
        // A lone pending lane with no live worker also emits a Continue nudge.
        assert!(a
            .iter()
            .any(|x| x.kind == NextActionKind::Dispatch && x.task_id.as_deref() == Some("tp")));
    }

    #[test]
    fn review_wip_blocked_suppresses_dispatch_with_structured_reason() {
        let mut s = empty();
        s.open_lanes = vec![lane("tp", TaskStatus::Pending)];
        s.live_agents = vec![LiveAgent {
            task_id: "other".into(),
            name: "other".into(),
            tab_id: "tab_other".into(),
        }];
        s.review_gate = super::super::review_gate::ReviewGateStatus {
            wip_count: 5,
            wip_limit: 5,
            credit: 0,
            blocked: true,
            reason: Some("review_wip_exceeded: 5/5 tasks in_review".into()),
        };
        let a = compute_next_actions(&s);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].kind, NextActionKind::ReviewWipBlocked);
        assert!(a[0].message.contains("review_wip_exceeded"));
        assert!(a
            .iter()
            .all(|action| action.kind != NextActionKind::Dispatch));
    }

    #[test]
    fn review_gate_not_blocked_leaves_dispatch_untouched() {
        let mut s = empty();
        s.open_lanes = vec![lane("tp", TaskStatus::Pending)];
        s.live_agents = vec![LiveAgent {
            task_id: "other".into(),
            name: "other".into(),
            tab_id: "tab_other".into(),
        }];
        s.review_gate = super::super::review_gate::ReviewGateStatus {
            wip_count: 1,
            wip_limit: 5,
            credit: 0,
            blocked: false,
            reason: None,
        };
        let a = compute_next_actions(&s);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].kind, NextActionKind::Dispatch);
    }

    #[test]
    fn stranded_debt_surfaces_capacity_dispatch_but_blocks_ordinary_work() {
        let mut s = empty();
        let mut review = lane("review", TaskStatus::Pending);
        review.clears_review_capacity = true;
        s.open_lanes = vec![review, lane("implementation", TaskStatus::Pending)];
        s.live_agents = vec![LiveAgent {
            task_id: "other".into(),
            name: "other".into(),
            tab_id: "tab_other".into(),
        }];
        s.review_gate = super::super::review_gate::ReviewGateStatus {
            wip_count: 1,
            wip_limit: 5,
            credit: -5,
            blocked: true,
            reason: Some("review_credit_exceeded".into()),
        };

        let actions = compute_next_actions(&s);
        assert!(actions.iter().any(|action| {
            action.kind == NextActionKind::Dispatch && action.task_id.as_deref() == Some("review")
        }));
        assert!(actions.iter().all(|action| {
            action.kind != NextActionKind::Dispatch
                || action.task_id.as_deref() != Some("implementation")
        }));
        assert!(actions
            .iter()
            .any(|action| action.kind == NextActionKind::ReviewWipBlocked));

        s.review_gate.wip_count = 5;
        let saturated = compute_next_actions(&s);
        assert!(saturated
            .iter()
            .all(|action| action.kind != NextActionKind::Dispatch));
    }

    #[test]
    fn dispatch_rule_skips_pending_lane_waiting_on_dependency() {
        let mut s = empty();
        let mut waiting = lane("tp", TaskStatus::Pending);
        waiting.waiting_on_dependencies = true;
        waiting.dependency_waits = vec![super::super::state::DependencyWait {
            task_id: "ta".into(),
            name: "name-ta".into(),
            status: Some(TaskStatus::InProgress),
            missing: false,
        }];
        s.open_lanes = vec![waiting];
        assert!(compute_next_actions(&s)
            .iter()
            .all(|action| action.kind != NextActionKind::Dispatch));
    }

    #[test]
    fn zero_worker_open_work_emits_continuation_but_qa_only_does_not() {
        let mut s = empty();
        s.open_lanes = vec![lane("pending", TaskStatus::Pending)];
        assert!(compute_next_actions(&s)
            .iter()
            .any(|action| action.kind == NextActionKind::Continue));
        s.open_lanes = vec![OpenLane {
            review_stage: ReviewStage::Qa,
            qa_state: Some(QaState::Queued),
            ..lane("qa", TaskStatus::InReview)
        }];
        assert!(compute_next_actions(&s)
            .iter()
            .all(|action| action.kind != NextActionKind::Continue));
    }

    #[test]
    fn priority_ordering_gate_before_dispatch() {
        let mut s = empty();
        s.open_lanes = vec![
            lane("p1", TaskStatus::Pending),
            lane("r1", TaskStatus::InReview),
        ];
        s.blocked_lanes = vec![lane("b1", TaskStatus::Blocked)];
        s.drift.total = 1;
        s.loops = vec![loop_snap("lp", 5, 5, LoopStatus::Running)];

        let a = compute_next_actions(&s);
        let kinds: Vec<NextActionKind> = a.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            vec![
                NextActionKind::Gate,
                NextActionKind::Continue,
                NextActionKind::Advance,
                NextActionKind::Reconcile,
                NextActionKind::ConsiderStop,
                NextActionKind::Dispatch,
            ]
        );
        assert!(a.windows(2).all(|w| w[0].priority <= w[1].priority));
    }

    #[test]
    fn regression_gate_fires_from_pending_group_and_is_advisory() {
        let mut s = empty();
        s.pending_regression_gates = vec![super::super::milestone_gates::RegressionGate {
            key: "phase-1".into(),
            label: "Phase 1".into(),
        }];
        let a = compute_next_actions(&s);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].kind, NextActionKind::RegressionGate);
        assert!(a[0].message.contains("Phase 1"));
        assert!(a[0].message.contains("--group phase-1"));
        assert!(a[0].task_id.is_none());

        s.pending_regression_gates = vec![];
        assert!(compute_next_actions(&s)
            .iter()
            .all(|x| x.kind != NextActionKind::RegressionGate));
    }

    #[test]
    fn planning_state_points_to_approval_gate_and_defers_to_planner() {
        let mut s = empty();
        s.execution_tasks_unlocked = false;
        s.planning_state = crate::plan::PlanningState::Drafting;
        s.planner_resolution = crate::plan::peer::PeerResolution::Planner {
            tab_id: "tab_planner".into(),
        };
        s.planning_approval_blockers = vec!["no GOAL vector set".into()];
        let a = compute_next_actions(&s);
        assert_eq!(a[0].kind, NextActionKind::ApprovePlan);
        assert!(a[0].message.contains("tab_planner"));
        assert!(a[0].message.contains("no GOAL vector set"));
    }

    #[test]
    fn approved_plan_has_no_approval_action() {
        let s = empty(); // execution_tasks_unlocked = true
        let a = compute_next_actions(&s);
        assert!(a.iter().all(|x| x.kind != NextActionKind::ApprovePlan));
    }
}
