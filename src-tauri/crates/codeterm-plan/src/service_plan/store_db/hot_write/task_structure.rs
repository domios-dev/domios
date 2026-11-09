use super::*;
use crate::plan::TaskStatus;
use crate::service_plan::{mutations, section_links, SplitParentReason};

impl PlanDatabase {
    pub(in crate::service_plan) fn remove_tasks(
        &self,
        plan_id: &str,
        selector: Option<&str>,
        now: u64,
    ) -> Result<(Vec<String>, CommittedRows), PlanError> {
        let mut ids: Vec<String> = Vec::new();
        let receipt = self.hot_transaction(plan_id, now, |tx, receipt| {
            let selected = selector.map(|selector| resolve_task(tx, plan_id, selector, receipt)).transpose()?;
            let review: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM plan_tasks WHERE plan_id=?1 AND (?2 IS NULL OR task_id=?2) AND status='in_review')", params![plan_id, selected], |row| row.get(0)).map_err(db_error)?;
            if review { return Err(PlanError::TransitionDenied { from: TaskStatus::InReview, to: TaskStatus::Cancelled }); }
            if let Some(id) = &selected {
                let mut statement = tx.prepare_cached("SELECT task_id FROM plan_tasks t WHERE plan_id=?1 AND task_id!=?2 AND EXISTS(SELECT 1 FROM json_each(t.row_json,'$.depends_on') WHERE value=?2) ORDER BY position").map_err(db_error)?;
                let dependents = statement.query_map(params![plan_id, id], |row| row.get::<_, String>(0)).map_err(db_error)?.collect::<Result<Vec<_>, _>>().map_err(db_error)?;
                if !dependents.is_empty() { return Err(PlanError::DependencyTaskReferenced { task_id: id.clone(), dependent_task_ids: dependents }); }
            }
            let mut statement = tx.prepare_cached("SELECT task_id FROM plan_tasks WHERE plan_id=?1 AND (?2 IS NULL OR task_id=?2) ORDER BY position").map_err(db_error)?;
            ids = statement.query_map(params![plan_id, selected], |row| row.get(0)).map_err(db_error)?.collect::<Result<_, _>>().map_err(db_error)?;
            for id in &ids {
                let mut statement = tx.prepare_cached("SELECT json_extract(row_json,'$.id') FROM plan_task_milestones WHERE plan_id=?1 AND task_id=?2 ORDER BY seq").map_err(db_error)?;
                let milestones: Vec<String> = statement.query_map(params![plan_id, id], |row| row.get(0)).map_err(db_error)?.collect::<Result<_, _>>().map_err(db_error)?;
                receipt.projected_text_bytes += id.len() + milestones.iter().map(String::len).sum::<usize>();
                receipt.milestones += milestones.len();
                receipt.attempts += tx.query_row("SELECT count(*) FROM plan_task_attempts WHERE plan_id=?1 AND task_id=?2", params![plan_id, id], |row| row.get::<_, usize>(0)).map_err(db_error)?;
                receipt.removed_tasks.push((id.clone(), milestones));
                receipt.tasks.insert(id.clone());
            }
            receipt.task_rows += tx.execute("DELETE FROM plan_tasks WHERE plan_id=?1 AND (?2 IS NULL OR task_id=?2)", params![plan_id, selected]).map_err(db_error)?;
            if selector.is_none() {
                tx.execute("UPDATE plans SET header_json=json_set(header_json,'$.mode','planning','$.heartbeat.current_focus',NULL,'$.heartbeat.active_lanes',json('[]')) WHERE id=?1", [plan_id]).map_err(db_error)?;
                receipt.header_rows += 1;
            }
            Ok(())
        })?;
        Ok((ids, receipt))
    }

    pub(in crate::service_plan) fn update_dependencies(
        &self,
        plan_id: &str,
        selector: &str,
        dependencies: &[String],
        now: u64,
    ) -> Result<CommittedRows, PlanError> {
        self.hot_transaction(plan_id, now, |tx, receipt| {
            let mut task = read_task(tx, plan_id, selector, receipt)?;
            task.depends_on = resolve_dependencies(tx, plan_id, &task.id, dependencies, receipt)?;
            task.updated_at = now;
            write_task(tx, plan_id, &task, receipt)?;
            task_fields::queue_task_index(tx, plan_id, &task, false, false, receipt)
        })
    }

    pub(in crate::service_plan) fn insert_tasks(
        &self,
        plan_id: &str,
        tasks: Vec<Task>,
        split_parent: Option<&str>,
        now: u64,
    ) -> Result<CommittedRows, PlanError> {
        self.hot_transaction(plan_id, now, |tx, receipt| {
            insert_task_rows(tx, plan_id, tasks, split_parent, receipt)
        })
    }
}

