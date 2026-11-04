use super::*;
use crate::plan::{TaskGroup, TaskPriority, TaskStatus};
use crate::service_plan::{lane_metadata, mutations, qa, review_gate, tag_model, TaskLaneUpdate};

#[derive(Default)]
pub(in crate::service_plan) struct TaskFields {
    pub name: Option<String>,
    pub kind: Option<String>,
    pub model: Option<Option<String>>,
    pub model_rationale: Option<Option<String>>,
    pub status: Option<String>,
    pub status_label: Option<Option<String>>,
    pub priority: Option<String>,
    pub body: Option<String>,
    pub intent: Option<Option<String>>,
    pub executor_role: Option<Option<String>>,
    pub section_ids: Option<Vec<String>>,
    pub parent_task_id: Option<Option<String>>,
    pub group: Option<Option<String>>,
    pub tags: Vec<String>,
    pub remove_tags: Vec<String>,
    pub primary: Option<String>,
    pub handle: Option<String>,
    pub lane: Option<TaskLaneUpdate>,
}

pub(super) fn apply(
    tx: &Transaction<'_>,
    plan_id: &str,
    selector: &str,
    fields: TaskFields,
    now_ms: u64,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    let mut task = read_task(tx, plan_id, selector, receipt)?;
    let handles_changed = !fields.tags.is_empty()
        || !fields.remove_tags.is_empty()
        || fields.primary.is_some()
        || fields.handle.is_some()
        || task.handle.is_none();
    let previous_name = task.name.clone();
    let previous_status = task.status.clone();
    let status = fields
        .status
        .as_ref()
        .map(|value| {
            serde_json::from_value::<TaskStatus>(serde_json::Value::String(value.clone()))
                .map_err(|_| PlanError::BadStatus)
        })
        .transpose()?;
    if let Some(next) = &status {
        let current_tab = task
            .assigned_tab_id
            .as_deref()
            .or(task.lane.tab_id.as_deref())
            .is_some_and(|tab| !tab.trim().is_empty());
        let has_tab = fields
            .lane
            .as_ref()
            .and_then(|lane| lane.tab_id.as_ref())
            .map_or(current_tab, |tab| {
                tab.as_deref().is_some_and(|tab| !tab.trim().is_empty())
            });
        mutations::status_transition::validate(
            Some(&task),
            &task.status,
            next,
            fields.status_label.as_ref().map(|label| label.as_deref()),
            has_tab,
            |task| warn_review_backlog(tx, plan_id, task),
            || reject_open_subtasks(tx, plan_id, &task.id),
        )?;
    }
    if let Some(Some(group)) = &fields.group {
        declare_group(tx, plan_id, group, receipt)?;
    }
    if let Some(lane) = fields.lane {
        let explicit = lane.tab_id.is_some();
        lane_metadata::apply_lane_update(&mut task.lane, lane);
        task.lane = lane_metadata::normalize_lane_metadata(std::mem::take(&mut task.lane));
        if explicit {
            task.assigned_tab_id = task.lane.tab_id.clone();
        }
    }
    if let Some(value) = fields.name {
        task.name = value;
    }
    if let Some(value) = fields.kind {
        task.kind = value;
    }
    if let Some(value) = fields.model {
        task.model = value;
    }
    if let Some(value) = fields.model_rationale {
        task.model_rationale = value;
    }
    if let Some(next) = status {
        let entering = task.status != TaskStatus::InReview && next == TaskStatus::InReview;
        let closing = task.status == TaskStatus::InReview
            && matches!(next, TaskStatus::Done | TaskStatus::Cancelled);
        if task.status != next {
            task.status_changed_at = now_ms;
            task.status = next;
        }
        if task.status == TaskStatus::InProgress {
            tx.execute("UPDATE plans SET header_json=json_set(header_json, '$.mode', 'execution') WHERE id=?1", [plan_id]).map_err(db_error)?;
            receipt.header_rows += 1;
        }
        qa::normalize_task_review_fields(&mut task);
        if task.status != TaskStatus::Paused {
            task.pause_reason = None;
        }
        if matches!(task.status, TaskStatus::Done | TaskStatus::Cancelled) {
            close_assigned_attempts(tx, plan_id, &task, now_ms, receipt)?;
            mutations::clear_task_assignment(&mut task);
        }
        if entering {
            review_gate::fix_oracle_and_consume_override(&mut task, now_ms);
            change_review_credit(tx, plan_id, -1, receipt)?;
        } else if closing {
            change_review_credit(tx, plan_id, 1, receipt)?;
        }
    }
    if let Some(value) = fields.priority {
        task.priority = serde_json::from_value::<TaskPriority>(serde_json::Value::String(value))
            .map_err(|_| PlanError::BadPriority)?;
    }
    if let Some(value) = fields.status_label {
        task.status_label = value;
    }
    if let Some(value) = fields.body {
        task.body = value;
    }
    if let Some(value) = fields.intent {
        task.intent = value;
    }
    if let Some(value) = fields.executor_role {
        task.executor_role = value;
    }
    if let Some(value) = fields.section_ids {
        task.section_ids = value;
    }
    if let Some(value) = fields.parent_task_id {
        task.parent_task_id = value;
    }
    if let Some(value) = fields.group {
        task.group = value;
    }
    tag_model::apply_tags(&mut task.tags, &fields.tags, fields.primary.as_deref());
    task.tags.retain(|tag| !fields.remove_tags.contains(tag));
    if !fields.tags.is_empty() || !fields.remove_tags.is_empty() || fields.primary.is_some() {
        task.handle = None;
    }
    if let Some(value) = fields.handle {
        task.handle = Some(value);
    }
    task.updated_at = now_ms;
    write_task(tx, plan_id, &task, receipt)?;
    if handles_changed {
        normalize_handles(tx, plan_id, receipt)?;
    }
    queue_task_index(
        tx,
        plan_id,
        &task,
        previous_name != task.name,
        previous_status != task.status,
        receipt,
    )
}

