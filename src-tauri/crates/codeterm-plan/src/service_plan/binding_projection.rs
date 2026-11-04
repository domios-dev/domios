use super::{PlanError, PlanStore};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TaskBindingRead {
    pub plan_id: String,
    pub task_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KpiPlanContextRead {
    pub plan_id: Option<String>,
    pub task_id: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct RestoredPlanBindingRead {
    pub plan_id: String,
    pub owns_plan: bool,
    pub planner_binding: Option<crate::plan::PlannerRef>,
}

impl PlanStore {
    pub fn restored_plan_bindings_projection(
        &self,
        tab: &str,
    ) -> Result<Vec<RestoredPlanBindingRead>, PlanError> {
        self.database.restored_plan_bindings_projection(tab)
    }

    pub fn coordinator_binding_projection(
        &self,
        tab: &str,
        probe: &crate::plan::reconcile::TabProbe,
    ) -> Result<bool, PlanError> {
        self.database.coordinator_binding_projection(
            tab,
            probe.agent_session_id.as_deref(),
            probe.planner_routable_for(probe.agent_session_id.as_deref()),
        )
    }

    pub fn kpi_plan_context_projection(&self, tab: &str) -> Result<KpiPlanContextRead, PlanError> {
        self.database.kpi_plan_context_projection(tab)
    }
    pub fn assigned_task_binding_projection(
        &self,
        tab: &str,
    ) -> Result<Option<TaskBindingRead>, PlanError> {
        self.database.assigned_task_binding_projection(tab, false)
    }

    pub fn report_pending_task_binding_projection(
        &self,
        tab: &str,
    ) -> Result<Option<TaskBindingRead>, PlanError> {
        self.database.assigned_task_binding_projection(tab, true)
    }

    pub fn planner_binding_projection(&self, tab: &str) -> Result<Option<String>, PlanError> {
        self.database.planner_binding_projection(tab)
    }

    pub fn taskless_plan_bindings_projection(&self, tab: &str) -> Result<Vec<String>, PlanError> {
        self.database.taskless_plan_bindings_projection(tab)
    }
}
