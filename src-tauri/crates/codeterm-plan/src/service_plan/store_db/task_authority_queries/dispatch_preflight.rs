use super::*;
use crate::service_plan::TaskDispatchPreflightRead;

impl PlanDatabase {
    pub(in crate::service_plan) fn task_dispatch_preflight_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<TaskDispatchPreflightRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (exists,raw):(bool,Option<String>)=connection.query_row(
            &format!("{SELECTED_TASK_IN_PLAN} SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),
                (SELECT json_object('plan_id',p.id,'project_path',json_extract(p.header_json,'$.project_path'),
                    'canonical_project_path',CASE WHEN json_type(p.header_json,'$.canonical_project_path') IS NULL THEN '' ELSE json_extract(p.header_json,'$.canonical_project_path') END,
                    'owner_tab_id',json_extract(p.header_json,'$.owner.tab_id'),'model',json_extract(t.row_json,'$.model'),
                    'lane',json(CASE WHEN json_type(t.row_json,'$.lane') IS NULL THEN '{{}}' ELSE t.row_json->'$.lane' END),
                    'files',json(CASE WHEN json_type(t.row_json,'$.files') IS NULL THEN '{{}}' ELSE t.row_json->'$.files' END),
                    'kind',json_extract(t.row_json,'$.type'),
                    'build_breaking',json(CASE WHEN EXISTS(SELECT 1 FROM json_each(t.row_json,'$.tags') WHERE value='build-breaking') THEN 'true' ELSE 'false' END),
                    'id',t.task_id,'name',json_extract(t.row_json,'$.name'),'status',json_extract(t.row_json,'$.status'),
                    'pause_reason',json_extract(t.row_json,'$.pause_reason'),'status_label',json_extract(t.row_json,'$.status_label'),
                    'execution_intent',CASE WHEN json_type(t.row_json,'$.execution_intent') IS NULL THEN 'repository_mutation' ELSE json_extract(t.row_json,'$.execution_intent') END,
                    'is_unplanned_work',json(CASE WHEN json_extract(t.row_json,'$.type')='unplanned'
                        AND EXISTS(SELECT 1 FROM json_each(t.row_json,'$.tags') WHERE value='unplanned') THEN 'true' ELSE 'false' END))
                 FROM selected_task t JOIN plans p ON p.id=t.plan_id WHERE p.status='active'
                    AND CASE WHEN json_type(t.row_json,'$.type')='text' THEN 1 ELSE json('invalid task type') END
                    AND CASE WHEN json_type(t.row_json,'$.tags') IS NULL THEN 1
                        WHEN json_type(t.row_json,'$.tags')='array' AND NOT EXISTS(SELECT 1 FROM json_each(t.row_json,'$.tags') WHERE type<>'text') THEN 1
                        ELSE json('invalid task tags') END)"),
            params![plan,task],|row|Ok((column(row,0)?,column(row,1)?)),
        ).map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        decode(plan, &raw.ok_or(PlanError::TaskNotFound)?)
    }
}

pub(super) fn register(connection: &rusqlite::Connection) -> Result<(), PlanError> {
    connection
        .create_scalar_function(
            "domios_dispatch_checkout_path",
            1,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8
                | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
            |context| Ok(codeterm_paths::normalize_cwd(&context.get::<String>(0)?)),
        )
        .map_err(db_error)
}