pub(super) fn change_review_credit(
    tx: &Transaction<'_>,
    plan_id: &str,
    delta: i64,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    tx.execute("UPDATE plans SET header_json=json_set(header_json, '$.review_policy.credit', coalesce(json_extract(header_json, '$.review_policy.credit'),0)+?2) WHERE id=?1", params![plan_id, delta]).map_err(db_error)?;
    receipt.header_rows += 1;
    Ok(())
}

pub(super) fn reject_open_subtasks(
    tx: &Transaction<'_>,
    plan_id: &str,
    task_id: &str,
) -> Result<(), PlanError> {
    let mut statement = tx.prepare_cached("SELECT task_id, json_extract(row_json, '$.name'), status, json_extract(row_json, '$.handle') FROM plan_tasks WHERE plan_id=?1 AND json_extract(row_json, '$.parent_task_id')=?2 AND status NOT IN ('done','cancelled') ORDER BY position").map_err(db_error)?;
    let subtasks = statement
        .query_map(params![plan_id, task_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(db_error)?
        .map(|row| {
            let (id, name, status, handle) = row.map_err(db_error)?;
            Ok(crate::service_plan::OpenSubtask {
                id,
                name,
                status: serde_json::from_value(serde_json::Value::String(status))
                    .map_err(json_error)?,
                handle,
            })
        })
        .collect::<Result<Vec<_>, PlanError>>()?;
    if subtasks.is_empty() {
        Ok(())
    } else {
        Err(PlanError::OpenSubtasks { subtasks })
    }
}

pub(super) fn warn_review_backlog(
    tx: &Transaction<'_>,
    plan_id: &str,
    task: &Task,
) -> Result<(), PlanError> {
    let (count, limit, credit): (usize, u32, i64) = tx.query_row("SELECT (SELECT count(*) FROM plan_tasks WHERE plan_id=?1 AND status='in_review'), coalesce(json_extract(header_json,'$.review_policy.wip_limit'),5), coalesce(json_extract(header_json,'$.review_policy.credit'),0) FROM plans WHERE id=?1", [plan_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).map_err(db_error)?;
    if let Some(reason) = review_gate::review_gate_status_for(count, limit, credit).reason {
        log::warn!("[review-backlog] task={}: {}", task.id, reason);
    }
    Ok(())
}

pub(super) fn declare_group(
    tx: &Transaction<'_>,
    plan_id: &str,
    key: &str,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    if key.trim().is_empty() {
        return Ok(());
    }
    let json: String = tx.query_row("SELECT coalesce(json_extract(header_json,'$.task_groups'),'[]') FROM plans WHERE id=?1", [plan_id], |row| row.get(0)).map_err(db_error)?;
    receipt.decoded_bytes += json.len();
    let mut groups: Vec<TaskGroup> = serde_json::from_str(&json).map_err(json_error)?;
    if groups.iter().any(|group| group.key == key) {
        return Ok(());
    }
    groups.push(TaskGroup {
        key: key.into(),
        label: key.into(),
        color: None,
    });
    let json = encode(&groups, receipt)?;
    tx.execute(
        "UPDATE plans SET header_json=json_set(header_json,'$.task_groups',json(?2)) WHERE id=?1",
        params![plan_id, json],
    )
    .map_err(db_error)?;
    receipt.header_rows += 1;
    Ok(())
}

pub(super) fn close_assigned_attempts(
    tx: &Transaction<'_>,
    plan_id: &str,
    task: &Task,
    now_ms: u64,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    let Some(tab) = task
        .assigned_tab_id
        .as_deref()
        .map(str::trim)
        .filter(|tab| !tab.is_empty())
    else {
        return Ok(());
    };
    let mut statement = tx.prepare_cached("SELECT position, row_json FROM plan_task_attempts WHERE plan_id=?1 AND task_id=?2 AND executor_tab_id=?3 AND json_extract(row_json,'$.closed_at') IS NULL ORDER BY position").map_err(db_error)?;
    let rows = statement
        .query_map(params![plan_id, task.id, tab], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    for (position, json) in rows {
        receipt.decoded_bytes += json.len();
        let mut attempt: TaskExecutionAttempt = serde_json::from_str(&json).map_err(json_error)?;
        attempt.closed_at = Some(now_ms);
        let json = encode(&attempt, receipt)?;
        tx.execute("UPDATE plan_task_attempts SET row_json=?4, content_hash=?5 WHERE plan_id=?1 AND task_id=?2 AND position=?3", params![plan_id, task.id, position, json, store_rows::row_hash(&json)]).map_err(db_error)?;
        receipt.attempts += 1;
    }
    Ok(())
}

pub(super) fn normalize_handles(
    tx: &Transaction<'_>,
    plan_id: &str,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    let mut used = std::collections::HashSet::new();
    let mut statement = tx.prepare_cached("SELECT json_extract(j.value,'$.handle') FROM plan_sections s, json_tree(s.row_json) j WHERE s.plan_id=?1 AND j.type='object' AND (j.fullkey='$' OR j.path LIKE '%.subsections') AND json_extract(j.value,'$.handle') IS NOT NULL").map_err(db_error)?;
    for handle in statement
        .query_map([plan_id], |row| row.get::<_, String>(0))
        .map_err(db_error)?
    {
        used.insert(handle.map_err(db_error)?);
    }
    let mut statement = tx.prepare_cached("SELECT task_id, json_extract(row_json, '$.handle') FROM plan_tasks INDEXED BY plan_tasks_handle_order WHERE plan_id=?1 ORDER BY position").map_err(db_error)?;
    let rows = statement
        .query_map([plan_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    for (id, handle) in rows {
        receipt.projected_text_bytes += id.len() + handle.as_ref().map_or(0, String::len);
        let seed = match &handle {
            Some(handle) => handle.clone(),
            None => {
                let task = read_task(tx, plan_id, &id, receipt)?;
                tag_model::preview_handle(&task.tags, &task.name)
            }
        };
        let normalized = tag_model::unique_handle(seed, &mut used);
        if handle.as_deref() != Some(normalized.as_str()) {
            let mut task = read_task(tx, plan_id, &id, receipt)?;
            task.handle = Some(normalized);
            write_task(tx, plan_id, &task, receipt)?;
        }
    }
    Ok(())
}

pub(super) fn queue_task_index(
    tx: &Transaction<'_>,
    plan_id: &str,
    task: &Task,
    renamed: bool,
    restatused: bool,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    let identity: (String, String, String, String) = tx.query_row("SELECT title, status, project_path, coalesce(json_extract(header_json,'$.canonical_project_path'),'') FROM plans WHERE id=?1", [plan_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).map_err(db_error)?;
    receipt.projected_text_bytes +=
        identity.0.len() + identity.1.len() + identity.2.len() + identity.3.len();
    let mut delta = crate::service_plan::search_index::TaskIndexDelta {
        plan_id: plan_id.into(),
        title: identity.0,
        plan_status: identity.1,
        project_path: identity.2,
        canonical_project_path: identity.3,
        task: task.clone(),
        milestones: Vec::new(),
        milestone_status: Vec::new(),
    };
    if renamed || restatused {
        let sql = if renamed {
            "SELECT row_json FROM plan_task_milestones WHERE plan_id=?1 AND task_id=?2 ORDER BY seq"
        } else {
            "SELECT json_extract(row_json,'$.id') FROM plan_task_milestones WHERE plan_id=?1 AND task_id=?2 ORDER BY seq"
        };
        let mut statement = tx.prepare_cached(sql).map_err(db_error)?;
        for row in statement
            .query_map(params![plan_id, task.id], |row| row.get::<_, String>(0))
            .map_err(db_error)?
        {
            let json = row.map_err(db_error)?;
            if renamed {
                receipt.decoded_bytes += json.len();
                delta
                    .milestones
                    .push(serde_json::from_str(&json).map_err(json_error)?);
            } else {
                receipt.projected_text_bytes += json.len();
                delta.milestone_status.push(json);
            }
        }
    }
    receipt.task_search.push(delta);
    Ok(())
}

pub(super) fn insert_task(
    tx: &Transaction<'_>,
    plan_id: &str,
    task: &Task,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    text_bounds::validate_stored_task_text(task)?;
    let mut statement = tx.prepare_cached("SELECT json_extract(value,'$.key') FROM plans, json_each(header_json,'$.tag_facets') WHERE plans.id=?1").map_err(db_error)?;
    let facets = statement
        .query_map([plan_id], |row| row.get::<_, String>(0))
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    codeterm_types::plan::work_facet::validate_work_facet(
        facets.iter().map(String::as_str),
        task.tags.iter().map(String::as_str),
    )
    .map_err(PlanError::WorkFacetViolation)?;
    let mut value = serde_json::to_value(task).map_err(json_error)?;
    if let Some(fields) = value.as_object_mut() {
        fields.remove("milestones");
        fields.remove("execution_attempts");
    }
    let json = encode(&value, receipt)?;
    tx.execute("INSERT INTO plan_tasks(plan_id,task_id,position,updated_at,content_hash,milestone_count,milestone_digest,row_json) VALUES (?1,?2,(SELECT coalesce(max(position)+1,0) FROM plan_tasks WHERE plan_id=?1),?3,?4,0,0,?5)", params![plan_id, task.id, task.updated_at, store_rows::row_hash(&json), json]).map_err(db_error)?;
    receipt.tasks.insert(task.id.clone());
    receipt.task_rows += 1;
    Ok(())
}
