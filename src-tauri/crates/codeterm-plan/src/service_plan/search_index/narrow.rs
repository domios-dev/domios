use super::*;
use rusqlite::OptionalExtension;

pub(in crate::service_plan) fn enqueue_plan_metadata(
    _turn: &MutationWriterTurn<'_>,
    root: PathBuf,
    row: crate::service_plan::store_db::PlanIndexRow,
) {
    index_queue().enqueue(IndexJob::PlanMetadata(root, row));
}

pub(super) fn mirror_plan_metadata(
    root: &Path,
    row: &crate::service_plan::store_db::PlanIndexRow,
) -> Result<(), String> {
    let status = plan_status_text(&row.status);
    let unit = super::unit_inputs::UnitParent {
        id: &row.id,
        title: &row.title,
        scope: &project_scope(root),
        status: &status,
    }
    .header("", row.updated_at);
    let mut connection = open_index(root)?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    delete_unit_vectors(&transaction, &unit.id)?;
    transaction
        .execute("DELETE FROM mem_fts WHERE mem_id=?1", [&unit.id])
        .map_err(|error| error.to_string())?;
    transaction
        .execute("DELETE FROM mem_items WHERE id=?1", [&unit.id])
        .map_err(|error| error.to_string())?;
    insert_unit(&transaction, &unit)?;
    transaction.execute("UPDATE mem_items SET title=CASE kind WHEN 'plan_section' THEN ?2 || ' section ' || json_extract(source,'$.unit_title') WHEN 'task' THEN ?2 || ' task ' || json_extract(source,'$.unit_title') ELSE title END, source=json_set(source,'$.parent_title',CASE WHEN kind='stride' THEN json_extract(source,'$.parent_title') ELSE ?2 END,'$.status', CASE WHEN instr(coalesce(json_extract(source,'$.status'),''),':')>0 THEN ?3 || substr(json_extract(source,'$.status'),instr(json_extract(source,'$.status'),':')) ELSE ?3 END) WHERE json_extract(source,'$.plan_id')=?1", params![row.id, row.title, status]).map_err(|error| error.to_string())?;
    transaction.execute("UPDATE mem_fts SET title=(SELECT title FROM mem_items WHERE id=mem_id) WHERE mem_id IN (SELECT id FROM mem_items WHERE json_extract(source,'$.plan_id')=?1) AND title IS NOT (SELECT title FROM mem_items WHERE id=mem_id)", [&row.id]).map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    index_queue().enqueue(IndexJob::Embed(root.to_path_buf(), unit));
    Ok(())
}

pub(in crate::service_plan) fn enqueue_plan_time(
    _turn: &MutationWriterTurn<'_>,
    root: PathBuf,
    plan_id: String,
    at: u64,
) {
    index_queue().enqueue(IndexJob::PlanTime(root, plan_id, at));
}

pub(super) fn mirror_plan_time(root: &Path, plan_id: &str, at: u64) -> Result<(), String> {
    let connection = open_index(root)?;
    connection.execute("UPDATE mem_items SET source=json_set(source,'$.updated_at',?2), updated_at=?3 WHERE kind IN ('plan','plan_section') AND json_extract(source,'$.plan_id')=?1 AND json_extract(source,'$.updated_at') IS NOT ?2", params![plan_id, at, timestamp_ms_to_rfc3339(at)]).map_err(|error| error.to_string())?;
    Ok(())
}

#[derive(Debug)]
pub(in crate::service_plan) struct TaskIndexDelta {
    pub plan_id: String,
    pub title: String,
    pub plan_status: String,
    pub project_path: String,
    pub canonical_project_path: String,
    pub task: Task,
    pub milestones: Vec<Milestone>,
    pub milestone_status: Vec<String>,
}

pub(super) struct StatusDelta {
    pub key: String,
    pub ids: Vec<String>,
    pub status: String,
}

pub(in crate::service_plan) fn enqueue_task_index(
    _turn: &MutationWriterTurn<'_>,
    delta: TaskIndexDelta,
) {
    let root = PathBuf::from(if delta.canonical_project_path.trim().is_empty() {
        &delta.project_path
    } else {
        &delta.canonical_project_path
    });
    let scope = project_scope(&root);
    let unit = task_unit(
        &delta.plan_id,
        &delta.title,
        &delta.task,
        &scope,
        &delta.plan_status,
    );
    index_queue().enqueue(IndexJob::Unit(root.clone(), unit));
    for milestone in &delta.milestones {
        let unit = milestone_unit(
            &delta.plan_id,
            &delta.title,
            &delta.task.name,
            &task_status_text(&delta.task),
            milestone,
            &scope,
            &delta.plan_status,
        );
        index_queue().enqueue(IndexJob::Unit(root.clone(), unit));
    }
    if !delta.milestone_status.is_empty() {
        index_queue().enqueue(IndexJob::Status(
            root,
            StatusDelta {
                key: format!("{}:{}", delta.plan_id, delta.task.id),
                ids: delta
                    .milestone_status
                    .iter()
                    .map(|id| stable_id(&delta.plan_id, "stride", id))
                    .collect(),
                status: format!("{}:{}", delta.plan_status, task_status_text(&delta.task)),
            },
        ));
    }
}

