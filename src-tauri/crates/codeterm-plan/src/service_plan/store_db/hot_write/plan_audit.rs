use super::header_edits::append_field;
use super::*;
use crate::plan::TaskGroup;

pub(in crate::service_plan) enum PlanAudit {
    GroupClose { key: String },
    Retrospective { supersede_id: String },
}

impl PlanDatabase {
    pub(in crate::service_plan) fn append_plan_audit(
        &self,
        plan_id: &str,
        milestone: Milestone,
        change: PlanAudit,
        now_ms: u64,
    ) -> Result<CommittedRows, PlanError> {
        self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            match change {
                PlanAudit::GroupClose { key } => {
                    let declared: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM plans p,json_each(p.header_json,'$.task_groups') g WHERE p.id=?1 AND json_extract(g.value,'$.key')=?2)", params![plan_id, key], |row| row.get(0)).map_err(db_error)?;
                    if !declared {
                        let member: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM plan_tasks WHERE plan_id=?1 AND json_extract(row_json,'$.group')=?2)", params![plan_id, key], |row| row.get(0)).map_err(db_error)?;
                        if !member { return Err(PlanError::GroupNotFound); }
                        append_field(tx, plan_id, "$.task_groups", &TaskGroup { key: key.clone(), label: key, color: None }, receipt)?;
                    }
                }
                PlanAudit::Retrospective { supersede_id } => {
                    let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM plan_strides WHERE plan_id=?1 AND CASE WHEN json_extract(row_json,'$.kind')='retrospective' THEN CASE WHEN json_type(row_json,'$.text')='text' THEN trim(json_extract(row_json,'$.text'),?2)<>?3 ELSE json('invalid retrospective text') END ELSE 0 END)", params![plan_id, super::super::status_queries::WHITESPACE, crate::service_plan::retrospective::retrospective_supersede_note()], |row| row.get(0)).map_err(db_error)?;
                    if exists {
                        let mut supersede = milestone.clone();
                        supersede.id = supersede_id;
                        supersede.text = crate::service_plan::retrospective::retrospective_supersede_note().into();
                        append_stride(tx, plan_id, &supersede, receipt)?;
                    }
                }
            }
            append_stride(tx, plan_id, &milestone, receipt)
        })
    }
}

#[cfg(test)]
mod tests;
