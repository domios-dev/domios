use super::{column, decode, record_query};
use crate::plan::{
    ExecutionIntent, Milestone, TaskCompetitionMetadata, TaskExecutionAttempt, TaskLaneMetadata,
    TaskStatus,
};
use crate::service_plan::store_db::{db_error, PlanDatabase};
use crate::service_plan::{PlanError, PlanStore};
use rusqlite::params;

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct TaskDraftPromptRead {
    pub id: String,
    pub name: String,
    pub status: TaskStatus,
    pub body: String,
    pub execution_intent: ExecutionIntent,
    pub model: Option<String>,
    pub speed: Option<String>,
    pub personality: Option<String>,
    pub role_guidance: Option<String>,
    pub executor_role: Option<String>,
    pub intent: Option<String>,
    pub correctness: Option<String>,
    pub test_plan: Option<String>,
    pub acceptance_criteria: Vec<String>,
    pub lane: TaskLaneMetadata,
    pub competition: Option<TaskCompetitionMetadata>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub milestones: Vec<Milestone>,
}

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct TaskDraftRead {
    pub plan_id: String,
    pub project_path: String,
    pub owner_tab_id: Option<String>,
    pub task: TaskDraftPromptRead,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<TaskExecutionAttempt>,
}

impl PlanStore {
    pub fn task_draft_projection(
        &self,
        plan: &str,
        task: &str,
        source: Option<&str>,
    ) -> Result<TaskDraftRead, PlanError> {
        self.database.task_draft_projection(plan, task, source)
    }
}

impl PlanDatabase {
    fn task_draft_projection(
        &self,
        plan: &str,
        task: &str,
        source: Option<&str>,
    ) -> Result<TaskDraftRead, PlanError> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction().map_err(db_error)?;
        record_query();
        let (exists,raw):(bool,Option<String>)=tx.query_row(&format!(
            "{selected} SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),(SELECT json_object(
                'plan_id',p.id,'project_path',p.project_path,'owner_tab_id',json_extract(p.header_json,'$.owner.tab_id'),
                'task',json_object('id',t.task_id,'name',json_extract(t.row_json,'$.name'),'status',json_extract(t.row_json,'$.status'),
                    'body',CASE WHEN json_type(t.row_json,'$.body') IS NULL THEN '' ELSE json_extract(t.row_json,'$.body') END,
                    'execution_intent',CASE WHEN json_type(t.row_json,'$.interruption.execution_intent') IS NULL THEN 'unspecified' ELSE json_extract(t.row_json,'$.interruption.execution_intent') END,
                    'model',json_extract(t.row_json,'$.model'),'speed',json_extract(t.row_json,'$.speed'),
                    'personality',json_extract(t.row_json,'$.personality'),'role_guidance',json_extract(t.row_json,'$.role_guidance'),
                    'executor_role',json_extract(t.row_json,'$.executor_role'),'intent',json_extract(t.row_json,'$.intent'),
                    'correctness',json_extract(t.row_json,'$.correctness'),'test_plan',json_extract(t.row_json,'$.test_plan'),
                    'acceptance_criteria',json(CASE WHEN json_type(t.row_json,'$.acceptance_criteria') IS NULL THEN '[]' ELSE t.row_json->'$.acceptance_criteria' END),
                    'competition',json(t.row_json->'$.competition'),
                    'lane',json(CASE WHEN json_type(t.row_json,'$.lane') IS NULL THEN '{{}}' ELSE t.row_json->'$.lane' END)))
                FROM selected_task t JOIN plans p ON p.id=t.plan_id)",
            selected=super::super::task_authority_queries::SELECTED_TASK_IN_PLAN
        ),params![plan,task],|row|Ok((column(row,0)?,column(row,1)?))).map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        let mut read: TaskDraftRead = decode(plan, &raw.ok_or(PlanError::TaskNotFound)?)?;
        if let Some(source) = source {
            crate::service_plan::task_dispatch::task_respawn_status(&read.task.status)?;
            record_query();
            use rusqlite::OptionalExtension;
            let raw:Option<String>=tx.query_row(
                "SELECT row_json FROM plan_task_attempts WHERE plan_id=?1 AND task_id=?2 AND json_extract(row_json,'$.id')=?3 ORDER BY position LIMIT 1",
                params![plan,read.task.id,source],|row|column(row,0)
            ).optional().map_err(db_error)?;
            read.source = Some(decode(plan, &raw.ok_or(PlanError::TaskNotFound)?)?);
        }
        record_query();
        let mut statement=tx.prepare(
            "WITH context AS (SELECT seq FROM plan_task_milestones WHERE plan_id=?1 AND task_id=?2
                AND domios_worker_stride_rank(json_extract(row_json,'$.kind')) IS NOT NULL AND trim(coalesce(json_extract(row_json,'$.text'),''),?4)<>''
                ORDER BY domios_worker_stride_rank(json_extract(row_json,'$.kind')),domios_u64_order(row_json->'$.ts') DESC,
                    json_extract(row_json,'$.id') DESC,seq LIMIT ?5),
             report AS (SELECT seq FROM plan_task_milestones WHERE plan_id=?1 AND task_id=?2
                AND json_extract(row_json,'$.from.tab_id')=?3 ORDER BY domios_u64_order(row_json->'$.ts') DESC,seq DESC LIMIT 1),
             chosen AS (SELECT seq FROM context UNION SELECT seq FROM report)
             SELECT json_object('id',json_extract(m.row_json,'$.id'),'from',json_object('tab_id',json_extract(m.row_json,'$.from.tab_id')),
                'kind',json_extract(m.row_json,'$.kind'),'text',coalesce(json_extract(m.row_json,'$.text'),''),'ts',json(m.row_json->'$.ts'))
             FROM plan_task_milestones m JOIN chosen USING(seq) WHERE plan_id=?1 AND task_id=?2 ORDER BY seq"
        ).map_err(db_error)?;
        let rows = statement
            .query_map(
                params![
                    plan,
                    read.task.id,
                    read.source
                        .as_ref()
                        .map(|source| source.executor_tab_id.as_str()),
                    super::super::status_queries::WHITESPACE,
                    crate::service_plan::stride_context::WORKER_STRIDE_LIMIT
                ],
                |row| column::<String>(row, 0),
            )
            .map_err(db_error)?;
        read.task.milestones = rows
            .map(|row| decode(plan, &row.map_err(db_error)?))
            .collect::<Result<_, _>>()?;
        drop(statement);
        tx.commit().map_err(db_error)?;
        Ok(read)
    }
}

#[cfg(test)]
mod tests;
