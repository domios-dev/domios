use super::*;

/// What one plan persist changed; task units embed the plan title and status, so those changes are `Full`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanIndexScope {
    Full,
    Tasks(HashSet<String>),
}

impl PlanIndexScope {
    pub(super) fn union(self, other: PlanIndexScope) -> PlanIndexScope {
        match (self, other) {
            (Self::Tasks(mut tasks), Self::Tasks(other)) => {
                tasks.extend(other);
                Self::Tasks(tasks)
            }
            _ => Self::Full,
        }
    }
}

pub(super) fn mirror_plan_scope(
    plan: &Plan,
    scope: &PlanIndexScope,
    sync_embed: bool,
) -> Result<Vec<Unit>, String> {
    match scope {
        PlanIndexScope::Full => mirror_plan(plan, sync_embed),
        PlanIndexScope::Tasks(changed) => mirror_plan_tasks(plan, changed, sync_embed),
    }
}

/// Rebuilds the plan header and the changed tasks' units, and deletes units of tasks or strides that are gone.
pub(super) fn mirror_plan_tasks(
    plan: &Plan,
    changed: &HashSet<String>,
    sync_embed: bool,
) -> Result<Vec<Unit>, String> {
    let root = plan_project_root(plan);
    let scope = project_scope(root);
    let plan_status = plan_status_text(&plan.status);
    let mut units = plan_header_units(plan, &scope);
    let mut live_ids = HashSet::new();
    for task in &plan.tasks {
        let rebuild = changed.contains(&task.id);
        live_ids.insert(stable_id(&plan.id, "task", &task.id));
        if rebuild {
            units.push(task_unit(plan, task, &scope, &plan_status));
        }
        for stride in &task.milestones {
            live_ids.insert(stable_id(&plan.id, "stride", &stride.id));
            if rebuild {
                units.push(stride_unit(plan, task, stride, &scope, &plan_status));
            }
        }
    }
    let conn = open_index(root)?;
    let mut existing = indexed_units_like(&conn, &format!("planunit_{}_plan%", plan.id))?;
    let rebuilt = units
        .iter()
        .map(|unit| unit.id.as_str())
        .collect::<HashSet<_>>();
    let mut candidates = Vec::new();
    for kind in ["task", "stride"] {
        for id in unit_ids_like(&conn, &format!("planunit_{}_{kind}_%", plan.id))? {
            if !live_ids.contains(&id) || rebuilt.contains(id.as_str()) {
                candidates.push(id);
            }
        }
    }
    existing.extend(indexed_units_by_id(&conn, &candidates)?);
    apply_unit_changes(root, conn, existing, &units, sync_embed)
}

fn indexed_units_by_id(
    conn: &Connection,
    ids: &[String],
) -> Result<HashMap<String, IndexedUnit>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT title, body_inline, kind, scope, source, updated_at FROM mem_items WHERE id = ?1",
        )
        .map_err(|e| e.to_string())?;
    let mut units = HashMap::new();
    for id in ids {
        let unit = stmt
            .query_row(params![id], |r| {
                Ok(IndexedUnit {
                    title: r.get(0)?,
                    body: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    kind: r.get(2)?,
                    scope: r.get(3)?,
                    source: r.get(4)?,
                    updated_at: r.get(5)?,
                })
            })
            .map_err(|e| e.to_string())?;
        units.insert(id.clone(), unit);
    }
    Ok(units)
}

#[cfg(test)]
mod tests;
