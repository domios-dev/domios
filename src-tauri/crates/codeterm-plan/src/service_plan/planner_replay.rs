use crate::plan::{OrchestratorRef, Plan, PlannerRef, PlannerRequestState};

use super::PlanStore;

/// The fields planner replay reads from a plan bound to one planner tab.
#[derive(Debug, Clone, PartialEq)]
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
    /// Borrows each plan under the store read lock; only matching plans contribute small projections.
    pub fn planner_replay_candidates(&self, tab_id: &str) -> Vec<PlannerReplayCandidate> {
        self.plans_matching(&super::PlanMatch::Planner(tab_id.to_string()))
            .iter()
            .filter_map(|plan| planner_replay_candidate(plan, tab_id))
            .collect()
    }
}

#[cfg(test)]
mod tests;
