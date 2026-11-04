use super::read_projections::{column, decode, record_query, SECTION_ROWS};
use super::{db_error, PlanDatabase};
use crate::service_plan::{ui_projections::ReferenceProjection, PlanError};
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
        task_refs: &[String],
        section_refs: &[String],
    ) -> Result<ReferenceProjection, PlanError> {
        let connection = self.connection.lock();
        let tasks = serde_json::to_string(task_refs).map_err(|e| PlanError::Io(e.to_string()))?;
        let sections =
            serde_json::to_string(section_refs).map_err(|e| PlanError::Io(e.to_string()))?;
        record_query();
        let raw: String = connection.query_row(&format!(
            "WITH requested AS (SELECT key AS request,value FROM json_each(?2)),
             exact AS MATERIALIZED (SELECT r.request,t.task_id FROM requested r JOIN plan_tasks t ON t.plan_id=?1 AND t.task_id=r.value),
             named_requests AS MATERIALIZED (SELECT r.request,r.value FROM requested r WHERE NOT EXISTS(SELECT 1 FROM exact e WHERE e.request=r.request)),
             named_tasks AS MATERIALIZED (SELECT task_id,json_extract(row_json,'$.handle') AS handle,
                json_extract(row_json,'$.key') AS key FROM plan_tasks WHERE plan_id=?1
                AND EXISTS(SELECT 1 FROM named_requests)),
             ranked AS MATERIALIZED (SELECT request,task_id,0 AS tier FROM exact UNION ALL SELECT r.request,t.task_id,
                domios_task_ref_rank(t.task_id,t.handle,t.key,r.value) AS tier
                FROM named_requests r CROSS JOIN named_tasks t),
             best AS (SELECT request,min(tier) AS tier FROM ranked WHERE tier IN (0,1) GROUP BY request),
             chosen AS (SELECT r.request,r.task_id FROM ranked r JOIN best b USING(request,tier)),
             requested_tasks AS (SELECT request,min(task_id) AS task_id FROM chosen GROUP BY request HAVING count(*)=1)
             SELECT json_object('title',p.title,'status',p.status,
             'owner',json(coalesce(json_extract(p.header_json,'$.owner'),'{{}}')),
             'canonical_project_path',coalesce(nullif(trim(json_extract(p.header_json,'$.canonical_project_path')),''),p.project_path),
             'task_refs',json((SELECT json_group_object(r.value,chosen.task_id) FROM requested r LEFT JOIN requested_tasks chosen USING(request))),
             'tasks',json((SELECT json_group_array(json_object('id',t.task_id,'name',json_extract(t.row_json,'$.name'),
               'key',json_extract(t.row_json,'$.key'),'handle',json_extract(t.row_json,'$.handle')))
               FROM plan_tasks t WHERE t.plan_id=p.id AND t.task_id IN (SELECT task_id FROM requested_tasks))),
             'sections',json((SELECT json_group_array(json_object('id',id,'title',title,'handle',handle))
               FROM ({SECTION_ROWS}) selected_sections WHERE EXISTS(SELECT 1 FROM json_each(?3) r
               WHERE selected_sections.id=r.value OR selected_sections.handle=r.value))))
             FROM plans p WHERE p.id=?1"), params![plan_id,tasks,sections], |row| column(row,0))
            .optional().map_err(db_error)?.ok_or(PlanError::NotFound)?;
        decode(plan_id, &raw)
    }
}
