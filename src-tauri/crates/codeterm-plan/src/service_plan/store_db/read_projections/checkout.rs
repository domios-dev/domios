use super::{column, decode, record_query};
use crate::plan::{TaskFiles, WorkerPlacement};
use crate::service_plan::shared_checkout::ReportedSharedCommit;
use crate::service_plan::store_db::{db_error, PlanDatabase};
use crate::service_plan::{PlanError, PlanStore};
use rusqlite::params;

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct CheckoutPeerRead {
    pub id: String,
    pub checkout: String,
    pub tasks: Vec<CheckoutTaskRead>,
}

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct CheckoutTaskRead {
    pub id: String,
    pub files: TaskFiles,
    pub execution_attempts: Vec<CheckoutAttemptRead>,
}

pub(in crate::service_plan::store_db) const LIVE_CHECKOUT_WRITER_PREDICATE: &str = "a.open
    AND coalesce(json_extract(t.row_json,'$.execution_intent'),'repository_mutation')='repository_mutation'
    AND coalesce(json_extract(a.row_json,'$.execution_intent'),'unspecified')<>'read_only'
    AND CASE
        WHEN json_type(a.row_json,'$.placement') IS NULL THEN 0
        WHEN json_type(a.row_json,'$.placement')='text' THEN
            CASE json_extract(a.row_json,'$.placement')
                WHEN 'canonical_writer' THEN 1
                WHEN 'shared' THEN 1
                WHEN 'isolated_lane' THEN 0
                WHEN 'read_only' THEN 0
                WHEN 'non_repository' THEN 0
                ELSE json('invalid worker placement')
            END
        ELSE json('invalid worker placement')
    END
    AND NOT (
        json_extract(t.row_json,'$.status') IS 'cancelled'
        AND json_extract(a.row_json,'$.mesh_peer') IS NULL
        AND trim(coalesce(json_extract(a.row_json,'$.id'),''))<>''
        AND trim(coalesce(a.executor_tab_id,''))<>''
        AND json_type(a.row_json,'$.executor_termination.confirmed_at') IS 'integer'
        AND json_extract(a.row_json,'$.executor_termination.attempt_id') IS json_extract(a.row_json,'$.id')
        AND json_extract(a.row_json,'$.executor_termination.executor_tab_id') IS a.executor_tab_id
        AND json_extract(a.row_json,'$.executor_termination.session_id') IS a.session_id
        AND json_extract(a.row_json,'$.executor_termination.session_binding_nonce') IS a.session_binding_nonce
    )";

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct CheckoutAttemptRead {
    pub id: String,
    pub placement: WorkerPlacement,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkout_path: Option<String>,
}

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct TaskCheckoutRead {
    pub id: String,
    pub checkout: String,
    pub files: TaskFiles,
    pub live_shared: bool,
}

