use super::{AttemptMatch, AttemptState, PlanError, PlanMatch, PlanStore};
use crate::plan::{Plan, Task, TaskFiles, WorkerPlacement};
mod inspection;
mod report;
pub(crate) use inspection::{SharedDispatchInspection, SharedReportInspection};
pub use report::{shared_checkout_head, validate_shared_report};

fn canonical(plan: &Plan) -> String {
    let path = if plan.canonical_project_path.trim().is_empty() {
        &plan.project_path
    } else {
        &plan.canonical_project_path
    };
    canonical_path(path)
}

fn canonical_path(path: &str) -> String {
    let resolved = std::fs::canonicalize(path).ok();
    codeterm_paths::normalize_cwd(
        resolved
            .as_ref()
            .map(|path| path.to_string_lossy())
            .as_deref()
            .unwrap_or(path),
    )
}

pub fn live_shared(task: &Task) -> bool {
    task.execution_attempts.iter().any(|attempt| {
        attempt.closed_at.is_none()
            && attempt.retired_at.is_none()
            && attempt.placement == WorkerPlacement::Shared
    })
}

pub fn validate_exclusive_checkout(
    plans: &[Plan],
    plan: &Plan,
    task_id: &str,
) -> Result<(), String> {
    for owner_plan in plans
        .iter()
        .filter(|other| canonical(other) == canonical(plan))
    {
        for owner in &owner_plan.tasks {
            if owner_plan.id == plan.id && owner.id == task_id {
                continue;
            }
            if owner.execution_attempts.iter().any(|attempt| {
                attempt.closed_at.is_none()
                    && attempt.retired_at.is_none()
                    && !attempt.placement.is_isolated_lane()
            }) {
                return Err(format!(
                    "canonical checkout is held by task '{}' in plan '{}'",
                    owner.id, owner_plan.id
                ));
            }
        }
    }
    Ok(())
}

pub fn validate_shared_files(
    plans: &[Plan],
    plan: &Plan,
    task: &Task,
    files: &TaskFiles,
) -> Result<(), String> {
    files.shared_edit_paths()?;
    validate_edit_links(plan, files)?;
    validate_shared_ownership(plans, plan, task, files)
}

pub(crate) fn validate_shared_ownership(
    plans: &[Plan],
    plan: &Plan,
    task: &Task,
    files: &TaskFiles,
) -> Result<(), String> {
    for owner_plan in plans
        .iter()
        .filter(|other| canonical(other) == canonical(plan))
    {
        for owner in &owner_plan.tasks {
            if owner_plan.id == plan.id && owner.id == task.id {
                continue;
            }
            for attempt in owner
                .execution_attempts
                .iter()
                .filter(|attempt| attempt.closed_at.is_none() && attempt.retired_at.is_none())
            {
                if attempt.placement == WorkerPlacement::CanonicalWriter {
                    return Err(format!(
                        "canonical checkout is held exclusively by task '{}' in plan '{}'",
                        owner.id, owner_plan.id
                    ));
                }
                if attempt.placement != WorkerPlacement::Shared {
                    continue;
                }
                owner.files.shared_edit_paths()?;
                if let Some(path) = files.overlapping_edit_path(&owner.files) {
                    return Err(format!(
                        "edit path '{path}' overlaps task '{}' in plan '{}' (attempt '{}')",
                        owner.id, owner_plan.id, attempt.id
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_edit_links(plan: &Plan, files: &TaskFiles) -> Result<(), String> {
    let root = std::path::PathBuf::from(canonical(plan));
    for edit in files.shared_edit_paths()? {
        let mut current = root.clone();
        for component in edit.split('/') {
            current.push(component);
            match std::fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(format!(
                        "shared edit path '{edit}' traverses a filesystem link"
                    ))
                }
                Ok(_) => {
                    let resolved = std::fs::canonicalize(&current).map_err(|error| {
                        format!("cannot resolve shared edit path '{edit}': {error}")
                    })?;
                    if !codeterm_paths::paths_equal(
                        &current.to_string_lossy(),
                        &resolved.to_string_lossy(),
                    ) {
                        return Err(format!("shared edit path '{edit}' uses a filesystem alias"));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(error) => {
                    return Err(format!("cannot inspect shared edit path '{edit}': {error}"))
                }
            }
        }
    }
    Ok(())
}

pub fn choose_placement(
    plans: &[Plan],
    plan: &Plan,
    task: &Task,
    requested: Option<&str>,
) -> Result<(WorkerPlacement, String), String> {
    match requested {
        Some("lane" | "isolated_lane") => {
            return Ok((
                WorkerPlacement::IsolatedLane,
                "explicit lane placement".into(),
            ))
        }
        Some("shared") | None => {}
        Some(value) => return Err(format!("unknown placement '{value}'")),
    }
    let lane_reason = if task.lane.worktree_path.is_some() || task.lane.branch.is_some() {
        Some("resume recorded lane".into())
    } else if task.tags.iter().any(|tag| tag == "build-breaking") {
        Some("task is marked build-breaking".into())
    } else {
        task.files.shared_edit_paths().map_err(|error| format!("{error}. Declare edit files: domios plan task files {} {} --edit-file <path>; or request isolation: domios plan task dispatch {} {} --execution-intent mutating --placement lane", plan.id, task.id, plan.id, task.id))?;
        validate_edit_links(plan, &task.files)?;
        match validate_shared_ownership(plans, plan, task, &task.files) {
            Ok(()) => None,
            Err(error) => Some(error),
        }
    };
    if let Some(reason) = lane_reason {
        if requested == Some("shared") {
            return Err(format!("shared placement refused: {reason}"));
        }
        return Ok((WorkerPlacement::IsolatedLane, reason));
    }
    Ok((
        WorkerPlacement::Shared,
        "disjoint declared edit files in shared checkout".into(),
    ))
}

impl PlanStore {
    pub fn checkout_peer_plans(&self, plan: &Plan) -> Vec<Plan> {
        let checkout = canonical(plan);
        let ids = self
            .plan_roots()
            .into_iter()
            .filter(|(_, root)| canonical_path(root) == checkout)
            .map(|(id, _)| id)
            .collect();
        let placements = ["shared", "canonical_writer"]
            .into_iter()
            .map(|placement| {
                PlanMatch::Attempt(AttemptMatch {
                    state: AttemptState::Open,
                    fields: vec![("$.placement", placement.to_string())],
                    ..Default::default()
                })
            })
            .collect();
        self.plans_matching(&PlanMatch::Ids(ids).and(PlanMatch::Any(placements)))
    }
    pub fn extend_task_edit_files(
        &self,
        plan_id: &str,
        task_id: &str,
        paths: Vec<String>,
        now: u64,
    ) -> Result<(), PlanError> {
        let writer_turn = self.mutation_writer.enter();
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let task = plan
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .ok_or(PlanError::TaskNotFound)?;
        if !live_shared(task) {
            return Err(PlanError::InvalidPlanText(
                "only a live shared Worker can extend edit files".into(),
            ));
        }
        let mut files = task.files.clone();
        files.edit.extend(paths);
        let files = files.validated().map_err(PlanError::InvalidPlanText)?;
        validate_shared_files(&self.checkout_peer_plans(&plan), &plan, task, &files)
            .map_err(PlanError::InvalidPlanText)?;
        self.with_plan_in_writer(&writer_turn, plan_id, now, |plan| {
            let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            task.files = files;
            task.updated_at = now;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;
