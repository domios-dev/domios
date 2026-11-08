use super::*;
use crate::plan::{IdentityProvenance, TaskPriority};

pub(super) fn upsert(
    tx: &Transaction<'_>,
    plan_id: &str,
    op: BatchOp,
    result: &mut BatchResult,
    keys: &mut std::vec::IntoIter<String>,
    now_ms: u64,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    let BatchOp::UpsertTask {
        client_id,
        handle,
        name,
        kind,
        sections,
        status,
        body,
        model,
        why,
        priority,
        group,
        personality,
        personality_provenance: _,
    } = op
    else {
        return Err(PlanError::Io("expected task upsert".into()));
    };
    let personality = personality
        .map(|name| {
            crate::service_plan::worker_identity::validate_executor_name(&name)
                .map_err(PlanError::InvalidExecutorName)
        })
        .transpose()?;
    let missing_model = model.as_deref().is_none_or(str::is_empty);
    let missing_why = why.as_deref().is_none_or(str::is_empty);
    let missing_sections = sections.is_empty();
    let selector = reference(&handle, result);
    let existing = match batch_task(tx, plan_id, &selector, receipt) {
        Ok(id) => Some(id),
        Err(PlanError::TaskNotFound) => None,
        Err(error) => return Err(error),
    };
    let created = existing.is_none();
    let id = if let Some(id) = existing {
        let close_all = status.as_deref() == Some("done");
        task_fields::apply(
            tx,
            plan_id,
            &id,
            TaskFields {
                name,
                kind,
                model: model.map(Some),
                model_rationale: why.map(Some),
                status,
                priority,
                body,
                section_ids: (!sections.is_empty()).then_some(sections),
                group: group.map(Some),
                ..Default::default()
            },
            now_ms,
            receipt,
        )?;
        if close_all {
            review_rows::close_all_live(tx, plan_id, &id, now_ms, receipt)?;
        }
        if let Some(personality) = personality {
            let mut task = read_task(tx, plan_id, &id, receipt)?;
            if task.personality_provenance != Some(IdentityProvenance::UserSupplied) {
                task.personality = Some(personality);
                task.personality_provenance = Some(IdentityProvenance::Suggested);
                write_task(tx, plan_id, &task, receipt)?;
            }
        }
        id
    } else {
        let personality = codeterm_types::plan::require_executor_name(personality.as_deref())
            .map_err(PlanError::InvalidExecutorName)?;
        let personality = Some(personality);
        let id = crate::plan::ids::short_id("task");
        let status = status
            .map(|status| {
                serde_json::from_value::<TaskStatus>(serde_json::Value::String(status))
                    .map_err(|_| PlanError::BadStatus)
            })
            .transpose()?
            .unwrap_or(TaskStatus::Pending);
        let priority = priority
            .map(|priority| {
                serde_json::from_value::<TaskPriority>(serde_json::Value::String(priority))
                    .map_err(|_| PlanError::BadPriority)
            })
            .transpose()?
            .unwrap_or_default();
        let provenance = personality.as_ref().map(|_| IdentityProvenance::Suggested);
        let task:Task=serde_json::from_value(serde_json::json!({
            "id":id,"name":name.unwrap_or_else(||handle.clone()),"type":kind.unwrap_or_else(||"implement".into()),
            "status":status,"created_at":now_ms,"updated_at":now_ms,"status_changed_at":now_ms,"handle":handle,
            "section_ids":sections,"body":body.unwrap_or_default(),"model":model,"model_rationale":why,
            "priority":priority,"group":group,"key":keys.next(),"personality":personality,"personality_provenance":provenance,
        })).map_err(json_error)?;
        if let Some(group) = task.group.as_deref() {
            task_fields::declare_group(tx, plan_id, group, receipt)?;
        }
        task_fields::insert_task(tx, plan_id, &task, receipt)?;
        task_fields::normalize_handles(tx, plan_id, receipt)?;
        let task = read_task(tx, plan_id, &id, receipt)?;
        task_fields::queue_task_index(tx, plan_id, &task, false, false, receipt)?;
        id
    };
    if let Some(client_id) = client_id {
        result.ids.insert(client_id, id.clone());
    }
    result
        .hints
        .extend(crate::service_plan::hints::task_create_hints(
            plan_id,
            &id,
            !created || !missing_sections,
            !created || !missing_model,
            !created || !missing_why,
            true,
            true,
            true,
        ));
    push_once(&mut result.task_ids, id);
    Ok(())
}
