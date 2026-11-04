use super::{column, decode, record_query};
use crate::plan::CuratorRound;
use crate::service_plan::{PlanError, PlanStore};
use rusqlite::OptionalExtension;

impl PlanStore {
    pub fn curator_skill_episodes_projection(
        &self,
        plan: &str,
        slug: &str,
    ) -> Result<Vec<String>, PlanError> {
        self.database.curator_skill_episodes_projection(plan, slug)
    }

    pub fn curator_real_rounds_projection(&self, plan: &str) -> Result<u64, PlanError> {
        self.database.curator_real_rounds_projection(plan)
    }

    pub fn curator_closed_tab_projection(
        &self,
        plan: &str,
        tab: &str,
    ) -> Result<(bool, Option<String>, Option<CuratorRound>), PlanError> {
        self.database.curator_closed_tab_projection(plan, tab)
    }
}

impl super::PlanDatabase {
    pub(in crate::service_plan) fn curator_skill_episodes_projection(
        &self,
        plan: &str,
        slug: &str,
    ) -> Result<Vec<String>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (exists, encoded): (bool,String) = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),
                (SELECT json_group_array(task_id) FROM (SELECT t.task_id FROM plan_tasks t
                    WHERE t.plan_id=?1 AND t.status='done' AND EXISTS(SELECT 1 FROM plan_task_attempts a,
                        json_each(a.row_json,'$.report.loaded_plan_skills') s
                        WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id AND json_extract(s.value,'$.slug')=?2)
                    ORDER BY t.position))",
            [plan,slug], |row| Ok((column(row,0)?,column(row,1)?)),
        ).map_err(super::super::db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        decode(plan, &encoded)
    }

    pub(in crate::service_plan) fn curator_real_rounds_projection(
        &self,
        plan: &str,
    ) -> Result<u64, PlanError> {
        let connection = self.connection.lock();
        super::super::curator_snapshot::register_functions(&connection)?;
        record_query();
        let (declared, recorded): (String,u64) = connection.query_row(
            "SELECT coalesce(header_json->'$.control_loop.real_rounds_started','0'),
                (SELECT count(*) FROM plan_strides WHERE plan_id=?1 AND json_extract(row_json,'$.kind')='curator_round'
                    AND domios_curator_round(json_extract(row_json,'$.text'))) FROM plans WHERE id=?1",
            [plan], |row| Ok((column(row,0)?,column(row,1)?)),
        ).optional().map_err(super::super::db_error)?.ok_or(PlanError::NotFound)?;
        let declared: u64 = decode(plan, &declared)?;
        Ok(declared.max(recorded))
    }

    pub(in crate::service_plan) fn curator_closed_tab_projection(
        &self,
        plan: &str,
        tab: &str,
    ) -> Result<(bool, Option<String>, Option<CuratorRound>), PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (running, run_id, round): (bool,Option<String>,Option<String>) = connection.query_row(
            "SELECT coalesce(json_extract(header_json,'$.control_loop.running.tab_id')=?2,0),
                CASE WHEN json_extract(header_json,'$.control_loop.running.tab_id')=?2 THEN json_extract(header_json,'$.control_loop.running.run_id') END,
                CASE WHEN coalesce(json_extract(header_json,'$.control_loop.running.tab_id')=?2,0)=0
                    AND json_extract(header_json,'$.control_loop.last_round.tab_id')=?2
                    THEN json_extract(header_json,'$.control_loop.last_round') END FROM plans WHERE id=?1",
            [plan,tab], |row| Ok((column(row,0)?,column(row,1)?,column(row,2)?)),
        ).optional().map_err(super::super::db_error)?.ok_or(PlanError::NotFound)?;
        Ok((
            running,
            run_id,
            round.map(|raw| decode(plan, &raw)).transpose()?,
        ))
    }
}
