use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::service_plan::task_authority_projection::TabAttemptOwnerRead;
use crate::service_plan::{
    PlanError, PlanTaskAdmissionRead, TaskBindingRead, TaskContextFilesRead, TaskOwnerEpochRead,
};
use rusqlite::{params, OptionalExtension};

pub(super) const SELECTED_ACTIVE_TASK: &str = "WITH candidates AS (
                SELECT p.id AS plan_id,t.task_id,t.row_json,t.assigned_tab_id,p.updated_at,t.position,
                    t.task_id=?1 AS exact,
                    sum(t.task_id=?1) OVER (PARTITION BY p.id) AS exact_count,
                    count(*) OVER (PARTITION BY p.id) AS matches
                FROM plans p JOIN plan_tasks t ON t.plan_id=p.id
                WHERE p.status='active' AND (t.task_id=?1 OR (?1<>'' AND (
                    json_extract(t.row_json,'$.handle') COLLATE NOCASE=?1 OR
                    coalesce(json_extract(t.row_json,'$.key'),CASE WHEN substr(t.task_id,1,5)='task_' THEN substr(t.task_id,6) ELSE t.task_id END) COLLATE NOCASE=?1))))
             , selected_task AS (
                SELECT plan_id,task_id,row_json,assigned_tab_id FROM candidates
                WHERE exact OR (exact_count=0 AND matches=1)
                ORDER BY updated_at DESC,plan_id,position LIMIT 1
             )";

pub(super) const SELECTED_TASK_IN_PLAN: &str = "WITH candidates AS (
    SELECT t.plan_id,t.task_id,t.row_json,t.assigned_tab_id,t.position,t.task_id=?2 AS exact,
        sum(t.task_id=?2) OVER () AS exact_count,count(*) OVER () AS matches
    FROM plan_tasks t WHERE t.plan_id=?1 AND (t.task_id=?2 OR (?2<>'' AND (
        json_extract(t.row_json,'$.handle') COLLATE NOCASE=?2 OR
        coalesce(json_extract(t.row_json,'$.key'),CASE WHEN substr(t.task_id,1,5)='task_' THEN substr(t.task_id,6) ELSE t.task_id END) COLLATE NOCASE=?2)))),
    selected_task AS (SELECT plan_id,task_id,row_json,assigned_tab_id FROM candidates
        WHERE exact OR (exact_count=0 AND matches=1) ORDER BY position LIMIT 1)";

