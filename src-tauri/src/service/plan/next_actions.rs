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
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NextAction {
    pub priority: u8,
    pub kind: NextActionKind,
    pub task_id: Option<String>,
    pub loop_id: Option<String>,
    pub message: String,
}

// Planning-phase next action: delegate structure drafting to the Planner by
// name and surface any approval blockers. Shared wording lives in prompts.
fn planning_next_action_message(s: &PlanStateSnapshot) -> String {
    let peer = crate::service::plan::peer_mandate::planner_peer_label(&s.planner_resolution);
    let mut msg = crate::prompts::orch_defer_structure_line(&peer);
    if s.planning_approval_blockers.is_empty() {
        msg.push_str(" Structure looks ready — run the quality doctor, then approve.");
    } else {
        msg.push_str(" Approval blockers: ");
        msg.push_str(&s.planning_approval_blockers.join("; "));
        msg.push('.');
    }
    msg
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

    for lane in &s.blocked_lanes {
        actions.push(NextAction {
            priority: 2,
            kind: NextActionKind::Advance,
            task_id: Some(lane.task_id.clone()),
            loop_id: None,
            message: format!("advance/unblock {} ({})", lane.task_id, lane.name),
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

    for lane in &s.open_lanes {
        if lane.status == TaskStatus::Pending {
            let message = if let Some(summary) = lane.competition.as_deref() {
                format!(
                    "{} {} ({summary}): plan.task.compete route for plan {} task {}",
                    crate::service::plan::competition::COMPETITIVE_DISPATCH_NUDGE,
                    lane.task_id,
                    s.plan_id,
                    lane.task_id
                )
            } else {
                format!("dispatch {} ({})", lane.task_id, lane.name)
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
    use crate::service::plan::state::{DriftSummary, OpenLane};

    fn lane(id: &str, status: TaskStatus) -> OpenLane {
        OpenLane {
            task_id: id.into(),
            name: format!("name-{id}"),
            status,
            executor_role: None,
            competition: None,
            live_competition_attempts: 0,
            review_stage: ReviewStage::Review,
            qa_state: None,
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
            blocked_lanes: vec![],
            live_agents: vec![],
            recent_strides: vec![],
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
        // running, 4/5 -> near limit
        s.loops = vec![loop_snap("lp", 4, 5, LoopStatus::Running)];
        let a = compute_next_actions(&s);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].kind, NextActionKind::ConsiderStop);
        assert_eq!(a[0].loop_id.as_deref(), Some("lp"));

        // running but plenty of headroom -> silent
        s.loops = vec![loop_snap("lp", 1, 5, LoopStatus::Running)];
        assert!(compute_next_actions(&s).is_empty());

        // near limit but already stopped -> silent
        s.loops = vec![loop_snap("lp", 5, 5, LoopStatus::Stopped)];
        assert!(compute_next_actions(&s).is_empty());
    }

    #[test]
    fn dispatch_rule_fires_for_pending() {
        let mut s = empty();
        s.open_lanes = vec![lane("tp", TaskStatus::Pending)];
        let a = compute_next_actions(&s);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].kind, NextActionKind::Dispatch);
        assert_eq!(a[0].task_id.as_deref(), Some("tp"));
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
                NextActionKind::Advance,
                NextActionKind::Reconcile,
                NextActionKind::ConsiderStop,
                NextActionKind::Dispatch,
            ]
        );
        // priorities must be non-decreasing
        assert!(a.windows(2).all(|w| w[0].priority <= w[1].priority));
    }

    #[test]
    fn planning_state_points_to_approval_gate_and_defers_to_planner() {
        let mut s = empty();
        s.execution_tasks_unlocked = false;
        s.planning_state = crate::plan::PlanningState::Drafting;
        s.planner_resolution = crate::plan::peer::PeerResolution::Planner {
            pane_id: "pane_planner".into(),
        };
        s.planning_approval_blockers = vec!["no GOAL vector set".into()];
        let a = compute_next_actions(&s);
        assert_eq!(a[0].kind, NextActionKind::ApprovePlan);
        assert!(a[0].message.contains("pane_planner"));
        assert!(a[0].message.contains("no GOAL vector set"));
    }

    #[test]
    fn approved_plan_has_no_approval_action() {
        let s = empty(); // execution_tasks_unlocked = true
        let a = compute_next_actions(&s);
        assert!(a.iter().all(|x| x.kind != NextActionKind::ApprovePlan));
    }
}
