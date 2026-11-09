use super::{db_error, json_error, PlanDatabase};
use crate::plan::{
    IdentityProvenance, Milestone, MilestoneKind, ReviewOverride, Task, TaskExecutionAttempt,
};
use crate::service_plan::{store_rows, text_bounds, PlanError};
use rusqlite::{params, OptionalExtension, Transaction};
use std::collections::BTreeSet;
mod batch_rows;
mod batch_sidecars;
mod dispatch_rows;
mod element_rows;
mod gate_rows;
pub(in crate::service_plan) mod handoff;
mod header_edits;
mod ingest_rows;
mod lane_history;
pub(in crate::service_plan) mod lifecycle_rows;
mod loop_rows;
mod milestones;
mod plan_audit;
mod plan_fields;
mod plan_state;
mod relationship_rows;
pub(in crate::service_plan) mod review_header;
mod review_rows;
pub(in crate::service_plan) mod runtime;
mod section_rows;
mod spec_rows;
mod storage_rows;
mod task_fields;
mod task_structure;
pub(in crate::service_plan) use element_rows::ElementChange;
pub(in crate::service_plan) use gate_rows::{GateAsk, GateChange};
pub(in crate::service_plan) use header_edits::HeaderChange;
pub(in crate::service_plan) use loop_rows::LoopChange;
pub(in crate::service_plan) use plan_audit::PlanAudit;
pub(in crate::service_plan) use plan_state::PlanEdit;
pub(in crate::service_plan) use review_rows::{TaskHistory, TaskRowEdit};
pub(in crate::service_plan) use section_rows::SectionChange;
pub(in crate::service_plan) use spec_rows::{SpecLinkChange, SpecPlanCreate};
pub(in crate::service_plan) use storage_rows::DeletedPlanRows;
pub(in crate::service_plan) use task_fields::TaskFields;

#[derive(Default)]
pub(in crate::service_plan) struct AttemptTarget<'a> {
    pub tab: &'a str,
    pub id: Option<&'a str>,
    pub session: Option<(&'a str, &'a str)>,
    pub unretired: bool,
    pub live: bool,
    pub model_unknown: bool,
}

pub(in crate::service_plan) enum TaskMetadata {
    RoleGuidance(Option<Option<String>>),
    Personality {
        name: Option<Option<String>>,
        provenance: Option<Option<IdentityProvenance>>,
    },
    ReviewOverride(ReviewOverride),
}

pub(in crate::service_plan) enum HotChange {
    AuditMilestone {
        task_id: String,
        milestone: Milestone,
    },
    MilestoneEdit {
        task_id: String,
        milestone_id: String,
        kind: Option<String>,
        text: Option<String>,
        remove: bool,
    },
    Fields {
        selector: String,
        fields: TaskFields,
    },
    Task {
        selector: String,
        change: TaskMetadata,
    },
    Milestone {
        task_id: String,
        milestone: Milestone,
        add_kind: bool,
    },
    Stride(Milestone),
}

#[derive(Default)]
pub(in crate::service_plan) struct CommittedRows {
    pub plan_id: String,
    pub revision: i64,
    pub previous_revision: i64,
    pub updated_at: u64,
    pub tasks: BTreeSet<String>,
    pub attempts: usize,
    pub milestones: usize,
    pub strides: usize,
    pub task_rows: usize,
    pub section_rows: usize,
    pub spec_rows: usize,
    pub header_rows: usize,
    pub heartbeat_rows: usize,
    pub element_rows: usize,
    pub decoded_bytes: usize,
    pub serialized_bytes: usize,
    pub projected_text_bytes: usize,
    pub hashed_bytes: usize,
    pub index_root: Option<std::path::PathBuf>,
    pub plan_metadata: Option<super::PlanIndexRow>,
    pub reset_tabs: Vec<String>,
    pub search: Vec<crate::service_plan::search_index::MilestoneIndexDelta>,
    pub removed_tasks: Vec<(String, Vec<String>)>,
    pub task_search: Vec<crate::service_plan::search_index::TaskIndexDelta>,
    pub section_search: Vec<crate::service_plan::search_index::SectionIndexDelta>,
    pub specs: Vec<crate::plan::Spec>,
    pub created_plans: Vec<CommittedRows>,
    pub body_files: Vec<batch_sidecars::NewBodyFile>,
    pub file_bytes: usize,
}

