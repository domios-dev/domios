use super::*;

impl PlanDatabase {
    pub(in crate::service_plan) fn task_assignment_projection(
        &self,
        plan: &str,
        task: &str,
    ) -> Result<Option<String>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection.query_row("SELECT json_extract(row_json,'$.assigned_tab_id') FROM plan_tasks WHERE plan_id=?1 AND task_id=?2",
            params![plan,task],|row|column(row,0)).optional().map(Option::flatten).map_err(db_error)
    }

    pub(in crate::service_plan) fn plan_skill_receipt_projection(
        &self,
        plan: &str,
        team: Option<&str>,
        tab: &str,
    ) -> Result<bool, PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection.query_row("SELECT CASE WHEN ?2 IS NOT NULL AND trim(?2,?4)<>'' AND EXISTS(
            SELECT 1 FROM plans p WHERE p.status='active' AND p.team_id=trim(?2,?4) AND p.id<>?1)
            THEN 0 ELSE EXISTS(SELECT 1 FROM plan_tasks t WHERE t.plan_id=?1
                AND CASE WHEN t.status IN ('done','cancelled') THEN 0
                    WHEN t.status IN ('pending','paused','in_progress','blocked','in_review') THEN 1
                    ELSE json('invalid task status') END
                AND (json_extract(t.row_json,'$.assigned_tab_id')=?3 OR EXISTS(
                    SELECT 1 FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id
                        AND json_extract(a.row_json,'$.executor_tab_id')=?3
                        AND json_extract(a.row_json,'$.closed_at') IS NULL AND json_extract(a.row_json,'$.retired_at') IS NULL))) END",
            params![plan,team,tab,super::super::status_queries::WHITESPACE],|row|column(row,0)).map_err(db_error)
    }
}
