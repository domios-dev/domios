use super::{column, decode, record_query};
use crate::plan::{GateStatus, OrchestratorRef, PlanMode, PlannerRef};
use crate::service_plan::{PlanError, PlanStore};
use rusqlite::OptionalExtension;

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct PlannerRehydrateGateRead {
    pub id: String,
    pub status: GateStatus,
    pub question: String,
    pub asked_by_tab_id: Option<String>,
    pub routed_to_tab_id: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct PlannerRehydrateRead {
    pub id: String,
    pub owner: OrchestratorRef,
    pub planner: Option<PlannerRef>,
    pub open_count: usize,
    pub gates: Vec<PlannerRehydrateGateRead>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TaskReviewNoticeRead {
    pub owner: Option<String>,
    pub task_name: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct GateAnswerNoticeRead {
    pub mode: PlanMode,
    pub asker: Option<String>,
}

#[derive(serde::Deserialize)]
struct RelinkTargetsRead {
    workers: Vec<String>,
    planner: Option<String>,
}

impl PlanStore {
    pub fn hierarchy_planner_bindings_projection(
        &self,
        tab: &str,
        bound_session_only: bool,
    ) -> Result<Vec<(String, Option<String>)>, PlanError> {
        self.database
            .hierarchy_planner_bindings_projection(tab, bound_session_only)
    }
    pub fn planner_rehydrate_projection(
        &self,
        plan: &str,
    ) -> Result<PlannerRehydrateRead, PlanError> {
        self.database.planner_rehydrate_projection(plan)
    }
    pub fn task_review_notice_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<TaskReviewNoticeRead, PlanError> {
        self.database.task_review_notice_projection(plan, task)
    }
    pub fn gate_answer_notice_projection(
        &self,
        plan: &str,
        gate: &str,
    ) -> Result<GateAnswerNoticeRead, PlanError> {
        self.database.gate_answer_notice_projection(plan, gate)
    }
    pub fn relink_targets_projection(
        &self,
        plan: &str,
        include_planner: bool,
    ) -> Result<Vec<String>, PlanError> {
        self.database
            .relink_targets_projection(plan, include_planner)
    }
}

impl super::PlanDatabase {
    pub(in crate::service_plan) fn hierarchy_planner_bindings_projection(
        &self,
        tab: &str,
        bound_session_only: bool,
    ) -> Result<Vec<(String, Option<String>)>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let mut statement = connection.prepare_cached(
            "SELECT id,CASE WHEN ?2 THEN json_extract(header_json,'$.planner.title') END FROM plans
             WHERE json_extract(header_json,'$.planner.kind')='tab' AND json_extract(header_json,'$.planner.tab_id')=?1
                AND (?2=0 OR json_extract(header_json,'$.planner.session_id') IS NOT NULL)
             ORDER BY updated_at DESC,id"
        ).map_err(super::super::db_error)?;
        let rows = statement
            .query_map(rusqlite::params![tab, bound_session_only], |row| {
                Ok((column(row, 0)?, column(row, 1)?))
            })
            .map_err(super::super::db_error)?;
        rows.collect::<Result<_, _>>()
            .map_err(super::super::db_error)
    }

    pub(in crate::service_plan) fn planner_rehydrate_projection(
        &self,
        plan: &str,
    ) -> Result<PlannerRehydrateRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let encoded: String = connection.query_row(
            "SELECT json_object('id',p.id,'owner',json_extract(p.header_json,'$.owner'),
                'planner',CASE WHEN json_extract(p.header_json,'$.planner.kind')='tab' THEN json_object('kind','tab','tab_id',json_extract(p.header_json,'$.planner.tab_id')) END,
                'open_count',(SELECT count(*) FROM json_each(p.header_json,'$.gates') g WHERE json_extract(g.value,'$.status') IN ('open','escalated')),
                'gates',json((SELECT json_group_array(json(document)) FROM (SELECT json_object(
                    'id',json_extract(g.value,'$.id'),'status',json_extract(g.value,'$.status'),
                    'question',json_extract(g.value,'$.question'),'asked_by_tab_id',json_extract(g.value,'$.asked_by_tab_id'),
                    'routed_to_tab_id',json_extract(g.value,'$.routed_to_tab_id')) AS document
                    FROM json_each(p.header_json,'$.gates') g WHERE json_extract(g.value,'$.status') IN ('open','escalated') ORDER BY g.key LIMIT 10))))
             FROM plans p WHERE p.id=?1",
            [plan], |row| column(row,0),
        ).optional().map_err(super::super::db_error)?.ok_or(PlanError::NotFound)?;
        decode(plan, &encoded)
    }

    pub(in crate::service_plan) fn task_review_notice_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<TaskReviewNoticeRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let encoded: String = connection.query_row(
            "SELECT json_object('owner',json_extract(p.header_json,'$.owner.tab_id'),
                'task_name',coalesce((SELECT json_extract(t.row_json,'$.name') FROM plan_tasks t WHERE t.plan_id=p.id AND t.task_id=?2 ORDER BY t.position LIMIT 1),?2)) FROM plans p WHERE p.id=?1",
            [plan,task], |row| column(row,0),
        ).optional().map_err(super::super::db_error)?.ok_or(PlanError::NotFound)?;
        decode(plan, &encoded)
    }

    pub(in crate::service_plan) fn gate_answer_notice_projection(
        &self,
        plan: &str,
        gate: &str,
    ) -> Result<GateAnswerNoticeRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let encoded: String = connection.query_row(
            "SELECT json_object('mode',coalesce(json_extract(p.header_json,'$.mode'),'planning'),
                'asker',coalesce((SELECT json_extract(g.value,'$.asked_by_tab_id') FROM json_each(p.header_json,'$.gates') g WHERE json_extract(g.value,'$.id')=?2 ORDER BY g.key LIMIT 1),json_extract(p.header_json,'$.owner.tab_id'))) FROM plans p WHERE p.id=?1",
            [plan,gate], |row| column(row,0),
        ).optional().map_err(super::super::db_error)?.ok_or(PlanError::NotFound)?;
        decode(plan, &encoded)
    }

    pub(in crate::service_plan) fn relink_targets_projection(
        &self,
        plan: &str,
        include_planner: bool,
    ) -> Result<Vec<String>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let encoded: String = connection.query_row(
            "SELECT json_object('workers',json((SELECT json_group_array(assigned_tab_id) FROM (SELECT assigned_tab_id FROM plan_tasks t WHERE t.plan_id=p.id AND status='in_progress' AND assigned_tab_id IS NOT NULL ORDER BY position))),
                'planner',CASE WHEN ?2 AND json_extract(p.header_json,'$.planner.kind')='tab' THEN json_extract(p.header_json,'$.planner.tab_id') END) FROM plans p WHERE p.id=?1",
            rusqlite::params![plan,include_planner], |row| column(row,0),
        ).optional().map_err(super::super::db_error)?.ok_or(PlanError::NotFound)?;
        let targets: RelinkTargetsRead = decode(plan, &encoded)?;
        let mut selected = Vec::new();
        for target in targets.workers.into_iter().chain(targets.planner) {
            let target = target.trim();
            if !target.is_empty() && !selected.iter().any(|prior| prior == target) {
                selected.push(target.to_string());
            }
        }
        Ok(selected)
    }
}