impl std::fmt::Debug for CommittedRows {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommittedRows")
            .field("task_rows", &self.task_rows)
            .field("section_rows", &self.section_rows)
            .field("spec_rows", &self.spec_rows)
            .field("file_bytes", &self.file_bytes)
            .field("header_rows", &self.header_rows)
            .field("attempts", &self.attempts)
            .field("milestones", &self.milestones)
            .field("strides", &self.strides)
            .field("projected_text_bytes", &self.projected_text_bytes)
            .field("hashed_bytes", &self.hashed_bytes)
            .field("decoded_bytes", &self.decoded_bytes)
            .field("serialized_bytes", &self.serialized_bytes)
            .field("changed_tasks", &self.tasks.len())
            .field(
                "search_deltas",
                &(self.search.len() + self.task_search.len()),
            )
            .finish()
    }
}

#[cfg(test)]
thread_local! {
    static LAST_COMMITTED_WORK: std::cell::RefCell<Option<[usize; 14]>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(in crate::service_plan) fn record_work(receipt: &CommittedRows) {
    record_work_values(work_values(receipt));
}

#[cfg(test)]
pub(in crate::service_plan) fn work_values(receipt: &CommittedRows) -> [usize; 14] {
    let mut values = [
        receipt.task_rows,
        receipt.header_rows,
        receipt.attempts,
        receipt.milestones,
        receipt.strides,
        receipt.decoded_bytes,
        receipt.serialized_bytes,
        receipt.projected_text_bytes,
        receipt.hashed_bytes,
        receipt.section_rows,
        receipt.element_rows,
        receipt.heartbeat_rows,
        receipt.spec_rows,
        receipt.file_bytes,
    ];
    for child in &receipt.created_plans {
        for (sum, value) in values.iter_mut().zip(work_values(child)) {
            *sum += value;
        }
    }
    values
}

#[cfg(test)]
pub(in crate::service_plan) fn record_work_values(values: [usize; 14]) {
    LAST_COMMITTED_WORK.with(|cell| *cell.borrow_mut() = Some(values));
}

#[cfg(test)]
fn take_work() -> [usize; 14] {
    LAST_COMMITTED_WORK.with(|cell| cell.borrow_mut().take().expect("committed hot receipt"))
}

impl PlanDatabase {
    pub(in crate::service_plan) fn commit_hot_changes(
        &self,
        plan_id: &str,
        changes: Vec<HotChange>,
        now_ms: u64,
    ) -> Result<CommittedRows, PlanError> {
        self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            for change in changes {
                match change {
                    HotChange::AuditMilestone { task_id, milestone } => {
                        let exists = tx.query_row("SELECT EXISTS(SELECT 1 FROM plan_task_milestones WHERE plan_id=?1 AND task_id=?2 AND json_extract(row_json,'$.id')=?3)", params![plan_id, task_id, milestone.id], |row| row.get::<_, bool>(0)).map_err(db_error)?;
                        if !exists { append_milestone(tx, plan_id, &task_id, &milestone, receipt)?; }
                    }
                    HotChange::MilestoneEdit { task_id, milestone_id, kind, text, remove } => milestones::edit(tx, plan_id, &task_id, &milestone_id, kind.as_deref(), text.as_deref(), remove, receipt)?,
                    HotChange::Fields { selector, fields } => task_fields::apply(tx, plan_id, &selector, fields, now_ms, receipt)?,
                    HotChange::Task { selector, change } => {
                        let mut task = read_task(tx, plan_id, &selector, receipt)?;
                        match change {
                            TaskMetadata::RoleGuidance(value) => {
                                if let Some(value) = value {
                                    task.role_guidance = value;
                                }
                            }
                            TaskMetadata::Personality { name, provenance } => {
                                if let Some(name) = name {
                                    task.personality = name;
                                }
                                if let Some(provenance) = provenance {
                                    task.personality_provenance = provenance;
                                }
                            }
                            TaskMetadata::ReviewOverride(value) => {
                                task.review_override = Some(value)
                            }
                        }
                        task.updated_at = now_ms;
                        write_task(tx, plan_id, &task, receipt)?;
                        task_fields::queue_task_index(tx, plan_id, &task, false, false, receipt)?;
                    }
                    HotChange::Milestone {
                        task_id,
                        milestone,
                        add_kind,
                    } => {
                        declare_kind(tx, plan_id, &milestone.kind, add_kind, receipt)?;
                        append_milestone(tx, plan_id, &task_id, &milestone, receipt)?;
                    }
                    HotChange::Stride(milestone) => {
                        append_stride(tx, plan_id, &milestone, receipt)?
                    }
                }
            }
            Ok(())
        })
    }

    pub(in crate::service_plan) fn change_attempt<T>(
        &self,
        plan_id: &str,
        selector: &str,
        target: AttemptTarget<'_>,
        now_ms: u64,
        change: impl FnOnce(Option<&mut TaskExecutionAttempt>) -> Result<Option<T>, PlanError>,
    ) -> Result<(Option<T>, CommittedRows), PlanError> {
        let mut result = None;
        let receipt = self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            let task_id = resolve_task(tx, plan_id, selector, receipt)?;
            let row: Option<(i64, String)> = tx
                .query_row(
                    "SELECT position, row_json FROM plan_task_attempts
                 WHERE plan_id=?1 AND task_id=?2 AND executor_tab_id=?3
                 AND (?4 IS NULL OR attempt_id=?4)
                 AND (?5 IS NULL OR (session_id=?5 AND session_binding_nonce=?6))
                 AND (NOT ?7 OR json_extract(row_json,'$.retired_at') IS NULL)
                 AND (NOT ?8 OR json_extract(row_json,'$.closed_at') IS NULL)
                 AND (NOT ?9 OR json_extract(row_json,'$.session_model') IS NULL)
                 ORDER BY CASE WHEN ?4 IS NULL THEN position END DESC, position ASC LIMIT 1",
                    params![
                        plan_id,
                        task_id,
                        target.tab,
                        target.id,
                        target.session.map(|value| value.0),
                        target.session.map(|value| value.1),
                        target.unretired || target.live,
                        target.live,
                        target.model_unknown
                    ],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(db_error)?;
            let mut attempt: Option<TaskExecutionAttempt> = row
                .as_ref()
                .map(|(_, json)| {
                    receipt.decoded_bytes += json.len();
                    serde_json::from_str(json).map_err(json_error)
                })
                .transpose()?;
            result = change(attempt.as_mut())?;
            if result.is_none() {
                return Ok(());
            }
            let Some((position, _)) = row else {
                return Err(PlanError::TaskNotFound);
            };
            let document = encode(attempt.as_ref().ok_or(PlanError::TaskNotFound)?, receipt)?;
            tx.execute(
                "UPDATE plan_task_attempts SET row_json=?4, content_hash=?5
                 WHERE plan_id=?1 AND task_id=?2 AND position=?3",
                params![
                    plan_id,
                    task_id,
                    position,
                    document,
                    store_rows::row_hash(&document)
                ],
            )
            .map_err(db_error)?;
            receipt.attempts += 1;
            let mut task = read_task(tx, plan_id, &task_id, receipt)?;
            task.updated_at = now_ms;
            write_task(tx, plan_id, &task, receipt)?;
            task_fields::queue_task_index(tx, plan_id, &task, false, false, receipt)
        })?;
        Ok((result, receipt))
    }

    fn hot_transaction(
        &self,
        plan_id: &str,
        now_ms: u64,
        change: impl FnOnce(&Transaction<'_>, &mut CommittedRows) -> Result<(), PlanError>,
    ) -> Result<CommittedRows, PlanError> {
        let mut receipt =
            self.commit_entity_rows(plan_id, "persist_plan", now_ms, |tx, operation_id| {
                let exists: bool = tx
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1)",
                        [plan_id],
                        |row| row.get(0),
                    )
                    .map_err(db_error)?;
                if !exists {
                    return Err(PlanError::NotFound);
                }
                let mut receipt = row_receipt(tx, plan_id, now_ms, operation_id)?;
                change(tx, &mut receipt)?;
                if receipt.element_rows
                    + receipt.heartbeat_rows
                    + receipt.header_rows
                    + receipt.section_rows
                    + receipt.spec_rows
                    + receipt.task_rows
                    + receipt.milestones
                    + receipt.attempts
                    + receipt.strides
                    > 0
                    || !receipt.created_plans.is_empty()
                {
                    touch_header(tx, &mut receipt)?;
                } else {
                    tx.execute("DELETE FROM plan_operations WHERE id=?1", [operation_id])
                        .map_err(db_error)?;
                }
                Ok(receipt)
            })?;
        for file in &mut receipt.body_files {
            file.keep();
        }
        self.record_row_change(&receipt);
        for created in &receipt.created_plans {
            self.record_row_change(created);
        }
        Ok(receipt)
    }

    pub(super) fn record_row_change(&self, receipt: &CommittedRows) {
        if receipt.header_rows > 0 {
            self.pending_changes
                .lock()
                .insert(super::change_feed::change(
                    &receipt.plan_id,
                    receipt.revision,
                    receipt.previous_revision,
                    receipt.tasks.iter().cloned().collect(),
                    receipt.section_rows + receipt.element_rows + receipt.spec_rows > 0,
                    false,
                ));
        }
    }
}

