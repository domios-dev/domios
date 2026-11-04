use super::*;

impl PlanDatabase {
    pub(in crate::service_plan) fn worker_gate_claim_projection(
        &self,
        plan_id: &str,
        task_ref: &str,
        reporter: &str,
    ) -> Result<Option<Vec<String>>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        // A malformed authority epoch must fail even when it would exclude the gate row.
        let (plan_exists,task_exists,_epoch,invalid_epoch,raw): (bool,bool,Option<u64>,bool,Option<String>) = connection.query_row(
            &format!("{SELECTED_TASK_IN_PLAN}, gate_task AS MATERIALIZED (
                SELECT plan_id,task_id,assigned_tab_id,json_extract(row_json,'$.interruption.owner_epoch') AS owner_epoch FROM selected_task
             ) SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),
                EXISTS(SELECT 1 FROM selected_task),
                (SELECT coalesce(owner_epoch,0) FROM gate_task),
                EXISTS(SELECT 1 FROM plan_task_attempts a JOIN gate_task t ON t.plan_id=a.plan_id AND t.task_id=a.task_id
                    WHERE a.executor_tab_id=?3 AND json_extract(a.row_json,'$.closed_at') IS NULL
                        AND json_type(a.row_json,'$.interruption.owner_epoch') IS NOT NULL
                        AND (json_type(a.row_json,'$.interruption.owner_epoch')<>'integer' OR json_extract(a.row_json,'$.interruption.owner_epoch')<0)),
                (SELECT json_extract(a.row_json,'$.authorized_gates') FROM plan_task_attempts a
                    JOIN gate_task t ON t.plan_id=a.plan_id AND t.task_id=a.task_id
                    WHERE t.assigned_tab_id=?3 AND a.executor_tab_id=?3
                        AND json_extract(a.row_json,'$.closed_at') IS NULL
                        AND (coalesce(t.owner_epoch,0)=0
                            OR coalesce(json_extract(a.row_json,'$.interruption.owner_epoch'),0)=t.owner_epoch)
                    ORDER BY a.position DESC LIMIT 1)"),
            params![plan_id,task_ref,reporter],|row|Ok((column(row,0)?,column(row,1)?,column(row,2)?,column(row,3)?,column(row,4)?)),
        ).map_err(db_error)?;
        if !plan_exists {
            return Err(PlanError::NotFound);
        }
        if !task_exists {
            return Err(PlanError::TaskNotFound);
        }
        if invalid_epoch {
            return Err(PlanError::Io(
                "gate claim attempt has an invalid owner epoch".into(),
            ));
        }
        raw.map(|raw| decode(plan_id, &raw)).transpose()
    }
}
