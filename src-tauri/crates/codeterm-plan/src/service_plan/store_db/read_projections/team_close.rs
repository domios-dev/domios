use super::{column, decode, record_query};
use crate::plan::{LaneOwnership, PlanStatus, TaskStatus};
use crate::service_plan::store_db::{db_error, PlanDatabase};
use crate::service_plan::{PlanError, PlanStore};
use std::fmt;

#[derive(Debug)]
pub enum TeamCloseProjectionError {
    TooLarge,
    Store(PlanError),
}

impl fmt::Display for TeamCloseProjectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => f.write_str("workspace close plan projection exceeds its row bound"),
            Self::Store(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for TeamCloseProjectionError {}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TeamClosePlanRead {
    pub id: String,
    #[serde(default)]
    pub title: String,
    pub status: PlanStatus,
    pub team_id: Option<String>,
    pub project_path: String,
    pub canonical_project_path: String,
    #[serde(skip_deserializing)]
    pub tasks: Vec<TeamCloseTaskRead>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TeamCloseTaskRead {
    pub id: String,
    pub name: String,
    pub status: TaskStatus,
    pub assigned_tab_id: Option<String>,
    pub lane: TeamCloseLaneRead,
    pub has_remote_attempt: bool,
    pub member_executor: Option<String>,
    pub attempt_lanes: Vec<TeamCloseAttemptRead>,
    #[serde(default)]
    pub open_attempts: Vec<TeamCloseOpenAttemptRead>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TeamCloseLaneRead {
    pub tab_id: Option<String>,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub base: Option<String>,
    pub destination: Option<String>,
    #[serde(default)]
    pub ownership: Option<LaneOwnership>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TeamCloseAttemptRead {
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub base: Option<String>,
    pub destination: Option<String>,
    #[serde(default)]
    pub ownership: Option<LaneOwnership>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TeamCloseOpenAttemptRead {
    pub id: String,
    pub executor_tab_id: Option<String>,
    pub remote: bool,
}

impl PlanDatabase {
    pub(in crate::service_plan) fn team_close_projection(
        &self,
        team: &str,
        members: &[String],
    ) -> Result<Vec<TeamClosePlanRead>, PlanError> {
        self.team_close_projection_inner(team, members, None)
            .map_err(|error| match error {
                TeamCloseProjectionError::Store(error) => error,
                TeamCloseProjectionError::TooLarge => {
                    unreachable!("unbounded projection cannot overflow")
                }
            })
    }

    pub(in crate::service_plan) fn team_close_projection_bounded(
        &self,
        team: &str,
        members: &[String],
        max_rows: usize,
    ) -> Result<Vec<TeamClosePlanRead>, TeamCloseProjectionError> {
        self.team_close_projection_inner(team, members, Some(max_rows))
    }

    fn team_close_projection_inner(
        &self,
        team: &str,
        members: &[String],
        max_rows: Option<usize>,
    ) -> Result<Vec<TeamClosePlanRead>, TeamCloseProjectionError> {
        let predicate = crate::service_plan::store_queries::team_close_candidates(team, members);
        let (clause, mut params) = crate::service_plan::plan_match::compile(&predicate);
        let mut remaining = max_rows.unwrap_or(usize::MAX);
        let plan_limit = sql_row_limit(max_rows, remaining);
        params.push(rusqlite::types::Value::Integer(plan_limit));
        let mut connection = self.connection.lock();
        let tx = connection
            .transaction()
            .map_err(db_error)
            .map_err(TeamCloseProjectionError::Store)?;
        record_query();
        let mut statement = tx
            .prepare_cached(&format!(
                "SELECT p.id,json_object('id',p.id,'title',p.title,'status',p.status,'team_id',p.team_id,
             'project_path',p.project_path,'canonical_project_path',
             CASE WHEN json_type(p.header_json,'$.canonical_project_path') IS NULL THEN ''
             ELSE json_extract(p.header_json,'$.canonical_project_path') END)
             FROM plans p WHERE {clause} ORDER BY p.updated_at DESC,p.id LIMIT ?"
            ))
            .map_err(db_error)
            .map_err(TeamCloseProjectionError::Store)?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(params), |row| {
                Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
            })
            .map_err(db_error)
            .map_err(TeamCloseProjectionError::Store)?;
        let mut plans = Vec::new();
        for row in rows {
            let (id, raw) = row
                .map_err(db_error)
                .map_err(TeamCloseProjectionError::Store)?;
            if max_rows.is_some() && plans.len() >= remaining {
                return Err(TeamCloseProjectionError::TooLarge);
            }
            plans.push(
                decode::<TeamClosePlanRead>(&id, &raw).map_err(TeamCloseProjectionError::Store)?,
            );
        }
        drop(statement);
        if max_rows.is_some() {
            remaining = remaining.saturating_sub(plans.len());
        }
        let members_json = serde_json::to_string(members)
            .map_err(super::super::json_error)
            .map_err(TeamCloseProjectionError::Store)?;
        for plan in &mut plans {
            let task_limit = sql_row_limit(max_rows, remaining);
            record_query();
            let mut statement = tx.prepare_cached(
                "WITH task_facts AS MATERIALIZED (
                    SELECT plan_id,task_id,position,status,assigned_tab_id,json_extract(row_json,'$.name') AS name,
                        json_extract(row_json,'$.lane.tab_id') AS lane_tab,
                        json_extract(row_json,'$.lane.worktree_path') AS worktree_path,
                        json_extract(row_json,'$.lane.branch') AS branch,
                        json_extract(row_json,'$.lane.base') AS base,
                        json_extract(row_json,'$.lane.destination') AS destination,
                        json_extract(row_json,'$.lane.ownership') AS ownership
                    FROM plan_tasks WHERE plan_id=?1
                 ), remote_state AS MATERIALIZED (
                    SELECT task_id,max(json_type(row_json,'$.remote_worktree_path') NOT IN ('null','text')) AS invalid,
                        max(json_extract(row_json,'$.remote_worktree_path') IS NOT NULL) AS present
                    FROM plan_task_attempts WHERE plan_id=?1 GROUP BY task_id
                 ) SELECT json_object('id',t.task_id,'name',t.name,
                    'status',t.status,'assigned_tab_id',t.assigned_tab_id,
                    'lane',json_object('tab_id',t.lane_tab,
                        'worktree_path',t.worktree_path,
                        'branch',t.branch,
                        'base',t.base,
                        'destination',t.destination,
                        'ownership',t.ownership),
                    'has_remote_attempt',json(CASE WHEN (coalesce((SELECT invalid FROM remote_state WHERE task_id=t.task_id),0)>0) THEN 'null'
                        WHEN (coalesce((SELECT present FROM remote_state WHERE task_id=t.task_id),0)>0)
                        THEN 'true' ELSE 'false' END),
                    'member_executor',CASE
                        WHEN t.assigned_tab_id IN (SELECT value FROM json_each(?3)) THEN t.assigned_tab_id
                        WHEN t.lane_tab IN (SELECT value FROM json_each(?3))
                            THEN t.lane_tab
                        ELSE (SELECT a.executor_tab_id FROM plan_task_attempts a
                            WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id
                            AND json_extract(a.row_json,'$.closed_at') IS NULL
                            AND a.executor_tab_id IN (SELECT value FROM json_each(?3))
                            ORDER BY a.position LIMIT 1) END,
                    'attempt_lanes',json((SELECT json_group_array(json(document)) FROM (
                        SELECT json_object('worktree_path',worktree_path,'branch',branch,
                            'base',base,'destination',destination,
                            'ownership',CASE WHEN sum(ownership IS 'foreign_checkout')>0 THEN 'foreign_checkout'
                                WHEN sum(ownership IS NULL)>0 THEN NULL ELSE ownership END) AS document,min(position) AS first_position
                        FROM (SELECT a.position,
                            coalesce(json_extract(a.row_json,'$.worktree_path'),t.worktree_path) AS worktree_path,
                            coalesce(json_extract(a.row_json,'$.branch'),t.branch) AS branch,
                            coalesce(json_extract(a.row_json,'$.base'),t.base) AS base,
                            coalesce(json_extract(a.row_json,'$.destination'),t.destination) AS destination,
                            CASE WHEN json_extract(a.row_json,'$.worktree_path') IS NOT NULL
                                OR json_extract(a.row_json,'$.branch') IS NOT NULL
                                THEN json_extract(a.row_json,'$.lane_ownership') ELSE t.ownership END AS ownership
                        FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id
                        AND (?2 OR a.executor_tab_id IN (SELECT value FROM json_each(?3)))
                        AND NOT (coalesce((SELECT present FROM remote_state WHERE task_id=t.task_id),0)>0))
                        WHERE nullif(trim(worktree_path,?4),'') IS NOT NULL OR nullif(trim(branch,?4),'') IS NOT NULL
                        GROUP BY nullif(trim(worktree_path,?4),''),nullif(trim(branch,?4),''),
                            CASE WHEN destination IS NULL OR (destination<>'' AND destination=trim(destination,?4)) THEN destination ELSE position END
                        ORDER BY first_position))))
                 FROM task_facts t WHERE (?2
                    OR t.assigned_tab_id IN (SELECT value FROM json_each(?3))
                    OR t.lane_tab IN (SELECT value FROM json_each(?3))
                    OR EXISTS(SELECT 1 FROM plan_task_attempts a WHERE a.plan_id=t.plan_id
                        AND a.task_id=t.task_id AND a.executor_tab_id IN (SELECT value FROM json_each(?3))))
                 AND ((?5 AND t.status='in_progress')
                    OR EXISTS(SELECT 1 FROM plan_task_attempts a WHERE a.plan_id=t.plan_id
                        AND a.task_id=t.task_id AND json_extract(a.row_json,'$.closed_at') IS NULL
                        AND (?2 OR a.executor_tab_id IN (SELECT value FROM json_each(?3))))
                    OR (coalesce((SELECT invalid FROM remote_state WHERE task_id=t.task_id),0)>0) OR (
                    NOT (coalesce((SELECT present FROM remote_state WHERE task_id=t.task_id),0)>0)
                    AND (nullif(trim(t.worktree_path,?4),'') IS NOT NULL
                        OR nullif(trim(t.branch,?4),'') IS NOT NULL
                        OR EXISTS(SELECT 1 FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id
                            AND (?2 OR a.executor_tab_id IN (SELECT value FROM json_each(?3)))
                            AND (nullif(trim(json_extract(a.row_json,'$.worktree_path'),?4),'') IS NOT NULL
                                OR nullif(trim(json_extract(a.row_json,'$.branch'),?4),'') IS NOT NULL)))))
                 ORDER BY t.position LIMIT ?6"
            ).map_err(db_error).map_err(TeamCloseProjectionError::Store)?;
            let rows = statement
                .query_map(
                    rusqlite::params![
                        plan.id,
                        plan.team_id.as_deref() == Some(team),
                        members_json,
                        super::super::status_queries::WHITESPACE,
                        plan.status == PlanStatus::Active,
                        task_limit
                    ],
                    |row| column::<String>(row, 0),
                )
                .map_err(db_error)
                .map_err(TeamCloseProjectionError::Store)?;
            let mut tasks = Vec::new();
            for row in rows {
                let raw = row
                    .map_err(db_error)
                    .map_err(TeamCloseProjectionError::Store)?;
                if max_rows.is_some() && tasks.len() >= remaining {
                    return Err(TeamCloseProjectionError::TooLarge);
                }
                tasks.push(decode(&plan.id, &raw).map_err(TeamCloseProjectionError::Store)?);
            }
            drop(statement);
            if max_rows.is_some() {
                remaining = remaining.saturating_sub(tasks.len());
            }
            plan.tasks = tasks;
            if max_rows.is_some() {
                load_open_attempts(&tx, plan, &members_json, team, &mut remaining)?;
            }
        }
        tx.commit()
            .map_err(db_error)
            .map_err(TeamCloseProjectionError::Store)?;
        Ok(plans)
    }
}

fn sql_row_limit(max_rows: Option<usize>, remaining: usize) -> i64 {
    if max_rows.is_none() {
        -1
    } else {
        remaining.saturating_add(1).min(i64::MAX as usize) as i64
    }
}

fn load_open_attempts(
    tx: &rusqlite::Transaction<'_>,
    plan: &mut TeamClosePlanRead,
    members_json: &str,
    team: &str,
    remaining: &mut usize,
) -> Result<(), TeamCloseProjectionError> {
    if plan.tasks.is_empty() {
        return Ok(());
    }
    let task_ids = plan
        .tasks
        .iter()
        .map(|task| task.id.clone())
        .collect::<Vec<_>>();
    let task_ids = serde_json::to_string(&task_ids)
        .map_err(super::super::json_error)
        .map_err(TeamCloseProjectionError::Store)?;
    let team_owned = plan.team_id.as_deref() == Some(team);
    let limit = remaining.saturating_add(1).min(i64::MAX as usize) as i64;
    let mut statement = tx
        .prepare_cached(
            "SELECT a.task_id,json_extract(a.row_json,'$.id'),a.executor_tab_id,
                json_type(a.row_json,'$.remote_worktree_path')
             FROM plan_task_attempts a
             WHERE a.plan_id=?1 AND a.task_id IN (SELECT value FROM json_each(?2))
               AND json_extract(a.row_json,'$.closed_at') IS NULL
               AND (?3 OR a.executor_tab_id IN (SELECT value FROM json_each(?4)))
             ORDER BY a.position LIMIT ?5",
        )
        .map_err(db_error)
        .map_err(TeamCloseProjectionError::Store)?;
    record_query();
    let rows = statement
        .query_map(
            rusqlite::params![plan.id, task_ids, team_owned, members_json, limit],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .map_err(db_error)
        .map_err(TeamCloseProjectionError::Store)?;
    let mut attempts_by_task =
        std::collections::HashMap::<String, Vec<TeamCloseOpenAttemptRead>>::new();
    let mut attempt_count = 0usize;
    for row in rows {
        let (task_id, id, executor_tab_id, remote_type) = row
            .map_err(db_error)
            .map_err(TeamCloseProjectionError::Store)?;
        attempt_count += 1;
        if attempt_count > *remaining {
            return Err(TeamCloseProjectionError::TooLarge);
        }
        let id = id.filter(|id| !id.is_empty()).ok_or_else(|| {
            TeamCloseProjectionError::Store(PlanError::Validation(
                "workspace close preview found an open attempt without an id".into(),
            ))
        })?;
        let remote = match remote_type.as_deref() {
            None | Some("null") => false,
            Some("text") => true,
            Some(_) => {
                return Err(TeamCloseProjectionError::Store(PlanError::Validation(
                    "workspace close preview found invalid remote attempt metadata".into(),
                )))
            }
        };
        attempts_by_task
            .entry(task_id)
            .or_default()
            .push(TeamCloseOpenAttemptRead {
                id,
                executor_tab_id,
                remote,
            });
    }
    *remaining = remaining.saturating_sub(attempt_count);
    for task in &mut plan.tasks {
        task.open_attempts = attempts_by_task.remove(&task.id).unwrap_or_default();
    }
    Ok(())
}

impl PlanStore {
    pub fn team_close_projection(
        &self,
        team: &str,
        members: &[String],
    ) -> Result<Vec<TeamClosePlanRead>, PlanError> {
        self.database.team_close_projection(team, members)
    }

    pub fn team_close_projection_bounded(
        &self,
        team: &str,
        members: &[String],
        max_rows: usize,
    ) -> Result<Vec<TeamClosePlanRead>, TeamCloseProjectionError> {
        self.database
            .team_close_projection_bounded(team, members, max_rows)
    }
}
