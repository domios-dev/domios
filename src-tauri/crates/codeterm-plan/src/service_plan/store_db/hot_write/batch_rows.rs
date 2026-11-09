use super::*;
mod handoff;
mod plan;
mod specs;
mod tasks;
use crate::plan::{AgentRef, TaskStatus};
use crate::service_plan::batch::{BatchOp, BatchRequest, BatchResult};

impl PlanDatabase {
    pub(in crate::service_plan) fn commit_hot_batch(
        &self,
        plan_id: &str,
        request: BatchRequest,
        keys: Vec<String>,
        now_ms: u64,
    ) -> Result<(BatchResult, CommittedRows), PlanError> {
        let mut result = BatchResult::default();
        let mut keys = keys.into_iter();
        let receipt = self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            for op in request.ops {
                match op {
                    BatchOp::SetTaskBody { task, body } => {
                        text_bounds::validate_plan_text("task body", &body)?;
                        let id = batch_task(tx, plan_id, &reference(&task, &result), receipt)?;
                        task_fields::apply(tx, plan_id, &id, TaskFields { body: Some(body), ..Default::default() }, now_ms, receipt)?;
                        push_once(&mut result.task_ids, id);
                    }
                    BatchOp::SetTaskSections { client_id, task, sections } => {
                        let id = batch_task(tx, plan_id, &reference(&task, &result), receipt)?;
                        for section in &sections { text_bounds::validate_plan_text("section id", section)?; }
                        task_fields::apply(tx, plan_id, &id, TaskFields { section_ids: Some(sections), ..Default::default() }, now_ms, receipt)?;
                        if let Some(client_id) = client_id { result.ids.insert(client_id, id.clone()); }
                        push_once(&mut result.task_ids, id);
                    }
                    BatchOp::SetTaskStatus { client_id, task, status, status_label } => {
                        let next: TaskStatus = serde_json::from_value(serde_json::Value::String(status)).map_err(|_| PlanError::BadStatus)?;
                        let id = batch_task(tx, plan_id, &reference(&task, &result), receipt)?;
                        let mut task = read_task(tx, plan_id, &id, receipt)?;
                        let current = task.status.clone();
                        let label = status_label.filter(|label| !label.trim().is_empty());
                        if let Some(label) = &label { text_bounds::validate_plan_text("status label", label)?; }
                        crate::service_plan::mutations::status_transition::validate(Some(&task), &current, &next, label.as_deref().map(Some),
                            task.assigned_tab_id.as_deref().or(task.lane.tab_id.as_deref()).is_some_and(|tab| !tab.trim().is_empty()),
                            |task| task_fields::warn_review_backlog(tx, plan_id, task), || task_fields::reject_open_subtasks(tx, plan_id, &task.id))?;
                        if next == TaskStatus::Done {
                            task_fields::reject_open_subtasks(tx, plan_id, &task.id)?;
                            review_rows::close_all_live(tx, plan_id, &task.id, now_ms, receipt)?;
                            crate::service_plan::mutations::clear_task_assignment(&mut task);
                        }
                        let entering = current != TaskStatus::InReview && next == TaskStatus::InReview;
                        let closing = current == TaskStatus::InReview && matches!(next, TaskStatus::Done | TaskStatus::Cancelled);
                        if current != next { task.status_changed_at = now_ms; task.status = next; }
                        if entering {
                            crate::service_plan::review_gate::fix_oracle_and_consume_override(&mut task, now_ms);
                            task_fields::change_review_credit(tx, plan_id, -1, receipt)?;
                        } else if closing {
                            task_fields::change_review_credit(tx, plan_id, 1, receipt)?;
                        }
                        if let Some(label) = label { task.status_label = Some(label); }
                        crate::service_plan::qa::normalize_task_review_fields(&mut task);
                        task.updated_at = now_ms;
                        write_task(tx, plan_id, &task, receipt)?;
                        task_fields::queue_task_index(tx, plan_id, &task, false, current != task.status, receipt)?;
                        if let Some(client_id) = client_id { result.ids.insert(client_id, id.clone()); }
                        push_once(&mut result.task_ids, id);
                    }
                    BatchOp::AddMilestone { client_id, task, kind, text, from, key, links } => {
                        let selector = reference(&task, &result);
                        declare_kind(tx, plan_id, &kind, true, receipt)?;
                        let id = batch_task(tx, plan_id, &selector, receipt)?;
                        let key = key.as_deref().or(request.key.as_deref()).map(|key| format!("key:{key}"));
                        let existing: Option<String> = if let Some(key) = &key {
                            tx.query_row("SELECT json_extract(m.row_json,'$.id') FROM plan_task_milestones m WHERE plan_id=?1 AND task_id=?2 AND json_extract(row_json,'$.kind')=?3 AND EXISTS(SELECT 1 FROM json_each(m.row_json,'$.links') WHERE value=?4) ORDER BY seq LIMIT 1", params![plan_id, id, kind, key], |row| row.get(0)).optional().map_err(db_error)?
                        } else { None };
                        let milestone_id = if let Some(existing) = existing { existing } else {
                            text_bounds::validate_plan_label("milestone kind", &kind)?;
                            text_bounds::validate_plan_text("milestone text", &text)?;
                            let milestone_id = crate::plan::ids::short_id("ms");
                            let mut links: Vec<String> = links.iter().map(|link| reference(link, &result)).collect();
                            if let Some(key) = key { links.push(key); }
                            let milestone = Milestone { id: milestone_id.clone(), from: from.unwrap_or_else(|| AgentRef::from_tab("unknown")), kind, text, ts: now_ms, links, loop_id: None, iteration: None, canvas: Default::default(), attachments: vec![] };
                            let mut task = read_task(tx, plan_id, &id, receipt)?;
                            task.updated_at = now_ms;
                            write_task(tx, plan_id, &task, receipt)?;
                            append_milestone(tx, plan_id, &id, &milestone, receipt)?;
                            task_fields::queue_task_index(tx, plan_id, &task, false, false, receipt)?;
                            milestone_id
                        };
                        if let Some(client_id) = client_id { result.ids.insert(client_id, milestone_id.clone()); }
                        push_once(&mut result.milestone_ids, milestone_id);
                    }
                    op @ BatchOp::UpsertTask { .. } => tasks::upsert(tx, plan_id, op, &mut result, &mut keys, now_ms, receipt)?,
                    op @ (BatchOp::SetGoal { .. } | BatchOp::Link { .. } | BatchOp::IngestSections { .. } | BatchOp::AddElement { .. }) => plan::apply(tx, plan_id, op, &mut result, now_ms, receipt)?,
                    op @ BatchOp::AddHandoff { .. } => handoff::apply(tx, plan_id, op, &mut result, now_ms, receipt)?,
                    op @ (BatchOp::UpsertSpec { .. } | BatchOp::UpsertSpecSection { .. } | BatchOp::SetSpecSectionBody { .. } | BatchOp::LinkSpecSection { .. } | BatchOp::UnlinkSpecSection { .. } | BatchOp::CreatePlanFromSpec { .. }) => specs::apply(tx, plan_id, op, &mut result, now_ms, receipt)?,
                }
            }
            Ok(())
        })?;
        if let Some(hint) = crate::service_plan::hints::maybe_orchestration_reminder() {
            result.hints.push(hint);
        }
        Ok((result, receipt))
    }
}

fn reference(value: &str, result: &BatchResult) -> String {
    value
        .strip_prefix('$')
        .and_then(|key| result.ids.get(key))
        .cloned()
        .unwrap_or_else(|| value.into())
}

fn batch_task(
    tx: &Transaction<'_>,
    plan_id: &str,
    selector: &str,
    receipt: &mut CommittedRows,
) -> Result<String, PlanError> {
    if selector == "@latest" {
        return tx.query_row("SELECT task_id FROM plan_tasks WHERE plan_id=?1 ORDER BY updated_at DESC, coalesce(json_extract(row_json,'$.created_at'),0) DESC, position DESC LIMIT 1", [plan_id], |row| row.get(0)).optional().map_err(db_error)?.ok_or(PlanError::TaskNotFound);
    }
    resolve_task(tx, plan_id, selector, receipt)
}

fn push_once(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

#[cfg(test)]
mod tests;
