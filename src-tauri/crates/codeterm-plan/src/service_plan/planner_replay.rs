use crate::plan::{OrchestratorRef, Plan, PlannerRef, PlannerRequestState};

use super::{PlanError, PlanStore};

/// The fields planner replay reads from a plan bound to one planner tab.
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct PlannerReplayCandidate {
    pub plan_id: String,
    pub session_id: String,
    pub planner: Option<PlannerRef>,
    pub owner: OrchestratorRef,
}

pub fn planner_replay_candidate(plan: &Plan, tab_id: &str) -> Option<PlannerReplayCandidate> {
    let Some(PlannerRef::Tab {
        tab_id: bound,
        session_id: Some(session_id),
        ..
    }) = &plan.planner
    else {
        return None;
    };
    if bound != tab_id
        || plan
            .planner_requests
            .iter()
            .all(|request| request.state == PlannerRequestState::Completed)
    {
        return None;
    }
    Some(PlannerReplayCandidate {
        plan_id: plan.id.clone(),
        session_id: session_id.clone(),
        planner: plan.planner.clone(),
        owner: plan.owner.clone(),
    })
}

impl PlanStore {
    pub fn planner_replay_candidates(
        &self,
        tab_id: &str,
    ) -> Result<Vec<PlannerReplayCandidate>, PlanError> {
        self.database.planner_replay_rows(tab_id)
    }
}

#[cfg(test)]
mod tests;
