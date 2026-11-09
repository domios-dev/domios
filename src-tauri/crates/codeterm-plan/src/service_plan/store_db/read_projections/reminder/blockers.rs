use super::{column, record_query, ReminderBlockerRead};
use crate::service_plan::PlanError;
use rusqlite::{params, Connection};
use std::collections::BTreeMap;

pub(super) fn read(
    connection: &Connection,
    id: &str,
    task_id: Option<&str>,
) -> Result<Vec<ReminderBlockerRead>, PlanError> {
    record_query();
    let facts = super::super::super::curator_notice_queries::reminder_task_facts();
    let sql=format!("{facts}, blockers(identity,task_id,summary,position,phase,seq) AS MATERIALIZED (
        SELECT domios_reminder_block('task_blocked',coalesce(nullif(trim(json_extract(row_json,'$.pause_reason'),?2),''),nullif(trim(json_extract(row_json,'$.status_label'),?2),''),task_id)),task_id,'Task '||task_id||' is blocked',position,0,0 FROM plan_tasks WHERE plan_id=?1 AND status='blocked'
        UNION ALL SELECT domios_reminder_block3('dependency',dep.value,coalesce(target.status,'missing')),t.task_id,'Dependency '||dep.value||' is not ready',t.position,1,CAST(dep.key AS INTEGER)
            FROM plan_tasks t,json_each(coalesce(t.row_json->'$.depends_on','[]')) dep LEFT JOIN dependency_targets target ON target.task_id=dep.value WHERE t.plan_id=?1 AND (target.task_id IS NULL OR target.status NOT IN ('done','cancelled'))
        UNION ALL SELECT domios_reminder_block('handoff',task_id),task_id,'Task '||task_id||' needs a handoff',position,2,0 FROM plan_tasks WHERE plan_id=?1 AND json_extract(row_json,'$.interruption.handoff_required')=1
        UNION ALL SELECT domios_reminder_block3('review_hold',t.task_id,coalesce(json_extract(t.row_json,'$.review_evidence[#-1].id'),'')),t.task_id,'Task '||t.task_id||' is held for review',t.position,3,0 FROM plan_tasks t JOIN facts f ON f.task_id=t.task_id WHERE t.plan_id=?1 AND json_extract(f.decision,'$.held')=1
        UNION ALL SELECT domios_reminder_block('machine_hold',json_extract(a.row_json,'$.interruption.reason')),a.task_id,'Worker on task '||a.task_id||' is interrupted',t.position,4,a.position FROM plan_task_attempts a JOIN plan_tasks t ON t.plan_id=a.plan_id AND t.task_id=a.task_id WHERE a.plan_id=?1 AND json_type(a.row_json,'$.interruption.reason') IS NOT NULL AND json_type(a.row_json,'$.interruption.reason')<>'null'
    ) SELECT identity,task_id,summary FROM blockers WHERE ?3 IS NULL OR identity IN(SELECT identity FROM blockers WHERE task_id=?3) ORDER BY position,phase,seq");
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(super::super::super::db_error)?;
    let rows = statement
        .query_map(
            params![id, super::super::super::status_queries::WHITESPACE, task_id],
            |row| {
                Ok((
                    column::<String>(row, 0)?,
                    column::<String>(row, 1)?,
                    column::<String>(row, 2)?,
                ))
            },
        )
        .map_err(super::super::super::db_error)?;
    let mut groups = BTreeMap::<String, ReminderBlockerRead>::new();
    for row in rows {
        let (identity, task_id, summary) = row.map_err(super::super::super::db_error)?;
        groups
            .entry(identity.clone())
            .or_insert_with(|| ReminderBlockerRead {
                identity,
                summary,
                task_ids: Default::default(),
            })
            .task_ids
            .insert(task_id);
    }
    Ok(groups.into_values().collect())
}