pub(super) fn validate_parent(parent: &Task) -> Result<(), PlanError> {
    if parent.parent_task_id.is_some() {
        return Err(PlanError::InvalidSplitParent {
            reason: SplitParentReason::ChildParent,
        });
    }
    if matches!(parent.status, TaskStatus::Done | TaskStatus::Cancelled) {
        return Err(PlanError::InvalidSplitParent {
            reason: SplitParentReason::TerminalParent,
        });
    }
    Ok(())
}

pub(super) fn resolve_dependencies(
    tx: &Transaction<'_>,
    plan_id: &str,
    task_id: &str,
    dependencies: &[String],
    receipt: &mut CommittedRows,
) -> Result<Vec<String>, PlanError> {
    let mut resolved = Vec::new();
    for selector in mutations::normalized_dependency_ids(dependencies) {
        let dependency =
            resolve_task(tx, plan_id, &selector, receipt).map_err(|error| match error {
                PlanError::TaskNotFound => PlanError::DependencyTaskNotFound { task_id: selector },
                error => error,
            })?;
        if dependency == task_id {
            return Err(PlanError::DependencySelfReference);
        }
        if !resolved.contains(&dependency) {
            resolved.push(dependency);
        }
    }
    for dependency in &resolved {
        let reaches: bool = tx.query_row("WITH RECURSIVE reachable(id) AS (SELECT ?2 UNION SELECT d.value FROM reachable r JOIN plan_tasks t ON t.plan_id=?1 AND t.task_id=r.id JOIN json_each(t.row_json,'$.depends_on') d) SELECT EXISTS(SELECT 1 FROM reachable WHERE id=?3)", params![plan_id, dependency, task_id], |row| row.get(0)).map_err(db_error)?;
        if reaches {
            return Err(PlanError::DependencyCycle);
        }
    }
    Ok(resolved)
}

pub(super) fn insert_task_rows(
    tx: &Transaction<'_>,
    plan_id: &str,
    tasks: Vec<Task>,
    split_parent: Option<&str>,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    if split_parent.is_some() {
        let allowed: bool = tx.query_row("SELECT status!='archived' AND coalesce(json_extract(header_json,'$.mode'),'planning') NOT IN ('paused','complete') FROM plans WHERE id=?1", [plan_id], |row| row.get(0)).map_err(db_error)?;
        if !allowed {
            return Err(PlanError::BadMode);
        }
    }
    let section_input = super::super::section_link_queries::section_link_input(tx, plan_id, false)?;
    let mut created_ids = Vec::new();
    for mut task in tasks {
        if let Some(parent_id) = task.parent_task_id.as_deref() {
            let parent = read_task(tx, plan_id, parent_id, receipt)?;
            if parent.id != parent_id {
                return Err(PlanError::TaskNotFound);
            }
            validate_parent(&parent)?;
            if split_parent.is_some() {
                task.tags =
                    crate::service_plan::tag_model::inherit_parent_tags(&parent.tags, &task.tags);
            }
        }
        task.depends_on = resolve_dependencies(tx, plan_id, &task.id, &task.depends_on, receipt)?;
        if let Some(group) = task.group.as_deref() {
            task_fields::declare_group(tx, plan_id, group, receipt)?;
        }
        if task.section_ids.is_empty() {
            task.section_ids = section_links::auto_section_ids_for_task(&section_input, &task);
        }
        task_fields::insert_task(tx, plan_id, &task, receipt)?;
        created_ids.push(task.id);
    }
    task_fields::normalize_handles(tx, plan_id, receipt)?;
    for id in created_ids {
        let task = read_task(tx, plan_id, &id, receipt)?;
        task_fields::queue_task_index(tx, plan_id, &task, false, false, receipt)?;
    }
    Ok(())
}
