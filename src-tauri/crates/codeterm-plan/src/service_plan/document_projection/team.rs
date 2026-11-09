use super::{PlanError, PlanStore};
#[cfg(any(test, feature = "test-support"))]
use crate::plan::Plan;
use crate::plan::{PlanLifecycleEvent, PlanMode, PlanStatus, PlanningState};
use crate::service_plan::state::{mission_control::PlanEvent, TaskCounts};

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq)]
pub struct TeamPlanGoalRead {
    pub vector: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq)]
pub struct TeamPlanMetadataRead {
    pub id: String,
    pub title: String,
    pub team_id: Option<String>,
    pub status: PlanStatus,
    #[serde(default)]
    pub mode: PlanMode,
    #[serde(default)]
    pub planning_state: PlanningState,
    pub goal: Option<TeamPlanGoalRead>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TeamPlanRead {
    pub metadata: TeamPlanMetadataRead,
    pub task_counts: TaskCounts,
    pub lifecycle_events: Vec<(usize, PlanLifecycleEvent)>,
    pub stored_events: Vec<(usize, PlanEvent)>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TeamEventCursor {
    pub ts: u64,
    pub plan_id: String,
    pub event_id: String,
}

#[cfg(any(test, feature = "test-support"))]
impl From<&Plan> for TeamPlanRead {
    fn from(plan: &Plan) -> Self {
        Self {
            metadata: TeamPlanMetadataRead {
                id: plan.id.clone(),
                title: plan.title.clone(),
                team_id: plan.team_id.clone(),
                status: plan.status.clone(),
                mode: plan.mode.clone(),
                planning_state: plan.planning_state,
                goal: plan.goal.as_ref().map(|goal| TeamPlanGoalRead {
                    vector: goal.vector.clone(),
                }),
                created_at: plan.created_at,
                updated_at: plan.updated_at,
            },
            task_counts: crate::service_plan::state::task_counts(plan),
            lifecycle_events: plan.lifecycle_events.iter().cloned().enumerate().collect(),
            stored_events: crate::service_plan::state::mission_control::build_plan_event_log(plan)
                .entries
                .into_iter()
                .enumerate()
                .collect(),
        }
    }
}

impl PlanStore {
    pub fn team_mission_control_projection(
        &self,
        team_id: &str,
        limit: usize,
        cursor: Option<&TeamEventCursor>,
    ) -> Result<Vec<TeamPlanRead>, PlanError> {
        self.database
            .team_mission_control_input(team_id, limit, cursor)
    }
}
