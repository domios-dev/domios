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

impl PlanStore {
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