impl PlanStore {
    pub fn task_checkout_projection(
        &self,
        plan: &str,
        task: &str,
        exact: bool,
    ) -> Result<TaskCheckoutRead, PlanError> {
        self.database.task_checkout_projection(plan, task, exact)
    }
    pub fn checkout_peer_facts(&self, checkout: &str) -> Result<Vec<CheckoutPeerRead>, PlanError> {
        let roots = self.database.checkout_roots()?;
        let checkout = super::super::super::shared_checkout::resolved_checkout_target(checkout);
        let mut ids = roots
            .iter()
            .filter(|(_, root)| {
                super::super::super::shared_checkout::resolved_checkout_target(root) == checkout
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        ids.sort();
        ids.dedup();
        self.database.checkout_peer_facts(&roots, &ids)
    }

    pub(in crate::service_plan) fn checkout_plan_ids(
        &self,
        canonical_checkout: &str,
    ) -> Result<Vec<String>, PlanError> {
        let target =
            super::super::super::shared_checkout::resolved_checkout_target(canonical_checkout);
        let mut ids = self
            .database
            .checkout_roots()?
            .into_iter()
            .filter(|(_, root)| {
                super::super::super::shared_checkout::resolved_checkout_target(root) == target
            })
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        ids.sort();
        ids.dedup();
        Ok(ids)
    }
}

impl PlanDatabase {
    /// Validated in_review reports of shared attempts whose task is in review or accepted.
    pub(in crate::service_plan) fn reported_shared_commits(
        &self,
        plan_ids: &[String],
    ) -> Result<Vec<ReportedSharedCommit>, PlanError> {
        let ids =
            serde_json::to_string(plan_ids).map_err(|error| PlanError::Io(error.to_string()))?;
        let connection = self.connection.lock();
        record_query();
        let mut statement = connection
            .prepare_cached(
                "SELECT a.plan_id,a.task_id,a.attempt_id,json_extract(a.row_json,'$.report.sha')
             FROM plan_task_attempts a
             JOIN plan_tasks t ON t.plan_id=a.plan_id AND t.task_id=a.task_id
             JOIN plans p ON p.id=a.plan_id
             WHERE a.plan_id IN (SELECT value FROM json_each(?1)) AND p.status='active'
                AND t.status IN ('in_review','done')
                AND json_extract(a.row_json,'$.placement')='shared'
                AND json_extract(a.row_json,'$.report.status')='in_review'
                AND json_type(a.row_json,'$.report.sha')='text'
             ORDER BY a.plan_id,t.position,a.position",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map(params![ids], |row| {
                Ok(ReportedSharedCommit {
                    plan_id: row.get(0)?,
                    task_id: row.get(1)?,
                    attempt_id: row.get(2)?,
                    sha: row.get(3)?,
                })
            })
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }

    pub(in crate::service_plan) fn stride_recorded(
        &self,
        plan_id: &str,
        stride_id: &str,
    ) -> Result<bool, PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM plan_strides
                 WHERE json_extract(row_json,'$.id')=?2 AND plan_id=?1)",
                params![plan_id, stride_id],
                |row| row.get(0),
            )
            .map_err(db_error)
    }

    pub(in crate::service_plan) fn shared_history_checkouts(
        &self,
    ) -> Result<Vec<String>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let mut statement = connection
            .prepare_cached(
                "SELECT DISTINCT json_extract(a.row_json,'$.checkout_path')
             FROM plan_task_attempts a JOIN plans p ON p.id=a.plan_id
             WHERE p.status='active' AND json_extract(a.row_json,'$.placement')='shared'
                AND json_type(a.row_json,'$.checkout_path')='text'
                AND json_type(a.row_json,'$.shared_start_files') IS NULL",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }
}