pub(super) fn mirror_status(root: &Path, delta: &StatusDelta) -> Result<(), String> {
    let mut connection = open_index(root)?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    for id in &delta.ids {
        transaction.execute("UPDATE mem_items SET source=json_set(source,'$.status',?2) WHERE id=?1 AND json_extract(source,'$.status') IS NOT ?2", params![id, delta.status]).map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())
}

#[derive(Debug)]
pub(in crate::service_plan) struct MilestoneIndexDelta {
    pub plan_id: String,
    pub title: String,
    pub plan_status: String,
    pub project_path: String,
    pub canonical_project_path: String,
    pub task_name: String,
    pub task_status: String,
    pub milestone: Milestone,
    pub remove: bool,
}

pub(in crate::service_plan) fn enqueue_milestone_index(
    _turn: &MutationWriterTurn<'_>,
    delta: MilestoneIndexDelta,
) {
    let root = if delta.canonical_project_path.trim().is_empty() {
        PathBuf::from(&delta.project_path)
    } else {
        PathBuf::from(&delta.canonical_project_path)
    };
    let unit = milestone_unit(
        &delta.plan_id,
        &delta.title,
        &delta.task_name,
        &delta.task_status,
        &delta.milestone,
        &project_scope(&root),
        &delta.plan_status,
    );
    if delta.remove {
        index_queue().enqueue(IndexJob::DeleteUnit(root, unit.id));
    } else {
        index_queue().enqueue(IndexJob::Unit(root, unit));
    }
}

pub(super) fn milestone_unit(
    plan_id: &str,
    plan_title: &str,
    task_name: &str,
    task_status: &str,
    stride: &Milestone,
    scope: &str,
    plan_status: &str,
) -> Unit {
    super::unit_inputs::UnitParent {
        id: plan_id,
        title: plan_title,
        scope,
        status: plan_status,
    }
    .stride(
        task_name,
        task_status,
        super::unit_inputs::StrideUnitInput {
            id: &stride.id,
            kind: &stride.kind,
            text: &stride.text,
            ts: stride.ts,
        },
    )
}

pub(super) fn mirror_unit(root: &Path, unit: &Unit) -> Result<Vec<Unit>, String> {
    let connection = open_index(root)?;
    let previous = connection
        .query_row(
            "SELECT title, body_inline, kind, scope, source, updated_at FROM mem_items WHERE id=?1",
            [&unit.id],
            |row| {
                Ok(IndexedUnit {
                    title: row.get(0)?,
                    body: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    kind: row.get(2)?,
                    scope: row.get(3)?,
                    source: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some(previous) = &previous {
        let next = unit_signature(unit);
        if previous.title == next.title
            && previous.body == next.body
            && previous.kind == next.kind
            && previous.scope == next.scope
        {
            if previous != &next {
                connection
                    .execute(
                        "UPDATE mem_items SET source=?2, updated_at=?3 WHERE id=?1",
                        params![unit.id, next.source, next.updated_at],
                    )
                    .map_err(|error| error.to_string())?;
            }
            return Ok(Vec::new());
        }
    }
    let existing = previous
        .into_iter()
        .map(|previous| (unit.id.clone(), previous))
        .collect();
    apply_unit_changes(
        root,
        connection,
        existing,
        std::slice::from_ref(unit),
        false,
    )
}

pub(super) fn task_unit(
    plan_id: &str,
    plan_title: &str,
    task: &Task,
    scope: &str,
    plan_status: &str,
) -> Unit {
    super::unit_inputs::UnitParent {
        id: plan_id,
        title: plan_title,
        scope,
        status: plan_status,
    }
    .task(super::unit_inputs::TaskUnitInput {
        id: &task.id,
        name: &task.name,
        status: &task.status,
        updated_at: task.updated_at,
        body: &task.body,
        acceptance_criteria: &task.acceptance_criteria,
        external_refs: task
            .external_refs
            .iter()
            .map(|reference| reference.canonical_url.as_str())
            .collect(),
    })
}

pub(super) fn delete_unit(root: &Path, id: &str) -> Result<(), String> {
    let mut connection = open_index(root)?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    delete_unit_vectors(&transaction, id)?;
    transaction
        .execute("DELETE FROM mem_fts WHERE mem_id=?1", [id])
        .map_err(|error| error.to_string())?;
    transaction
        .execute("DELETE FROM mem_items WHERE id=?1", [id])
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

pub(in crate::service_plan) fn enqueue_task_removal(
    _turn: &MutationWriterTurn<'_>,
    root: PathBuf,
    plan_id: &str,
    task_id: &str,
    milestones: &[String],
) {
    index_queue().enqueue(IndexJob::DeleteUnit(
        root.clone(),
        stable_id(plan_id, "task", task_id),
    ));
    for id in milestones {
        index_queue().enqueue(IndexJob::DeleteUnit(
            root.clone(),
            stable_id(plan_id, "stride", id),
        ));
    }
}
