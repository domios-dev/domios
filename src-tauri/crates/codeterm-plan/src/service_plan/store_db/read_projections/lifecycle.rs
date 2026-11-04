use super::{column, decode, record_query};
use crate::plan::{OrchestratorRef, Plan, PlanMode, PlanPauseOperation, PlannerRef};
use crate::service_plan::store_db::{db_error, PlanDatabase};
use crate::service_plan::{PlanError, PlanStore};
use rusqlite::OptionalExtension;

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct PlanLifecycleRead {
    pub id: String,
    pub title: String,
    pub mode: PlanMode,
    pub owner: OrchestratorRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planner: Option<PlannerRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paused_from_mode: Option<PlanMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<PlanPauseOperation>,
}

impl From<&Plan> for PlanLifecycleRead {
    fn from(plan: &Plan) -> Self {
        Self {
            id: plan.id.clone(),
            title: plan.title.clone(),
            mode: plan.mode.clone(),
            owner: plan.owner.clone(),
            planner: plan.planner.clone(),
            team_id: plan.team_id.clone(),
            paused_from_mode: plan.paused_from_mode.clone(),
            operation: crate::service_plan::pause_operation::open_pause_operation(plan).cloned(),
        }
    }
}

impl PlanDatabase {
    pub(in crate::service_plan) fn plan_lifecycle_projection(
        &self,
        id: &str,
    ) -> Result<PlanLifecycleRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let raw: String = connection.query_row(
            "SELECT json_object('id',id,'title',title,'mode',CASE WHEN json_type(header_json,'$.mode') IS NULL THEN 'planning' ELSE json_extract(header_json,'$.mode') END,
                'owner',json_extract(header_json,'$.owner'),'planner',json_extract(header_json,'$.planner'),
                'team_id',team_id,'paused_from_mode',json_extract(header_json,'$.paused_from_mode'),
                'operation',CASE WHEN json_extract(header_json,'$.mode')='paused'
                    AND json_extract(header_json,'$.pause_operations[#-1].resume') IS NULL
                    THEN json_extract(header_json,'$.pause_operations[#-1]') END)
             FROM plans WHERE id=?1", [id],|row| column(row, 0),
        ).optional().map_err(db_error)?.ok_or(PlanError::NotFound)?;
        decode(id, &raw)
    }
}

impl PlanStore {
    pub fn plan_lifecycle_projection(&self, id: &str) -> Result<PlanLifecycleRead, PlanError> {
        self.database.plan_lifecycle_projection(id)
    }
}
