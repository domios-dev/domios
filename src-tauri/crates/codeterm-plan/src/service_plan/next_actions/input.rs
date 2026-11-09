use crate::plan::peer::PeerResolution;
use crate::plan::{LoopStatus, QaState, ReviewStage, TaskStatus};
use crate::service_plan::milestone_gates::RegressionGate;
use crate::service_plan::state::{DependencyWait, OpenLane, PlanStateSnapshot, TaskPresentation};

pub(in crate::service_plan) struct ActionLane<'a> {
    pub task_id: &'a str,
    pub name: &'a str,
    pub status: TaskStatus,
    pub review_stage: ReviewStage,
    pub qa_state: Option<QaState>,
    pub presentation: TaskPresentation,
    pub waiting_on_dependencies: bool,
    pub dependency_waits: &'a [DependencyWait],
    pub competition: Option<&'a str>,
}

impl<'a> From<&'a OpenLane> for ActionLane<'a> {
    fn from(lane: &'a OpenLane) -> Self {
        Self {
            task_id: &lane.task_id,
            name: &lane.name,
            status: lane.status.clone(),
            review_stage: lane.review_stage.clone(),
            qa_state: lane.qa_state.clone(),
            presentation: lane.presentation.clone(),
            waiting_on_dependencies: lane.waiting_on_dependencies,
            dependency_waits: &lane.dependency_waits,
            competition: lane.competition.as_deref(),
        }
    }
}

pub(in crate::service_plan) struct ActionLoop<'a> {
    pub id: &'a str,
    pub status: LoopStatus,
    pub count: u32,
    pub budget: u32,
}

pub(in crate::service_plan) struct NextActionInput<'a> {
    pub plan_id: &'a str,
    pub execution_tasks_unlocked: bool,
    pub planning_approval_blockers: &'a [String],
    pub planner_resolution: &'a PeerResolution,
    pub open_lanes: Vec<ActionLane<'a>>,
    pub blocked_lanes: Vec<ActionLane<'a>>,
    pub drift_total: usize,
    pub has_live_agents: bool,
    pub loops: Vec<ActionLoop<'a>>,
    pub pending_regression_gates: &'a [RegressionGate],
    pub pending_group_retrospectives: &'a [RegressionGate],
}

impl<'a> From<&'a PlanStateSnapshot> for NextActionInput<'a> {
    fn from(snapshot: &'a PlanStateSnapshot) -> Self {
        Self {
            plan_id: &snapshot.plan_id,
            execution_tasks_unlocked: snapshot.execution_tasks_unlocked,
            planning_approval_blockers: &snapshot.planning_approval_blockers,
            planner_resolution: &snapshot.planner_resolution,
            open_lanes: snapshot.open_lanes.iter().map(ActionLane::from).collect(),
            blocked_lanes: snapshot
                .blocked_lanes
                .iter()
                .map(ActionLane::from)
                .collect(),
            drift_total: snapshot.drift.total,
            has_live_agents: !snapshot.live_agents.is_empty(),
            loops: snapshot
                .loops
                .iter()
                .map(|item| ActionLoop {
                    id: &item.id,
                    status: item.status.clone(),
                    count: item.count,
                    budget: item.budget,
                })
                .collect(),
            pending_regression_gates: &snapshot.pending_regression_gates,
            pending_group_retrospectives: &snapshot.pending_group_retrospectives,
        }
    }
}
