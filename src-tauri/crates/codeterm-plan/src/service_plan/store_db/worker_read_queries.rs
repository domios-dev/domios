use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::plan::{Milestone, TaskExecutionAttempt};
use crate::service_plan::planner_replay::PlannerReplayCandidate;
use crate::service_plan::task_attempts::read::TaskAttemptRead;
use crate::service_plan::PlanError;
use rusqlite::params;

impl PlanDatabase {
    pub(in crate::service_plan) fn planner_replay_rows(
        &self,
        tab: &str,
    ) -> Result<Vec<PlannerReplayCandidate>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let mut statement=connection.prepare_cached(
            "SELECT p.id,json_object('plan_id',p.id,'session_id',json_extract(header_json,'$.planner.session_id'),
                'planner',json_extract(header_json,'$.planner'),'owner',json_extract(header_json,'$.owner'))
             FROM plans p WHERE json_extract(header_json,'$.planner.kind')='tab'
                AND json_extract(header_json,'$.planner.tab_id')=?1 AND json_extract(header_json,'$.planner.session_id') IS NOT NULL
                AND EXISTS(SELECT 1 FROM json_each(p.header_json,'$.planner_requests') r WHERE domios_request_pending(json_extract(r.value,'$.state')))
             ORDER BY p.updated_at DESC,p.id"
        ).map_err(db_error)?;
        let rows = statement
            .query_map([tab], |row| {
                Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
            })
            .map_err(db_error)?;
        rows.map(|row| {
            let (id, document) = row.map_err(db_error)?;
            decode(&id, &document)
        })
        .collect()
    }

    pub(in crate::service_plan) fn recent_worker_titles(
        &self,
        workspace: &str,
        session: Option<&str>,
        limit: usize,
    ) -> Result<Vec<String>, PlanError> {
        let workspace = workspace.trim();
        if workspace.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let session = session.map(str::trim).filter(|value| !value.is_empty());
        let connection = self.connection.lock();
        record_query();
        let mut statement=connection.prepare_cached(
            "WITH selected_tasks AS MATERIALIZED (
                SELECT t.plan_id,t.task_id FROM plans p JOIN plan_tasks t ON t.plan_id=p.id
                WHERE p.status='active' AND json_extract(t.row_json,'$.lane.workspace_id')=?1
             ), titles AS (SELECT CASE WHEN json_type(a.row_json,'$.executor_title') IS NULL OR json_type(a.row_json,'$.executor_title')='null' THEN NULL
                WHEN json_type(a.row_json,'$.executor_title')<>'text' THEN json('invalid executor title')
                ELSE trim(json_extract(a.row_json,'$.executor_title'),?4) END AS title,
                domios_u64_order(CASE WHEN json_type(a.row_json,'$.assigned_at') IS NULL OR json_type(a.row_json,'$.assigned_at')='null'
                    THEN a.row_json->'$.reported_at' ELSE a.row_json->'$.assigned_at' END) AS at
                FROM selected_tasks t JOIN plan_task_attempts a ON a.plan_id=t.plan_id AND a.task_id=t.task_id
                WHERE json_extract(a.row_json,'$.role')='worker'
                    AND (?2 IS NULL OR a.session_id=?2))
             SELECT title FROM titles WHERE title IS NOT NULL AND title<>'' ORDER BY at DESC,title COLLATE BINARY LIMIT ?3"
        ).map_err(db_error)?;
        let rows = statement
            .query_map(
                params![
                    workspace,
                    session,
                    i64::try_from(limit).unwrap_or(i64::MAX),
                    super::status_queries::WHITESPACE
                ],
                |row| column::<String>(row, 0),
            )
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }

    pub(in crate::service_plan) fn task_attempt_projection(
        &self,
        id: &str,
        reference: &str,
        attempt_id: Option<&str>,
    ) -> Result<TaskAttemptRead, PlanError> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction().map_err(db_error)?;
        record_query();
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1)",
                [id],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        record_query();
        let task_id =
            super::task_page_queries::resolve_detail_task(&tx, id, reference).map_err(|error| {
                match error {
                    // find_task represented an ambiguous reference as TaskNotFound.
                    PlanError::Validation(_) => PlanError::TaskNotFound,
                    error => error,
                }
            })?;
        if task_id != reference {
            record_query()
        }
        record_query();
        let sql = if attempt_id.is_some() {
            "SELECT json_object('id',json_extract(row_json,'$.id'),'prompt',json_object('id',json_extract(row_json,'$.id'),'name',json_extract(row_json,'$.name'),
                'model',json_extract(row_json,'$.model'),'personality',json_extract(row_json,'$.personality'),'role_guidance',json_extract(row_json,'$.role_guidance'),
                'executor_role',json_extract(row_json,'$.executor_role'),'body',CASE WHEN json_type(row_json,'$.body') IS NULL THEN '' ELSE json_extract(row_json,'$.body') END,
                'intent',json_extract(row_json,'$.intent'),'correctness',json_extract(row_json,'$.correctness'),'test_plan',json_extract(row_json,'$.test_plan'),
                'acceptance_criteria',json(CASE WHEN json_type(row_json,'$.acceptance_criteria') IS NULL THEN '[]' ELSE row_json->'$.acceptance_criteria' END),
                'lane',json_object('worktree_path',json_extract(row_json,'$.lane.worktree_path'),'branch',json_extract(row_json,'$.lane.branch'),
                    'base',json_extract(row_json,'$.lane.base'),'destination',json_extract(row_json,'$.lane.destination'),
                    'workspace_id',json_extract(row_json,'$.lane.workspace_id'),'workspace_name',json_extract(row_json,'$.lane.workspace_name'))))
             FROM plan_tasks WHERE plan_id=?1 AND task_id=?2"
        } else {
            "SELECT json_object('id',json_extract(row_json,'$.id')) FROM plan_tasks WHERE plan_id=?1 AND task_id=?2"
        };
        let document: String = tx
            .query_row(sql, params![id, task_id], |row| column(row, 0))
            .map_err(db_error)?;
        let mut task: TaskAttemptRead = decode(id, &document)?;
        record_query();
        let mut statement=tx.prepare_cached(
            "SELECT row_json FROM plan_task_attempts WHERE plan_id=?1 AND task_id=?2 AND (?3 IS NULL OR json_extract(row_json,'$.id')=?3)
             ORDER BY position LIMIT ?4"
        ).map_err(db_error)?;
        let cap = if attempt_id.is_some() { 1 } else { i64::MAX };
        for document in statement
            .query_map(params![id, task_id, attempt_id, cap], |row| {
                column::<String>(row, 0)
            })
            .map_err(db_error)?
        {
            task.execution_attempts.push(decode::<TaskExecutionAttempt>(
                id,
                &document.map_err(db_error)?,
            )?);
        }
        drop(statement);
        if attempt_id.is_some() && task.execution_attempts.is_empty() {
            return Err(PlanError::TaskNotFound);
        }
        record_query();
        let mut statement=tx.prepare_cached(
            "WITH selected_attempts AS (SELECT executor_tab_id FROM plan_task_attempts WHERE plan_id=?1 AND task_id=?2
                AND (?3 IS NULL OR json_extract(row_json,'$.id')=?3) ORDER BY position LIMIT ?5),
             reports AS (SELECT seq,row_number() OVER (PARTITION BY json_extract(row_json,'$.from.tab_id')
                ORDER BY domios_u64_order(row_json->'$.ts') DESC,seq DESC) AS rank FROM plan_task_milestones m
                WHERE plan_id=?1 AND task_id=?2 AND EXISTS(SELECT 1 FROM selected_attempts a WHERE a.executor_tab_id=json_extract(m.row_json,'$.from.tab_id'))),
             context AS (SELECT seq FROM plan_task_milestones WHERE plan_id=?1 AND task_id=?2 AND ?4
                AND domios_worker_stride_rank(json_extract(row_json,'$.kind')) IS NOT NULL AND trim(coalesce(json_extract(row_json,'$.text'),''),?6)<>''
                ORDER BY domios_worker_stride_rank(json_extract(row_json,'$.kind')),
                    domios_u64_order(row_json->'$.ts') DESC,json_extract(row_json,'$.id') DESC,seq LIMIT ?7),
             chosen AS (SELECT seq FROM reports WHERE rank=1 UNION SELECT seq FROM context)
             SELECT json_object('id',json_extract(m.row_json,'$.id'),'from',json_object('tab_id',json_extract(m.row_json,'$.from.tab_id')),
                'kind',json_extract(m.row_json,'$.kind'),'text',CASE WHEN json_type(m.row_json,'$.text') IS NULL THEN '' ELSE json_extract(m.row_json,'$.text') END,
                'ts',json(m.row_json->'$.ts')) FROM plan_task_milestones m JOIN chosen USING(seq) WHERE plan_id=?1 AND task_id=?2 ORDER BY seq"
        ).map_err(db_error)?;
        for document in statement
            .query_map(
                params![
                    id,
                    task_id,
                    attempt_id,
                    attempt_id.is_some(),
                    cap,
                    super::status_queries::WHITESPACE,
                    crate::service_plan::stride_context::WORKER_STRIDE_LIMIT
                ],
                |row| column::<String>(row, 0),
            )
            .map_err(db_error)?
        {
            task.milestones
                .push(decode::<Milestone>(id, &document.map_err(db_error)?)?);
        }
        drop(statement);
        tx.commit().map_err(db_error)?;
        Ok(task)
    }
}