pub(super) fn row_receipt(
    tx: &Transaction<'_>,
    plan_id: &str,
    now_ms: u64,
    operation_id: i64,
) -> Result<CommittedRows, PlanError> {
    let previous_revision = super::change_feed::revision(tx, plan_id)?;
    Ok(CommittedRows {
        plan_id: plan_id.into(),
        updated_at: now_ms,
        previous_revision,
        revision: super::change_feed::next_revision(previous_revision, operation_id)?,
        ..Default::default()
    })
}

pub(super) fn touch_header(
    tx: &Transaction<'_>,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    tx.execute("UPDATE plans SET updated_at=?2, header_json=json_set(header_json, '$.updated_at', ?2, '$._revision', ?3, '$.canonical_project_path', CASE WHEN trim(coalesce(json_extract(header_json,'$.canonical_project_path'),''))='' THEN project_path ELSE json_extract(header_json,'$.canonical_project_path') END) WHERE id=?1", params![receipt.plan_id, receipt.updated_at, receipt.revision]).map_err(db_error)?;
    receipt.header_rows += 1;
    let root: String = tx.query_row("SELECT CASE WHEN trim(coalesce(json_extract(header_json,'$.canonical_project_path'),''))='' THEN project_path ELSE json_extract(header_json,'$.canonical_project_path') END FROM plans WHERE id=?1", [&receipt.plan_id], |row| row.get(0)).map_err(db_error)?;
    receipt.projected_text_bytes += root.len();
    receipt.index_root = Some(root.into());
    let mut statement = tx.prepare_cached("SELECT DISTINCT assigned_tab_id FROM plan_tasks INDEXED BY plan_tasks_bound_tabs WHERE plan_id=?1 AND assigned_tab_id IS NOT NULL").map_err(db_error)?;
    receipt.reset_tabs = statement
        .query_map([&receipt.plan_id], |row| row.get(0))
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)?;
    receipt.projected_text_bytes += receipt.reset_tabs.iter().map(String::len).sum::<usize>();
    Ok(())
}

