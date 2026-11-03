use super::*;
use rusqlite::OptionalExtension;

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
    index_queue().enqueue(IndexJob::Unit(root, unit));
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
    Unit {
        id: stable_id(plan_id, "stride", &stride.id),
        kind: "stride",
        title: format!("{} stride {}", task_name, stride.kind),
        body: format!(
            "Plan \"{}\" · task \"{}\" · stride({}): {}",
            plan_title, task_name, stride.kind, stride.text
        ),
        scope: scope.to_string(),
        plan_id: Some(plan_id.into()),
        spec_id: None,
        unit_id: stride.id.clone(),
        unit_title: stride.kind.clone(),
        parent_title: task_name.into(),
        status: Some(format!("{plan_status}:{task_status}")),
        updated_at: stride.ts,
    }
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