pub(super) fn register_functions(connection: &rusqlite::Connection) -> Result<(), PlanError> {
    use rusqlite::functions::FunctionFlags;
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
    connection
        .create_scalar_function("domios_worker_stride_rank", 1, flags, |context| {
            let kind = context
                .get_raw(0)
                .as_str()
                .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
            Ok((!crate::service_plan::stride_context::is_noise(kind))
                .then(|| crate::service_plan::stride_context::tier_rank(kind)))
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_u64_order", 1, flags, |context| {
            let value = match context.get_raw(0) {
                rusqlite::types::ValueRef::Null => None,
                rusqlite::types::ValueRef::Text(bytes) if bytes.len() <= 20 => {
                    serde_json::from_slice::<Option<u64>>(bytes)
                        .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?
                }
                _ => {
                    return Err(rusqlite::Error::UserFunctionError(Box::new(
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "invalid unsigned timestamp",
                        ),
                    )))
                }
            };
            Ok(format!("{:020}", value.unwrap_or(0)))
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_request_pending", 1, flags, |context| match context
            .get_raw(0)
            .as_str()
        {
            Ok("completed") => Ok(false),
            Ok("queued" | "accepted" | "in_progress" | "failed") => Ok(true),
            _ => Err(rusqlite::Error::UserFunctionError(Box::new(
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "invalid planner request state",
                ),
            ))),
        })
        .map_err(db_error)
}
