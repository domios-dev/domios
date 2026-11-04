use super::{column, decode, record_query};
use crate::plan::{TaskFiles, TaskLaneMetadata, TaskStatus};
use crate::service_plan::store_db::{db_error, PlanDatabase};
use crate::service_plan::{PlanError, PlanStore};
use rusqlite::OptionalExtension;

#[derive(Debug, serde::Deserialize, serde::Serialize, PartialEq)]
pub struct ProviderSwitchTaskContextRead {
    pub plan_id: String,
    pub plan_title: String,
    pub task: ProviderSwitchTaskRead,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, PartialEq)]
pub struct ProviderSwitchTaskRead {
    pub id: String,
    pub name: String,
    pub status: TaskStatus,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correctness: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_plan: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub acceptance_criteria: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role_guidance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executor_role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub personality: Option<String>,
    pub files: TaskFiles,
    pub lane: TaskLaneMetadata,
}

impl PlanStore {
    pub fn provider_switch_task_context_projection(
        &self,
        task: &str,
    ) -> Result<Option<ProviderSwitchTaskContextRead>, PlanError> {
        self.database.provider_switch_task_context_projection(task)
    }
}

impl PlanDatabase {
    fn provider_switch_task_context_projection(
        &self,
        task: &str,
    ) -> Result<Option<ProviderSwitchTaskContextRead>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let raw: Option<String>=connection.query_row(&format!(
            "{selected} SELECT json_object('plan_id',p.id,'plan_title',p.title,'task',json_object(
                'id',t.task_id,'name',json_extract(t.row_json,'$.name'),'status',json_extract(t.row_json,'$.status'),
                'body',CASE WHEN json_type(t.row_json,'$.body') IS NULL THEN '' ELSE json_extract(t.row_json,'$.body') END,
                'intent',json_extract(t.row_json,'$.intent'),'correctness',json_extract(t.row_json,'$.correctness'),
                'test_plan',json_extract(t.row_json,'$.test_plan'),
                'acceptance_criteria',json(CASE WHEN json_type(t.row_json,'$.acceptance_criteria') IS NULL THEN '[]' ELSE t.row_json->'$.acceptance_criteria' END),
                'role_guidance',json_extract(t.row_json,'$.role_guidance'),'executor_role',json_extract(t.row_json,'$.executor_role'),
                'personality',json_extract(t.row_json,'$.personality'),
                'files',json(CASE WHEN json_type(t.row_json,'$.files') IS NULL THEN '{{}}' ELSE t.row_json->'$.files' END),
                'lane',json(CASE WHEN json_type(t.row_json,'$.lane') IS NULL THEN '{{}}' ELSE t.row_json->'$.lane' END)))
             FROM selected_task t JOIN plans p ON p.id=t.plan_id",
            selected=super::super::task_authority_queries::SELECTED_ACTIVE_TASK
        ),[task],|row|column(row,0)).optional().map_err(db_error)?;
        raw.map(|raw| decode(task, &raw)).transpose()
    }
}

#[cfg(test)]
mod tests;