impl PlanDatabase {
    pub(in crate::service_plan) fn dispatch_checkout_occupant_projection(
        &self,
        plan: &str,
        task: &str,
        checkout: &str,
        live_tabs: &[String],
    ) -> Result<Option<crate::service_plan::DispatchCheckoutOccupantRead>, PlanError> {
        let statuses = serde_json::to_string(live_tabs)
            .map_err(|error| PlanError::InvalidPlanText(error.to_string()))?;
        let connection = self.connection.lock();
        record_query();
        let (exists,raw):(bool,Option<String>)=connection.query_row("WITH reported AS (
                SELECT a.task_id,json_extract(a.row_json,'$.id') AS attempt_id,
                    CASE WHEN json_type(a.row_json,'$.executor_tab_id')='text' THEN json_extract(a.row_json,'$.executor_tab_id') ELSE json('invalid reported executor') END AS executor_tab_id,
                    row_number() OVER (PARTITION BY a.task_id,json_extract(a.row_json,'$.id') ORDER BY a.position DESC) AS rank
                FROM plan_task_attempts a WHERE a.plan_id=?1 AND a.task_id=?2
                    AND json_extract(a.row_json,'$.closed_at') IS NULL AND json_extract(a.row_json,'$.competition_group_id') IS NULL
                    AND json_extract(a.row_json,'$.reported_at') IS NOT NULL), checkout_root AS MATERIALIZED (
                SELECT id,CASE WHEN trim(coalesce(json_extract(header_json,'$.canonical_project_path'),''),?5)<>''
                    THEN json_extract(header_json,'$.canonical_project_path') ELSE json_extract(header_json,'$.project_path') END AS root
                FROM plans WHERE id=?1)
            SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),
            (SELECT json_object('task_id',t.task_id,'attempt_id',json_extract(a.row_json,'$.id'),'executor_tab_id',r.executor_tab_id)
             FROM checkout_root p JOIN plan_tasks t ON t.plan_id=p.id JOIN plan_task_attempts a ON a.plan_id=t.plan_id AND a.task_id=t.task_id
             LEFT JOIN reported r ON r.task_id=t.task_id AND r.attempt_id=json_extract(a.row_json,'$.id') AND r.rank=1
             WHERE p.id=?1 AND json_extract(a.row_json,'$.closed_at') IS NULL
                AND (r.rank IS NULL OR EXISTS(SELECT 1 FROM json_each(?4) WHERE value=r.executor_tab_id))
                AND domios_dispatch_checkout_path(?3)=domios_dispatch_checkout_path(
                    CASE WHEN trim(coalesce(json_extract(a.row_json,'$.worktree_path'),''),?5)<>''
                        THEN json_extract(a.row_json,'$.worktree_path')
                        ELSE p.root END)
             ORDER BY t.position,a.position LIMIT 1)",
             params![plan,task,checkout,statuses,super::super::status_queries::WHITESPACE],|row|Ok((column(row,0)?,column(row,1)?))).map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        raw.map(|raw| decode(plan, &raw)).transpose()
    }
}

impl PlanDatabase {
    pub(in crate::service_plan) fn remote_lane_integration_projection(
        &self,
        plan: &str,
        task: &str,
        attempt: &str,
    ) -> Result<crate::service_plan::RemoteLaneIntegrationRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (exists,raw):(bool,Option<String>)=connection.query_row(&format!("{SELECTED_TASK_IN_PLAN}
            SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),
                (SELECT json_object('canonical_path',CASE WHEN trim(coalesce(json_extract(p.header_json,'$.canonical_project_path'),''),?4)=''
                        THEN json_extract(p.header_json,'$.project_path') ELSE json_extract(p.header_json,'$.canonical_project_path') END,
                    'task_destination',json_extract(t.row_json,'$.lane.destination'),'destination',json_extract(a.row_json,'$.destination'),
                    'judge_verdict',json_extract(a.row_json,'$.judge_verdict'),'report_sha',json_extract(a.row_json,'$.report.sha'),
                    'remote_dispatch_state',json_extract(a.row_json,'$.remote_dispatch_state'),'remote_lane_tip',json_extract(a.row_json,'$.remote_lane_tip'),
                    'remote_merge_commit',json_extract(a.row_json,'$.remote_merge_commit'),'mesh_peer',json_extract(a.row_json,'$.mesh_peer'),
                    'branch',json_extract(a.row_json,'$.branch'),'base',json_extract(a.row_json,'$.base'))
                 FROM selected_task t JOIN plans p ON p.id=t.plan_id JOIN plan_task_attempts a ON a.plan_id=t.plan_id AND a.task_id=t.task_id
                 WHERE p.status='active' AND json_extract(a.row_json,'$.id')=?3 ORDER BY a.position LIMIT 1)"),
            params![plan,task,attempt,super::super::status_queries::WHITESPACE], |row| Ok((column(row,0)?,column(row,1)?))).map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        decode(plan, &raw.ok_or(PlanError::TaskNotFound)?)
    }
}
