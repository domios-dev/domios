use super::{column, decode, record_query};
use crate::plan::{OrchestratorRef, PlanEvidenceReporterGrant, PlannerRef};
use crate::service_plan::{PlanError, PlanStore};
use rusqlite::OptionalExtension;

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct HierarchyAuthorityRead {
    pub owner: OrchestratorRef,
    pub planner: Option<PlannerRef>,
    pub team_id: Option<String>,
    pub evidence_reporters: Vec<PlanEvidenceReporterGrant>,
}

impl PlanStore {
    pub fn hierarchy_planner_projection(&self, id: &str) -> Result<Option<PlannerRef>, PlanError> {
        self.database.hierarchy_planner_projection(id)
    }

    pub fn report_tab_holds_task_projection(
        &self,
        plan: &str,
        task: &str,
        tab: &str,
    ) -> Result<bool, PlanError> {
        self.database
            .report_tab_holds_task_projection(plan, task, tab)
    }

    pub fn hierarchy_authority_projection(
        &self,
        id: &str,
        tab: &str,
    ) -> Result<HierarchyAuthorityRead, PlanError> {
        self.database.hierarchy_authority_projection(id, tab)
    }
}

impl super::PlanDatabase {
    pub(in crate::service_plan) fn hierarchy_planner_projection(
        &self,
        id: &str,
    ) -> Result<Option<PlannerRef>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let encoded: String = connection
            .query_row(
                "SELECT json_quote(json_extract(header_json,'$.planner')) FROM plans WHERE id=?1",
                [id],
                |row| column(row, 0),
            )
            .optional()
            .map_err(super::super::db_error)?
            .ok_or(PlanError::NotFound)?;
        decode(id, &encoded)
    }

    pub(in crate::service_plan) fn report_tab_holds_task_projection(
        &self,
        plan: &str,
        task: &str,
        tab: &str,
    ) -> Result<bool, PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection
            .query_row(
                &format!(
                    "{} SELECT EXISTS(SELECT 1 FROM selected_task t
                WHERE t.assigned_tab_id=?3 OR EXISTS(SELECT 1 FROM plan_task_attempts a
                    WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id AND a.executor_tab_id=?3
                        AND json_extract(a.row_json,'$.closed_at') IS NULL
                        AND json_extract(a.row_json,'$.retired_at') IS NULL
                        AND json_extract(a.row_json,'$.competition_group_id') IS NOT NULL))",
                    super::super::task_authority_queries::SELECTED_TASK_IN_PLAN
                ),
                rusqlite::params![plan, task, tab],
                |row| column(row, 0),
            )
            .map_err(super::super::db_error)
    }

    pub(in crate::service_plan) fn hierarchy_authority_projection(
        &self,
        id: &str,
        tab: &str,
    ) -> Result<HierarchyAuthorityRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let encoded: String = connection
            .query_row(
                "SELECT json_object('owner',json_extract(header_json,'$.owner'),
                'planner',json_extract(header_json,'$.planner'),'team_id',team_id,
                'evidence_reporters',json((SELECT json_group_array(json(value)) FROM
                    (SELECT value FROM json_each(header_json,'$.evidence_reporters')
                        WHERE json_extract(value,'$.tab_id')=?2 ORDER BY key))))
             FROM plans WHERE id=?1",
                [id, tab],
                |row| column(row, 0),
            )
            .optional()
            .map_err(super::super::db_error)?
            .ok_or(PlanError::NotFound)?;
        decode(id, &encoded)
    }
}