impl PlanDatabase {
    pub(in crate::service_plan) fn plan_work_facet_projection(
        &self,
        plan_id: &str,
    ) -> Result<bool, PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM json_each(p.header_json,'$.tag_facets')
                    WHERE json_extract(value,'$.key')='work') FROM plans p WHERE id=?1",
                [plan_id],
                |row| column(row, 0),
            )
            .optional()
            .map_err(db_error)?
            .ok_or(PlanError::NotFound)
    }

    pub(in crate::service_plan) fn task_name_projection(
        &self,
        reference: &str,
    ) -> Result<String, PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection.query_row(
            &format!("{SELECTED_ACTIVE_TASK} SELECT json_extract(row_json,'$.name') FROM selected_task"),
            [reference], |row| column(row,0),
        ).optional().map_err(db_error)?.ok_or(PlanError::TaskNotFound)
    }

    pub(in crate::service_plan) fn task_authority_target_projection(
        &self,
        reference: &str,
    ) -> Result<TaskBindingRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection
            .query_row(
                &format!("{SELECTED_ACTIVE_TASK} SELECT plan_id,task_id FROM selected_task"),
                [reference],
                |row| {
                    Ok(TaskBindingRead {
                        plan_id: column(row, 0)?,
                        task_id: column(row, 1)?,
                    })
                },
            )
            .optional()
            .map_err(db_error)?
            .ok_or(PlanError::TaskNotFound)
    }

    pub(in crate::service_plan) fn plan_task_admission_projection(
        &self,
        plan_id: &str,
    ) -> Result<PlanTaskAdmissionRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let encoded: String = connection
            .query_row(
                "SELECT json_object('mode',coalesce(json_extract(header_json,'$.mode'),'planning'),
                'planning_state',coalesce(json_extract(header_json,'$.planning_state'),'drafting'))
             FROM plans WHERE id=?1",
                [plan_id],
                |row| column(row, 0),
            )
            .optional()
            .map_err(db_error)?
            .ok_or(PlanError::NotFound)?;
        decode(plan_id, &encoded)
    }

    pub(in crate::service_plan) fn ensure_plan_exists(
        &self,
        plan_id: &str,
    ) -> Result<(), PlanError> {
        let connection = self.connection.lock();
        record_query();
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1)",
                [plan_id],
                |row| column(row, 0),
            )
            .map_err(db_error)?;
        if exists {
            Ok(())
        } else {
            Err(PlanError::NotFound)
        }
    }

    pub(in crate::service_plan) fn unplanned_task_section_projection(
        &self,
        plan_id: &str,
        parent_task_id: Option<&str>,
    ) -> Result<Option<String>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection.query_row(
            "SELECT coalesce(
                (SELECT json_extract(row_json,'$.section_ids[0]') FROM plan_tasks WHERE plan_id=p.id AND task_id=?2),
                (SELECT json_extract(row_json,'$.id') FROM plan_sections WHERE plan_id=p.id ORDER BY position LIMIT 1))
             FROM plans p WHERE id=?1",
            params![plan_id,parent_task_id], |row| column(row,0),
        ).optional().map_err(db_error)?.ok_or(PlanError::NotFound)
    }

    pub(in crate::service_plan) fn task_owner_epoch_projection(
        &self,
        reference: &str,
    ) -> Result<TaskOwnerEpochRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection
            .query_row(
                &format!(
                    "{SELECTED_ACTIVE_TASK}
             SELECT plan_id,task_id,coalesce(json_extract(row_json,'$.interruption.owner_epoch'),0),
                json_extract(row_json,'$.interruption.owner_tab_id'),
                json_extract(row_json,'$.interruption.owner_session_id'),
                coalesce(json_extract(row_json,'$.interruption.handoff_required'),0)
             FROM selected_task"
                ),
                [reference],
                |row| {
                    Ok(TaskOwnerEpochRead {
                        plan_id: column(row, 0)?,
                        task_id: column(row, 1)?,
                        owner_epoch: column(row, 2)?,
                        owner_tab_id: column(row, 3)?,
                        owner_session_id: column(row, 4)?,
                        handoff_required: column(row, 5)?,
                    })
                },
            )
            .optional()
            .map_err(db_error)?
            .ok_or(PlanError::TaskNotFound)
    }

    pub(in crate::service_plan) fn task_context_files_projection(
        &self,
        reference: &str,
    ) -> Result<TaskContextFilesRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (plan_id, task_id, files): (String, String, String) = connection.query_row(
            &format!("{SELECTED_ACTIVE_TASK} SELECT plan_id,task_id,coalesce(json_extract(row_json,'$.files'),'{{}}') FROM selected_task"),
            [reference], |row| Ok((column(row,0)?,column(row,1)?,column(row,2)?)),
        ).optional().map_err(db_error)?.ok_or(PlanError::TaskNotFound)?;
        Ok(TaskContextFilesRead {
            files: decode(&plan_id, &files)?,
            plan_id,
            task_id,
        })
    }

    pub(in crate::service_plan) fn current_skill_attempt(
        &self,
        tab: &str,
        session: &str,
        nonce: &str,
    ) -> Result<Option<(String, String, String)>, PlanError> {
        let rows = self.tab_attempt_owners_projection(tab)?;
        crate::service_plan::task_authority_projection::current_skill_from_rows(
            &rows, tab, session, nonce,
        )
    }

    pub(in crate::service_plan) fn tab_attempt_owners_projection(
        &self,
        tab: &str,
    ) -> Result<Vec<TabAttemptOwnerRead>, PlanError> {
        let connection = self.connection.lock();
        tab_attempt_owners(&connection, tab)
    }

    pub(in crate::service_plan) fn executor_termination_confirmed(
        &self,
        plan: &str,
        task: &str,
        source: crate::service_plan::task_attempts::ExecutorSourceFields<'_>,
    ) -> Result<bool, PlanError> {
        let mut connection = self.connection.lock();
        let transaction = connection.transaction().map_err(db_error)?;
        record_query();
        let exists: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1)",
                [plan],
                |row| column(row, 0),
            )
            .map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        record_query();
        let task: String = transaction.query_row(
            "WITH candidates AS (
                SELECT task_id,position,task_id=?2 AS exact,
                    sum(task_id=?2) OVER () AS exact_count,count(*) OVER () AS matches
                FROM plan_tasks WHERE plan_id=?1 AND (task_id=?2 OR (?2<>'' AND (
                    json_extract(row_json,'$.handle') COLLATE NOCASE=?2 OR
                    coalesce(json_extract(row_json,'$.key'),CASE WHEN substr(task_id,1,5)='task_' THEN substr(task_id,6) ELSE task_id END) COLLATE NOCASE=?2))))
             SELECT task_id FROM candidates WHERE exact OR (exact_count=0 AND matches=1)
             ORDER BY position LIMIT 1",
            params![plan,task], |row| column(row,0),
        ).optional().map_err(db_error)?.ok_or(PlanError::TaskNotFound)?;
        record_query();
        let confirmed = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM plan_task_attempts a WHERE a.plan_id=?1 AND a.task_id=?2
                AND json_extract(a.row_json,'$.id')=?3 AND a.executor_tab_id=?4
                AND a.session_id IS ?5 AND a.session_binding_nonce IS ?6
                AND json_extract(a.row_json,'$.mesh_peer') IS NULL AND ?7
                AND json_extract(a.row_json,'$.executor_termination.attempt_id')=?3
                AND json_extract(a.row_json,'$.executor_termination.executor_tab_id')=?4
                AND json_extract(a.row_json,'$.executor_termination.session_id') IS ?5
                AND json_extract(a.row_json,'$.executor_termination.session_binding_nonce') IS ?6)",
            params![plan,task,source.id,source.executor_tab_id,source.session_id,source.session_binding_nonce,source.mesh_peer.is_none()],
            |row| column(row,0),
        ).map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        Ok(confirmed)
    }
}

