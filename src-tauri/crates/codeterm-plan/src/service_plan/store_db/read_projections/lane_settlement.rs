use super::{column, decode, record_query};
use crate::service_plan::store_db::{db_error, PlanDatabase};
use crate::service_plan::{PlanError, PlanStore};
use rusqlite::params;

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct LaneSettlementRead {
    pub plan_id: String,
    pub project_path: String,
    pub canonical_project_path: String,
    pub task_id: String,
    pub status: crate::plan::TaskStatus,
    pub bound_tabs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attempt_peer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleanup_status: Option<String>,
}

impl LaneSettlementRead {
    pub fn canonical_path(&self) -> &str {
        if self.canonical_project_path.trim().is_empty() {
            &self.project_path
        } else {
            &self.canonical_project_path
        }
    }
}

impl PlanDatabase {
    pub(in crate::service_plan) fn lane_settlement_projection(
        &self,
        plan: Option<&str>,
        task: Option<&str>,
        tab: Option<&str>,
    ) -> Result<Vec<LaneSettlementRead>, PlanError> {
        let connection = self.connection.lock();
        lane_settlement_rows(&connection, plan, task, tab, |_| {})
    }
}

pub(in crate::service_plan::store_db) fn lane_settlement_rows(
    connection: &rusqlite::Connection,
    plan: Option<&str>,
    task: Option<&str>,
    tab: Option<&str>,
    mut observe: impl FnMut(&str),
) -> Result<Vec<LaneSettlementRead>, PlanError> {
    record_query();
    let mut statement = connection.prepare_cached(
            "WITH eligible AS MATERIALIZED (
                SELECT DISTINCT h.plan_id FROM plan_tasks h
                WHERE ?3 IS NOT NULL AND (?1 IS NULL OR h.plan_id=?1)
                    AND h.status IN ('done','cancelled') AND (h.assigned_tab_id=?3
                        OR json_extract(h.row_json,'$.lane.tab_id')=?3
                        OR EXISTS(SELECT 1 FROM plan_task_attempts ha WHERE ha.plan_id=h.plan_id
                            AND ha.task_id=h.task_id AND ha.executor_tab_id=?3))
             ), selected_lane_plans AS MATERIALIZED (
                SELECT DISTINCT t.plan_id FROM plan_tasks t
                WHERE (?1 IS NULL OR t.plan_id=?1) AND (?2 IS NULL OR t.task_id=?2)
                    AND (?3 IS NULL OR t.plan_id IN (SELECT plan_id FROM eligible))
                    AND t.status IN ('done','cancelled')
                    AND (json_extract(t.row_json,'$.lane.worktree_path') IS NOT NULL OR json_extract(t.row_json,'$.lane.branch') IS NOT NULL)
             ), plan_scope AS MATERIALIZED (
                SELECT p.id,p.project_path,p.updated_at,coalesce(json_extract(p.header_json,'$.canonical_project_path'),'') AS canonical_project_path
                FROM plans p WHERE p.id IN (SELECT plan_id FROM selected_lane_plans)
             ) SELECT p.id,json_object('plan_id',p.id,'project_path',p.project_path,
                'canonical_project_path',p.canonical_project_path,
                'task_id',t.task_id,'status',t.status,
                'bound_tabs',json((SELECT json_group_array(tab) FROM (
                    SELECT json_extract(t.row_json,'$.lane.tab_id') AS tab
                    UNION ALL SELECT t.assigned_tab_id
                    UNION ALL SELECT executor_tab_id FROM (SELECT executor_tab_id FROM plan_task_attempts a
                        WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id ORDER BY a.position)) WHERE tab IS NOT NULL)),
                'peer',json_extract(t.row_json,'$.peer'),
                'last_attempt_peer',(SELECT json_extract(a.row_json,'$.mesh_peer') FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id ORDER BY a.position DESC LIMIT 1),
                'worktree',json_extract(t.row_json,'$.lane.worktree_path'),
                'branch',json_extract(t.row_json,'$.lane.branch'),
                'destination',json_extract(t.row_json,'$.lane.destination'),
                'cleanup_status',json_extract(t.row_json,'$.lane.cleanup_status'))
             FROM plan_scope p JOIN plan_tasks t ON t.plan_id=p.id
             WHERE (?1 IS NULL OR p.id=?1) AND (?2 IS NULL OR t.task_id=?2)
                AND (?3 IS NULL OR p.id IN (SELECT plan_id FROM eligible))
                AND t.status IN ('done','cancelled')
                AND (json_extract(t.row_json,'$.lane.worktree_path') IS NOT NULL OR json_extract(t.row_json,'$.lane.branch') IS NOT NULL)
             ORDER BY p.updated_at DESC,p.id,t.position"
        ).map_err(db_error)?;
    let rows = statement
        .query_map(params![plan, task, tab], |row| {
            Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
        })
        .map_err(db_error)?;
    rows.map(|row| {
        let (id, raw) = row.map_err(db_error)?;
        observe(&raw);
        decode(&id, &raw)
    })
    .collect()
}

impl PlanStore {
    pub fn lane_settlement_projection(
        &self,
        plan: Option<&str>,
        task: Option<&str>,
    ) -> Result<Vec<LaneSettlementRead>, PlanError> {
        self.database.lane_settlement_projection(plan, task, None)
    }
    pub fn held_lane_settlement_projection(
        &self,
        tab: &str,
    ) -> Result<Vec<LaneSettlementRead>, PlanError> {
        self.database
            .lane_settlement_projection(None, None, Some(tab))
    }
}
