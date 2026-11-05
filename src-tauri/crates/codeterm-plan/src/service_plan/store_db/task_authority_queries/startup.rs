use super::*;
use crate::plan::TaskStatus;
use crate::service_plan::task_authority_projection::startup::TaskStartupRead;
use crate::service_plan::DispatchStartupOwner;

const ATTEMPT: &str = "json_object('id',json_extract(a.row_json,'$.id'),'executor',json_extract(a.row_json,'$.executor_tab_id'),
    'title',json_extract(a.row_json,'$.executor_title'),'session',json_extract(a.row_json,'$.session_id'),
    'group',json_extract(a.row_json,'$.competition_group_id'),
    'acknowledged_at',json_extract(a.row_json,'$.acknowledged_at'),'reported_at',json_extract(a.row_json,'$.reported_at'),
    'has_report',json(CASE WHEN json_extract(a.row_json,'$.report') IS NOT NULL THEN 'true' ELSE 'false' END))";

impl PlanDatabase {
    pub(in crate::service_plan) fn worker_startup_state_projection(
        &self,
        plan_id: &str,
        task_ref: &str,
        tab: &str,
        attempt: &str,
    ) -> Result<(TaskStartupRead, bool), PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (exists, active, raw): (bool, bool, Option<String>) = connection.query_row(
            &format!("{SELECTED_TASK_IN_PLAN} SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),
                EXISTS(SELECT 1 FROM plans WHERE id=?1 AND status='active'),
                (SELECT json_object('status',json_extract(t.row_json,'$.status'),'assigned_tab',t.assigned_tab_id,
                    'first_matching',json((SELECT {ATTEMPT} FROM plan_task_attempts a
                        WHERE a.plan_id=?1 AND a.task_id=t.task_id AND a.open=1
                            AND a.executor_tab_id=?3 AND json_extract(a.row_json,'$.id')=?4
                        ORDER BY a.position LIMIT 1)),
                    'latest_unclosed',json((SELECT json_object('id',json_extract(a.row_json,'$.id'))
                        FROM plan_task_attempts a WHERE a.plan_id=?1 AND a.task_id=t.task_id
                            AND json_extract(a.row_json,'$.closed_at') IS NULL ORDER BY a.position DESC LIMIT 1)),
                    'latest_group',json((SELECT json_object('group',json_extract(a.row_json,'$.competition_group_id'))
                        FROM plan_task_attempts a WHERE a.plan_id=?1 AND a.task_id=t.task_id
                            AND json_extract(a.row_json,'$.closed_at') IS NULL
                            AND json_extract(a.row_json,'$.competition_group_id') IS NOT NULL ORDER BY a.position DESC LIMIT 1)))
                 FROM selected_task t)"),
            params![plan_id,task_ref,tab,attempt], |row| Ok((column(row,0)?,column(row,1)?,column(row,2)?)),
        ).map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        Ok((
            decode(plan_id, &raw.ok_or(PlanError::TaskNotFound)?)?,
            active,
        ))
    }

    pub(in crate::service_plan) fn task_status_projection(
        &self,
        plan_id: &str,
        reference: &str,
    ) -> Result<TaskStatus, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (status, exists): (Option<String>, bool) = connection
            .query_row(
                &format!(
                    "{SELECTED_TASK_IN_PLAN} SELECT
                (SELECT json_quote(json_extract(row_json,'$.status')) FROM selected_task),
                EXISTS(SELECT 1 FROM plans WHERE id=?1)"
                ),
                params![plan_id, reference],
                |row| Ok((column(row, 0)?, column(row, 1)?)),
            )
            .map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        decode(plan_id, &status.ok_or(PlanError::TaskNotFound)?)
    }

    pub(in crate::service_plan) fn dispatch_startup_owner_projection(
        &self,
        plan_id: &str,
        reference: &str,
    ) -> Result<Option<DispatchStartupOwner>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let raw: Option<String> = connection.query_row(
            &format!("{SELECTED_TASK_IN_PLAN},
                latest_open AS (
                    SELECT a.row_json,a.executor_tab_id,{ATTEMPT} AS data FROM plan_task_attempts a
                    WHERE a.plan_id=?1 AND a.task_id=(SELECT task_id FROM selected_task) AND a.open=1
                    ORDER BY a.position DESC LIMIT 1
                ), first_matching AS (
                    SELECT {ATTEMPT} AS data FROM plan_task_attempts a
                    WHERE a.plan_id=?1 AND a.task_id=(SELECT task_id FROM selected_task) AND a.open=1
                      AND json_extract(a.row_json,'$.id')=(SELECT json_extract(row_json,'$.id') FROM latest_open)
                      AND a.executor_tab_id=(SELECT executor_tab_id FROM latest_open)
                    ORDER BY a.position LIMIT 1
                )
                SELECT json_object('status',json_extract(t.row_json,'$.status'),'assigned_tab',t.assigned_tab_id,
                    'latest_open',json((SELECT data FROM latest_open)),
                    'first_matching',json((SELECT data FROM first_matching)),
                    'latest_unclosed',json((SELECT json_object('id',json_extract(a.row_json,'$.id'))
                        FROM plan_task_attempts a WHERE a.plan_id=?1 AND a.task_id=t.task_id
                            AND json_extract(a.row_json,'$.closed_at') IS NULL ORDER BY a.position DESC LIMIT 1)),
                    'latest_group',json((SELECT json_object('group',json_extract(a.row_json,'$.competition_group_id'))
                        FROM plan_task_attempts a WHERE a.plan_id=?1 AND a.task_id=t.task_id
                            AND json_extract(a.row_json,'$.closed_at') IS NULL
                            AND json_extract(a.row_json,'$.competition_group_id') IS NOT NULL ORDER BY a.position DESC LIMIT 1)))
                FROM selected_task t JOIN plans p ON p.id=t.plan_id"),
            params![plan_id,reference], |row| column(row,0),
        ).optional().map_err(db_error)?;
        raw.map(|raw| decode::<TaskStartupRead>(plan_id, &raw).map(|read| read.owner()))
            .transpose()
            .map(Option::flatten)
    }
}