mod worker_transfer;

mod startup;

mod continuity;
pub(super) fn register_functions(connection: &rusqlite::Connection) -> Result<(), PlanError> {
    continuity::register_functions(connection)?;
    ack::register(connection)?;
    dispatch_preflight::register(connection)
}

mod checkpoint;
mod creation;
mod gates;
mod restart;
mod restoration;
mod review;
mod runtime_restore;
mod runtime_session;
mod split;

mod provider_interruption;

mod remote;

mod lane_admission;

mod batch;

mod shared_inspection;

mod review_roles;

mod plan_metadata;

mod respawn_source;

mod worker_loss;

mod ack;

mod lifecycle;

mod launch;

mod deadline;

mod spawn_binding;

mod worker_preparation;

mod dispatch_preflight;

pub(super) fn tab_attempt_owners(
    connection: &rusqlite::Connection,
    tab: &str,
) -> Result<Vec<TabAttemptOwnerRead>, PlanError> {
    record_query();
    let mut statement = connection.prepare_cached(
            "SELECT p.id,t.task_id,json_extract(a.row_json,'$.id'),t.assigned_tab_id,
                json_extract(a.row_json,'$.competition_group_id'),a.session_id,a.session_binding_nonce
             FROM plans p JOIN plan_tasks t ON t.plan_id=p.id
             LEFT JOIN plan_task_attempts a ON a.plan_id=t.plan_id AND a.task_id=t.task_id
                AND a.executor_tab_id=?1 AND json_extract(a.row_json,'$.closed_at') IS NULL
                AND json_extract(a.row_json,'$.retired_at') IS NULL
             WHERE p.status='active' AND t.status IN ('pending','in_progress','in_review','blocked','paused')
                AND (a.attempt_id IS NOT NULL OR t.assigned_tab_id=?1)
             ORDER BY p.updated_at DESC,p.id,t.position,a.position LIMIT 2"
        ).map_err(db_error)?;
    let rows = statement
        .query_map([tab], |row| {
            Ok(TabAttemptOwnerRead {
                plan_id: column(row, 0)?,
                task_id: column(row, 1)?,
                attempt_id: column(row, 2)?,
                assigned_tab: column(row, 3)?,
                competition_group: column(row, 4)?,
                session_id: column(row, 5)?,
                nonce: column(row, 6)?,
            })
        })
        .map_err(db_error)?;
    rows.collect::<Result<_, _>>().map_err(db_error)
}