fn resolve_task(
    tx: &Transaction<'_>,
    plan_id: &str,
    selector: &str,
    receipt: &mut CommittedRows,
) -> Result<String, PlanError> {
    let exact = tx
        .query_row(
            "SELECT task_id FROM plan_tasks WHERE plan_id=?1 AND task_id=?2",
            params![plan_id, selector],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(db_error)?;
    if let Some(id) = exact {
        receipt.projected_text_bytes += id.len();
        return Ok(id);
    }
    let mut statement = tx.prepare_cached("SELECT task_id, json_extract(row_json, '$.key'), json_extract(row_json, '$.handle') FROM plan_tasks WHERE plan_id=?1 ORDER BY position").map_err(db_error)?;
    let mut found = None;
    let rows = statement
        .query_map([plan_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(db_error)?;
    for row in rows {
        let (id, key, handle) = row.map_err(db_error)?;
        receipt.projected_text_bytes +=
            id.len() + key.as_ref().map_or(0, String::len) + handle.as_ref().map_or(0, String::len);
        let key = key.unwrap_or_else(|| crate::plan::task_key::fallback_key(&id));
        if crate::plan::task_key::name_matches(handle.as_deref(), &key, selector) {
            if found.is_some() {
                return Err(PlanError::TaskNotFound);
            }
            found = Some(id);
        }
    }
    found.ok_or(PlanError::TaskNotFound)
}

fn read_task(
    tx: &Transaction<'_>,
    plan_id: &str,
    selector: &str,
    receipt: &mut CommittedRows,
) -> Result<Task, PlanError> {
    let id = resolve_task(tx, plan_id, selector, receipt)?;
    let json: String = tx
        .query_row(
            "SELECT row_json FROM plan_tasks WHERE plan_id=?1 AND task_id=?2",
            params![plan_id, id],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    receipt.decoded_bytes += json.len();
    serde_json::from_str(&json).map_err(json_error)
}

fn encode(value: &impl serde::Serialize, receipt: &mut CommittedRows) -> Result<String, PlanError> {
    let value = serde_json::to_value(value).map_err(json_error)?;
    text_bounds::validate_stored_entity(&value)?;
    let document = serde_json::to_string(&value).map_err(json_error)?;
    if document.len() > store_rows::MAX_STORED_ROW_BYTES {
        return Err(PlanError::InvalidPlanText(format!(
            "entity row exceeds {} UTF-8 bytes",
            store_rows::MAX_STORED_ROW_BYTES
        )));
    }
    receipt.serialized_bytes += document.len();
    Ok(document)
}

fn write_task(
    tx: &Transaction<'_>,
    plan_id: &str,
    task: &Task,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    text_bounds::validate_stored_task_text(task)?;
    let mut value = serde_json::to_value(task).map_err(json_error)?;
    if let Some(fields) = value.as_object_mut() {
        fields.remove("milestones");
        fields.remove("execution_attempts");
    }
    let document = encode(&value, receipt)?;
    tx.execute("UPDATE plan_tasks SET updated_at=?3, content_hash=?4, row_json=?5 WHERE plan_id=?1 AND task_id=?2", params![plan_id, task.id, task.updated_at, store_rows::row_hash(&document), document]).map_err(db_error)?;
    receipt.tasks.insert(task.id.clone());
    receipt.task_rows += 1;
    Ok(())
}

fn declare_kind(
    tx: &Transaction<'_>,
    plan_id: &str,
    kind: &str,
    add: bool,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    let json: String = tx.query_row("SELECT coalesce(json_extract(header_json, '$.milestone_kinds'), '[]') FROM plans WHERE id=?1", [plan_id], |row| row.get(0)).map_err(db_error)?;
    receipt.decoded_bytes += json.len();
    let mut kinds: Vec<MilestoneKind> = serde_json::from_str(&json).map_err(json_error)?;
    if kinds.iter().any(|entry| entry.key == kind) {
        return Ok(());
    }
    if !add {
        return Err(PlanError::UndeclaredKind {
            declared: kinds.into_iter().map(|entry| entry.key).collect(),
        });
    }
    kinds.push(MilestoneKind {
        key: kind.into(),
        label: kind.into(),
        color: None,
    });
    let json = encode(&kinds, receipt)?;
    tx.execute("UPDATE plans SET header_json=json_set(header_json, '$.milestone_kinds', json(?2)) WHERE id=?1", params![plan_id, json]).map_err(db_error)?;
    receipt.header_rows += 1;
    Ok(())
}

fn append_milestone(
    tx: &Transaction<'_>,
    plan_id: &str,
    task_id: &str,
    milestone: &Milestone,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    let (count, digest, next): (i64, i64, i64) = tx.query_row("SELECT milestone_count, milestone_digest, (SELECT coalesce(max(seq)+1,0) FROM plan_task_milestones WHERE plan_id=?1 AND task_id=?2) FROM plan_tasks WHERE plan_id=?1 AND task_id=?2", params![plan_id, task_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional().map_err(db_error)?.ok_or(PlanError::TaskNotFound)?;
    let json = encode(milestone, receipt)?;
    tx.execute(
        "INSERT INTO plan_task_milestones(plan_id, task_id, seq, row_json) VALUES (?1, ?2, ?3, ?4)",
        params![plan_id, task_id, next, json],
    )
    .map_err(db_error)?;
    tx.execute("UPDATE plan_tasks SET milestone_count=?3, milestone_digest=?4 WHERE plan_id=?1 AND task_id=?2", params![plan_id, task_id, count + 1, store_rows::chain_hash(digest, &json)]).map_err(db_error)?;
    receipt.tasks.insert(task_id.into());
    receipt.task_rows += 1;
    receipt.milestones += 1;
    queue_milestone_index(tx, plan_id, task_id, milestone, false, receipt)
}

fn queue_milestone_index(
    tx: &Transaction<'_>,
    plan_id: &str,
    task_id: &str,
    milestone: &Milestone,
    remove: bool,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    let delta = tx
        .query_row(
            "SELECT p.title, p.status, p.project_path,
         json_extract(p.header_json, '$.canonical_project_path'),
         json_extract(t.row_json, '$.name'), t.status
         FROM plans p JOIN plan_tasks t ON t.plan_id=p.id WHERE p.id=?1 AND t.task_id=?2",
            params![plan_id, task_id],
            |row| {
                Ok(crate::service_plan::search_index::MilestoneIndexDelta {
                    plan_id: plan_id.into(),
                    title: row.get(0)?,
                    plan_status: row.get(1)?,
                    project_path: row.get(2)?,
                    canonical_project_path: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    task_name: row.get(4)?,
                    task_status: row.get(5)?,
                    milestone: milestone.clone(),
                    remove,
                })
            },
        )
        .map_err(db_error)?;
    receipt.projected_text_bytes += delta.title.len()
        + delta.plan_status.len()
        + delta.project_path.len()
        + delta.canonical_project_path.len()
        + delta.task_name.len()
        + delta.task_status.len();
    receipt.search.push(delta);
    Ok(())
}

fn append_stride(
    tx: &Transaction<'_>,
    plan_id: &str,
    milestone: &Milestone,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    let (count, digest): (i64, i64) = tx
        .query_row(
            "SELECT stride_count, stride_digest FROM plans WHERE id=?1",
            [plan_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(db_error)?;
    let json = encode(milestone, receipt)?;
    tx.execute(
        "INSERT INTO plan_strides(plan_id, seq, row_json) VALUES (?1, ?2, ?3)",
        params![plan_id, count, json],
    )
    .map_err(db_error)?;
    tx.execute(
        "UPDATE plans SET stride_count=?2, stride_digest=?3 WHERE id=?1",
        params![plan_id, count + 1, store_rows::chain_hash(digest, &json)],
    )
    .map_err(db_error)?;
    receipt.header_rows += 1;
    receipt.strides += 1;
    Ok(())
}

#[cfg(test)]
pub(in crate::service_plan::store_db) mod tests;