impl PlanDatabase {
    fn task_checkout_projection(
        &self,
        plan: &str,
        task: &str,
        exact: bool,
    ) -> Result<TaskCheckoutRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (exists, raw): (bool,Option<String>) = connection.query_row(&format!(
            "{selected} SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),
                (SELECT json_object('id',t.task_id,'checkout',{root},'files',json(CASE WHEN json_type(t.row_json,'$.files') IS NULL THEN '{{}}' ELSE t.row_json->'$.files' END),
                    'live_shared',json(CASE WHEN EXISTS(SELECT 1 FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id
                        AND {writer} AND json_extract(a.row_json,'$.placement')='shared') THEN 'true' ELSE 'false' END))
                 FROM selected_task t JOIN plans p ON p.id=t.plan_id WHERE NOT ?3 OR t.task_id=?2)",
            selected=super::super::task_authority_queries::SELECTED_TASK_IN_PLAN,
            root="CASE WHEN trim(coalesce(json_extract(p.header_json,'$.canonical_project_path'),''),?4)='' THEN p.project_path ELSE json_extract(p.header_json,'$.canonical_project_path') END",
            writer=LIVE_CHECKOUT_WRITER_PREDICATE,
        ),params![plan,task,exact,super::super::status_queries::WHITESPACE],|row| Ok((column(row,0)?,column(row,1)?))).map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        decode(plan, &raw.ok_or(PlanError::TaskNotFound)?)
    }
    fn checkout_roots(&self) -> Result<Vec<(String, String)>, PlanError> {
        let connection = self.connection.lock();
        checkout_roots(&connection)
    }

    fn checkout_peer_facts(
        &self,
        roots: &[(String, String)],
        ids: &[String],
    ) -> Result<Vec<CheckoutPeerRead>, PlanError> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction().map_err(db_error)?;
        if checkout_roots(&tx)? != roots {
            return Err(PlanError::Io(
                "checkout roots changed during ownership inspection".into(),
            ));
        }
        let ids = serde_json::to_string(ids).map_err(|error| PlanError::Io(error.to_string()))?;
        record_query();
        let mut statement = tx.prepare(&format!(
            "WITH admitted AS (SELECT p.id FROM plans p WHERE p.id IN (SELECT value FROM json_each(?1))
                AND EXISTS(SELECT 1 FROM plan_tasks t JOIN plan_task_attempts a
                    ON a.plan_id=t.plan_id AND a.task_id=t.task_id
                    WHERE t.plan_id=p.id AND {writer}))
            SELECT p.id,{root},json_object('id',p.id,'checkout',{root},'tasks',json((SELECT json_group_array(json(task)) FROM (
                SELECT json_object('id',t.task_id,'files',json(CASE WHEN json_type(t.row_json,'$.files') IS NULL THEN '{{}}' ELSE t.row_json->'$.files' END),
                    'execution_attempts',json((SELECT json_group_array(json(attempt)) FROM (
                        SELECT json_object(
                            'id',json_extract(a.row_json,'$.id'),
                            'placement',json_extract(a.row_json,'$.placement'),
                            'checkout_path',CASE
                                WHEN trim(coalesce(json_extract(a.row_json,'$.checkout_path'),''))<>'' THEN json_extract(a.row_json,'$.checkout_path')
                                WHEN trim(coalesce(json_extract(a.row_json,'$.worktree_path'),''))<>'' THEN json_extract(a.row_json,'$.worktree_path')
                            END) AS attempt
                        FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id
                            AND {writer}
                        ORDER BY a.position)))) AS task
                FROM plan_tasks t WHERE t.plan_id=p.id AND EXISTS(SELECT 1 FROM plan_task_attempts a
                    WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id AND {writer}) ORDER BY t.position))))
            FROM plans p JOIN admitted ON admitted.id=p.id ORDER BY p.updated_at DESC,p.id",
            root=super::super::match_queries::ROOT,
            writer=LIVE_CHECKOUT_WRITER_PREDICATE,
        )).map_err(db_error)?;
        let rows = statement
            .query_map(params![ids], |row| {
                Ok((column::<String>(row, 0)?, column::<String>(row, 2)?))
            })
            .map_err(db_error)?;
        let result: Result<Vec<CheckoutPeerRead>, PlanError> = rows
            .map(|row| {
                let (id, raw) = row.map_err(db_error)?;
                decode(&id, &raw)
            })
            .collect();
        drop(statement);
        tx.commit().map_err(db_error)?;
        result
    }
}

fn checkout_roots(connection: &rusqlite::Connection) -> Result<Vec<(String, String)>, PlanError> {
    record_query();
    let mut statement = connection
        .prepare(&format!(
            "SELECT id,{root} FROM plans
             UNION
             SELECT a.plan_id, CASE
                 WHEN trim(coalesce(json_extract(a.row_json,'$.checkout_path'),''))<>'' THEN json_extract(a.row_json,'$.checkout_path')
                 ELSE json_extract(a.row_json,'$.worktree_path')
             END
             FROM plan_task_attempts a
             JOIN plan_tasks t ON t.plan_id=a.plan_id AND t.task_id=a.task_id
             WHERE {writer}
                AND (trim(coalesce(json_extract(a.row_json,'$.checkout_path'),''))<>''
                    OR trim(coalesce(json_extract(a.row_json,'$.worktree_path'),''))<>'')
             ORDER BY 1, 2",
            root=super::super::match_queries::ROOT,
            writer=LIVE_CHECKOUT_WRITER_PREDICATE,
        ))
        .map_err(db_error)?;
    let rows = statement
        .query_map([], |row| Ok((column(row, 0)?, column(row, 1)?)))
        .map_err(db_error)?;
    rows.collect::<Result<_, _>>().map_err(db_error)
}

#[cfg(test)]
mod tests;
