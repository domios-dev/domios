use super::read_projections::{column, decode, record_query, SECTION_ROWS};
use super::{db_error, PlanDatabase};
use crate::plan::{Plan, Section, Task};
use crate::service_plan::{ui_projections::PlanViewProjection, PlanError};
use rusqlite::{params, OptionalExtension};

impl PlanDatabase {
    pub(in crate::service_plan) fn task_reference(
        &self,
        reference: &str,
        project: Option<&str>,
        team: Option<&str>,
    ) -> Result<serde_json::Value, PlanError> {
        let mut guard = self.connection.lock();
        let transaction = guard.transaction().map_err(db_error)?;
        record_query();
        let mut statement=transaction.prepare_cached("SELECT t.task_id COLLATE NOCASE = ?1 AS exact,
            json_object('plan_id',p.id,'task_id',t.task_id,'name',json_extract(t.row_json,'$.name'),
            'key',coalesce(json_extract(t.row_json,'$.key'),upper(CASE WHEN substr(t.task_id,1,5)='task_' THEN substr(t.task_id,6) ELSE t.task_id END)),
            'team_id',p.team_id,'revision',CAST(coalesce(json_extract(p.header_json,'$._revision'),0) AS TEXT))
            FROM plan_tasks t JOIN plans p ON p.id=t.plan_id WHERE t.task_id COLLATE NOCASE = ?1 OR (
                ?2 IS NOT NULL AND p.project_path=?2 AND coalesce(p.team_id,'')=coalesce(?3,'') AND p.status='active'
                AND coalesce(json_extract(p.header_json,'$.mode'),'planning')<>'complete'
                AND json_extract(t.row_json,'$.key') COLLATE NOCASE = ?1)
            ORDER BY exact DESC,p.id,t.position LIMIT 2").map_err(db_error)?;
        let rows = statement
            .query_map(params![reference, project, team], |row| {
                Ok((row.get::<_, bool>(0)?, column::<String>(row, 1)?))
            })
            .map_err(db_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_error)?;
        drop(statement);
        let first = rows.first().ok_or(PlanError::TaskNotFound)?;
        if rows.len() > 1 && rows[1].0 == first.0 {
            return Err(PlanError::Validation("Task reference is ambiguous".into()));
        }
        let value = decode("task-reference", &first.1)?;
        transaction.commit().map_err(db_error)?;
        Ok(value)
    }

    pub(in crate::service_plan) fn reference_projection(
        &self,
        plan_id: &str,
        task_ref: Option<&str>,
        section_ref: Option<&str>,
    ) -> Result<Plan, PlanError> {
        let mut guard = self.connection.lock();
        let transaction = guard.transaction().map_err(db_error)?;
        let value =
            super::ui_projections::projection(&transaction, plan_id, PlanViewProjection::Summary)?;
        let mut plan: Plan =
            serde_json::from_value(value).map_err(|error| PlanError::Io(error.to_string()))?;
        if let Some(reference) = task_ref {
            match super::task_page_queries::resolve_detail_task(&transaction, plan_id, reference) {
                Ok(id) => {
                    record_query();
                    let raw: String = transaction.query_row(
                        "SELECT json_object('id',task_id,'name',json_extract(row_json,'$.name'),'type',json_extract(row_json,'$.type'),'status',status,'key',json_extract(row_json,'$.key'),'handle',json_extract(row_json,'$.handle')) FROM plan_tasks WHERE plan_id=?1 AND task_id=?2",
                        params![plan_id,id], |row| column(row,0)).map_err(db_error)?;
                    plan.tasks.push(decode::<Task>(plan_id, &raw)?);
                }
                Err(PlanError::TaskNotFound) => {}
                Err(error) => return Err(error),
            }
        }
        if let Some(reference) = section_ref {
            record_query();
            let raw: Option<String> = transaction.query_row(
                &format!("SELECT json_object('id',id,'title',title,'handle',handle) FROM ({SECTION_ROWS}) WHERE id=?2 OR handle=?2 ORDER BY position,traversal LIMIT 1"),
                params![plan_id,reference], |row| column(row,0)).optional().map_err(db_error)?;
            if let Some(raw) = raw {
                plan.sections.push(decode::<Section>(plan_id, &raw)?);
            }
        }
        transaction.commit().map_err(db_error)?;
        Ok(plan)
    }
}
