use super::*;
use crate::plan::loop_engine::{advance, can_advance, record_iteration, stop_for};
use crate::plan::{AgentRef, LoopSpec};
use crate::service_plan::loop_mutations::LoopRecordResult;

pub(in crate::service_plan) enum LoopChange {
    Create(LoopSpec),
    Set {
        budget: Option<u32>,
        stop_when: Option<Option<String>>,
    },
    Stop,
    Advance {
        made_progress: bool,
    },
    Record {
        note: String,
        stop: bool,
        preferred_task_id: Option<String>,
        milestone_id: String,
    },
}

impl PlanDatabase {
    pub(in crate::service_plan) fn change_loop(
        &self,
        plan_id: &str,
        loop_id: &str,
        change: LoopChange,
        now_ms: u64,
    ) -> Result<(LoopSpec, Option<LoopRecordResult>, CommittedRows), PlanError> {
        let mut updated = None;
        let mut record = None;
        let receipt = self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            let (path, mut spec) = if let LoopChange::Create(spec) = &change {
                (None, spec.clone())
            } else {
                let (position, json): (i64, String) = tx.query_row(
                    "SELECT entry.key, entry.value FROM plans p, json_each(p.header_json,'$.loops') entry WHERE p.id=?1 AND json_extract(entry.value,'$.id')=?2 ORDER BY entry.key LIMIT 1",
                    params![plan_id, loop_id], |row| Ok((row.get(0)?, row.get(1)?)),
                ).optional().map_err(db_error)?.ok_or(PlanError::NotFound)?;
                receipt.decoded_bytes += json.len();
                (Some(format!("$.loops[{position}]")), serde_json::from_str::<LoopSpec>(&json).map_err(json_error)?)
            };
            match change {
                LoopChange::Create(_) => {}
                LoopChange::Set { budget, stop_when } => {
                    if let Some(budget) = budget {
                        if spec.count >= budget {
                            return Err(PlanError::BudgetExhausted { count: spec.count, budget });
                        }
                        spec.budget = budget;
                    }
                    if let Some(stop_when) = stop_when { spec.stop_when = stop_when; }
                }
                LoopChange::Stop => stop_for(&mut spec),
                LoopChange::Advance { made_progress } => {
                    require_advance(&spec)?;
                    let vector: String = tx.query_row("SELECT coalesce(json_extract(header_json,'$.goal.vector'),'') FROM plans WHERE id=?1", [plan_id], |row| row.get(0)).map_err(db_error)?;
                    receipt.projected_text_bytes += vector.len();
                    advance(&mut spec);
                    record_iteration(&mut spec, made_progress, &vector);
                    spec.last_iteration_at = Some(now_ms);
                }
                LoopChange::Record { note, stop, preferred_task_id, milestone_id } => {
                    let task_id = loop_task(tx, plan_id, preferred_task_id.as_deref(), receipt)?;
                    require_advance(&spec)?;
                    let kind: String = tx.query_row("SELECT CASE WHEN EXISTS(SELECT 1 FROM json_each(header_json,'$.milestone_kinds') WHERE json_extract(value,'$.key')='decision') THEN 'decision' ELSE 'status_update' END FROM plans WHERE id=?1", [plan_id], |row| row.get(0)).map_err(db_error)?;
                    receipt.projected_text_bytes += kind.len();
                    declare_kind(tx, plan_id, &kind, true, receipt)?;
                    advance(&mut spec);
                    if stop { stop_for(&mut spec); }
                    spec.last_iteration_at = Some(now_ms);
                    let mut task = read_task(tx, plan_id, &task_id, receipt)?;
                    task.updated_at = now_ms;
                    write_task(tx, plan_id, &task, receipt)?;
                    append_milestone(tx, plan_id, &task_id, &Milestone {
                        id: milestone_id.clone(), from: AgentRef::from_tab("loop-recorder"), kind,
                        text: note.trim().into(), ts: now_ms, links: vec![], loop_id: Some(spec.id.clone()),
                        iteration: Some(spec.count), canvas: Default::default(), attachments: vec![],
                    }, receipt)?;
                    task_fields::queue_task_index(tx, plan_id, &task, false, false, receipt)?;
                    record = Some(LoopRecordResult { plan_id: plan_id.into(), loop_id: spec.id.clone(), task_id,
                        milestone_id, count: spec.count, budget: spec.budget, status: spec.status.clone() });
                }
            }
            let document = encode(&spec, receipt)?;
            receipt.header_rows += if let Some(path) = path {
                tx.execute("UPDATE plans SET header_json=json_set(header_json,?2,json(?3)) WHERE id=?1", params![plan_id, path, document])
            } else {
                tx.execute("UPDATE plans SET header_json=json_set(header_json,'$.loops',json_insert(coalesce(json_extract(header_json,'$.loops'),'[]'),'$[#]',json(?2))) WHERE id=?1", params![plan_id, document])
            }.map_err(db_error)?;
            updated = Some(spec);
            Ok(())
        })?;
        Ok((updated.ok_or(PlanError::NotFound)?, record, receipt))
    }
}

fn require_advance(spec: &LoopSpec) -> Result<(), PlanError> {
    if can_advance(spec) {
        Ok(())
    } else {
        Err(PlanError::BudgetExhausted {
            count: spec.count,
            budget: spec.budget,
        })
    }
}

fn loop_task(
    tx: &Transaction<'_>,
    plan_id: &str,
    preferred: Option<&str>,
    receipt: &mut CommittedRows,
) -> Result<String, PlanError> {
    if let Some(preferred) = preferred {
        match resolve_task(tx, plan_id, preferred, receipt) {
            Ok(id) => return Ok(id),
            Err(PlanError::TaskNotFound) => {}
            Err(error) => return Err(error),
        }
    }
    let id: String = tx.query_row(
        "SELECT task_id FROM plan_tasks WHERE plan_id=?1 AND status NOT IN ('done','cancelled') ORDER BY (json_extract(row_json,'$.type')='planning') DESC, updated_at DESC, position DESC LIMIT 1",
        [plan_id], |row| row.get(0),
    ).optional().map_err(db_error)?.ok_or(PlanError::TaskNotFound)?;
    receipt.projected_text_bytes += id.len();
    Ok(id)
}

#[cfg(test)]
mod tests;
