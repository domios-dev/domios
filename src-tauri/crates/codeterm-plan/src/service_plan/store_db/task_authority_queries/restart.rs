use super::continuity::CONTINUITY_DIGEST;
use super::*;
use crate::service_plan::{
    CompletedTaskRestartRead, TaskAttemptBindingRead, TaskContinuityDigest,
    TaskRestartAuthorityRead, TaskRestartQuery,
};

const AUTHORITY: &str = "json_object('plan_id',t.plan_id,'task_id',t.task_id,'status',json_extract(t.row_json,'$.status'),
    'assigned_tab_id',t.assigned_tab_id,'owner_epoch',coalesce(json_extract(t.row_json,'$.interruption.owner_epoch'),0),
    'owner_tab_id',json_extract(t.row_json,'$.interruption.owner_tab_id'),
    'owner_session_id',json_extract(t.row_json,'$.interruption.owner_session_id'),
    'attempt',json((SELECT json_object('id',json_extract(a.row_json,'$.id'),'executor_tab_id',a.executor_tab_id,
        'provider',json_extract(a.row_json,'$.provider'),'session_id',a.session_id,'session_binding_nonce',a.session_binding_nonce,
        'acknowledged_at',json_extract(a.row_json,'$.acknowledged_at'),'closed_at',json_extract(a.row_json,'$.closed_at'),
        'retired_at',json_extract(a.row_json,'$.retired_at'),'owner_epoch',coalesce(json_extract(a.row_json,'$.interruption.owner_epoch'),0))
        FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id
            AND json_extract(a.row_json,'$.id')=?2 AND a.executor_tab_id=?3 ORDER BY a.position LIMIT 1)),
    'attempt_id_count',(SELECT count(*) FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id AND json_extract(a.row_json,'$.id')=?2),
    'other_open_attempts',(SELECT count(*) FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id AND json_extract(a.row_json,'$.id')<>?2 AND a.open=1),
    'unclosed_attempts',(SELECT count(*) FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id AND json_extract(a.row_json,'$.closed_at') IS NULL),
    'completed_source_attempts',(SELECT count(*) FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id
        AND a.executor_tab_id=?3 AND json_extract(a.row_json,'$.closed_at') IS NOT NULL
        AND json_extract(a.row_json,'$.retired_at') IS NULL AND json_extract(a.row_json,'$.acknowledged_at') IS NOT NULL
        AND json_extract(a.row_json,'$.provider')=?4 AND a.session_id IS ?5 AND a.session_id IS NOT NULL AND a.session_id<>'' AND a.session_binding_nonce=?6),
    'provider_attempts',(SELECT count(*) FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id AND json_extract(a.row_json,'$.id')=?2 AND json_extract(a.row_json,'$.provider')=?4),
    'runtime_attempts',(SELECT count(*) FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id
        AND json_extract(a.row_json,'$.id')=?2 AND a.executor_tab_id=?3 AND json_extract(a.row_json,'$.provider')=?4
        AND a.session_id IS ?5 AND a.session_binding_nonce=?6 AND json_extract(a.row_json,'$.retired_at') IS NULL))";

impl PlanDatabase {
    fn restart_read(
        &self,
        query: &TaskRestartQuery<'_>,
        fingerprint: bool,
    ) -> Result<(TaskRestartAuthorityRead, Option<TaskContinuityDigest>), PlanError> {
        let connection = self.connection.lock();
        record_query();
        let digest = if fingerprint {
            format!(
                "CASE WHEN json_extract(t.row_json,'$.status')='done' THEN {CONTINUITY_DIGEST} END"
            )
        } else {
            "NULL".into()
        };
        let (raw, digest): (String, Option<Vec<u8>>) = connection
            .query_row(
                &format!("{SELECTED_ACTIVE_TASK} SELECT {AUTHORITY},{digest} FROM selected_task t"),
                params![
                    query.task_ref,
                    query.attempt_id,
                    query.executor_tab_id,
                    query.provider,
                    query.session_id,
                    query.nonce
                ],
                |row| Ok((column(row, 0)?, column(row, 1)?)),
            )
            .optional()
            .map_err(db_error)?
            .ok_or(PlanError::TaskNotFound)?;
        let authority: TaskRestartAuthorityRead = decode("task restart authority", &raw)?;
        Ok((
            authority,
            digest.map(TaskContinuityDigest::from_bytes).transpose()?,
        ))
    }

    pub(in crate::service_plan) fn task_restart_authority_projection(
        &self,
        query: &TaskRestartQuery<'_>,
    ) -> Result<TaskRestartAuthorityRead, PlanError> {
        self.restart_read(query, false).map(|(read, _)| read)
    }

    pub(in crate::service_plan) fn task_completed_restart_projection(
        &self,
        query: &TaskRestartQuery<'_>,
    ) -> Result<Option<CompletedTaskRestartRead>, PlanError> {
        let (authority, digest) = self.restart_read(query, true)?;
        if authority.status != crate::plan::TaskStatus::Done {
            return Ok(None);
        }
        Ok(Some(CompletedTaskRestartRead {
            authority,
            digest: digest.ok_or_else(|| {
                PlanError::Io("completed task continuity digest is missing".into())
            })?,
        }))
    }

    pub(in crate::service_plan) fn runtime_attempt_bindings_projection(
        &self,
        tab: &str,
        current_only: bool,
    ) -> Result<Vec<TaskAttemptBindingRead>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let mut statement = connection.prepare_cached(
            "WITH ranked AS (SELECT plan_id,task_id,json_extract(row_json,'$.id') AS attempt_id,
                json_extract(row_json,'$.retired_at') AS retired,
                row_number() OVER(PARTITION BY plan_id,task_id ORDER BY position DESC) AS rank
                FROM plan_task_attempts WHERE executor_tab_id=?1 AND json_extract(row_json,'$.closed_at') IS NULL)
             SELECT p.id,t.task_id,a.attempt_id FROM ranked a JOIN plan_tasks t USING(plan_id,task_id) JOIN plans p ON p.id=t.plan_id
             WHERE a.rank=1 AND (?2=0 OR a.retired IS NULL)
             ORDER BY p.updated_at DESC,p.id,t.position LIMIT 2"
        ).map_err(db_error)?;
        let rows = statement
            .query_map(params![tab, current_only], |row| {
                Ok(TaskAttemptBindingRead {
                    plan_id: column(row, 0)?,
                    task_id: column(row, 1)?,
                    attempt_id: column(row, 2)?,
                })
            })
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }
}
