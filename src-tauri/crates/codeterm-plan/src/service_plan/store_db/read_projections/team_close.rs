use super::{column, decode, record_query};
use crate::plan::{PlanStatus, TaskStatus};
use crate::service_plan::store_db::{db_error, PlanDatabase};
use crate::service_plan::{PlanError, PlanStore};

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TeamClosePlanRead {
    pub id: String,
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
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TeamCloseLaneRead {
    pub tab_id: Option<String>,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub base: Option<String>,
    pub destination: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TeamCloseAttemptRead {
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub base: Option<String>,
    pub destination: Option<String>,
}

impl PlanDatabase {
    pub(in crate::service_plan) fn team_close_projection(
        &self,
        team: &str,
        members: &[String],
    ) -> Result<Vec<TeamClosePlanRead>, PlanError> {
        let predicate = crate::service_plan::store_queries::team_close_candidates(team, members);
        let (clause, params) = crate::service_plan::plan_match::compile(&predicate);
        let mut connection = self.connection.lock();
        let tx = connection.transaction().map_err(db_error)?;
        record_query();
        let mut statement = tx
            .prepare_cached(&format!(
                "SELECT p.id,json_object('id',p.id,'status',p.status,'team_id',p.team_id,
             'project_path',p.project_path,'canonical_project_path',
             CASE WHEN json_type(p.header_json,'$.canonical_project_path') IS NULL THEN ''
             ELSE json_extract(p.header_json,'$.canonical_project_path') END)
             FROM plans p WHERE {clause} ORDER BY p.updated_at DESC,p.id"
            ))
            .map_err(db_error)?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(params), |row| {
                Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
            })
            .map_err(db_error)?;
        let mut plans = rows
            .map(|row| {
                let (id, raw) = row.map_err(db_error)?;
                decode::<TeamClosePlanRead>(&id, &raw)
            })
            .collect::<Result<Vec<_>, PlanError>>()?;
        drop(statement);
        let members = serde_json::to_string(members).map_err(super::super::json_error)?;
        for plan in &mut plans {
            record_query();
            let mut statement = tx.prepare_cached(
                "WITH task_facts AS MATERIALIZED (
                    SELECT plan_id,task_id,position,status,assigned_tab_id,json_extract(row_json,'$.name') AS name,
                        json_extract(row_json,'$.lane.tab_id') AS lane_tab,
                        json_extract(row_json,'$.lane.worktree_path') AS worktree_path,
                        json_extract(row_json,'$.lane.branch') AS branch,
                        json_extract(row_json,'$.lane.base') AS base,
                        json_extract(row_json,'$.lane.destination') AS destination
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
                        'destination',t.destination),
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
                            'base',base,'destination',destination) AS document,min(position) AS first_position
                        FROM (SELECT a.position,
                            coalesce(json_extract(a.row_json,'$.worktree_path'),t.worktree_path) AS worktree_path,
                            coalesce(json_extract(a.row_json,'$.branch'),t.branch) AS branch,
                            coalesce(json_extract(a.row_json,'$.base'),t.base) AS base,
                            coalesce(json_extract(a.row_json,'$.destination'),t.destination) AS destination
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
                    OR (coalesce((SELECT invalid FROM remote_state WHERE task_id=t.task_id),0)>0) OR (
                    NOT (coalesce((SELECT present FROM remote_state WHERE task_id=t.task_id),0)>0)
                    AND (nullif(trim(t.worktree_path,?4),'') IS NOT NULL
                        OR nullif(trim(t.branch,?4),'') IS NOT NULL
                        OR EXISTS(SELECT 1 FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id
                            AND (?2 OR a.executor_tab_id IN (SELECT value FROM json_each(?3)))
                            AND (nullif(trim(json_extract(a.row_json,'$.worktree_path'),?4),'') IS NOT NULL
                                OR nullif(trim(json_extract(a.row_json,'$.branch'),?4),'') IS NOT NULL)))))
                 ORDER BY t.position"
            ).map_err(db_error)?;
            let rows = statement
                .query_map(
                    rusqlite::params![
                        plan.id,
                        plan.team_id.as_deref() == Some(team),
                        members,
                        super::super::status_queries::WHITESPACE,
                        plan.status == PlanStatus::Active
                    ],
                    |row| column::<String>(row, 0),
                )
                .map_err(db_error)?;
            plan.tasks = rows
                .map(|row| decode(&plan.id, &row.map_err(db_error)?))
                .collect::<Result<_, _>>()?;
        }
        tx.commit().map_err(db_error)?;
        Ok(plans)
    }
}

impl PlanStore {
    pub fn team_close_projection(
        &self,
        team: &str,
        members: &[String],
    ) -> Result<Vec<TeamClosePlanRead>, PlanError> {
        self.database.team_close_projection(team, members)
    }
}
