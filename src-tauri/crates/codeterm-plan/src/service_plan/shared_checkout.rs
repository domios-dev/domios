use super::{PlanError, PlanStore};
#[cfg(test)]
use crate::plan::{Plan, Task};
use crate::plan::{TaskFiles, WorkerPlacement};
mod history;
mod inspection;
mod plain_folder;
mod report;
pub use super::store_db::read_projections::checkout::{
    CheckoutAttemptRead, CheckoutPeerRead, CheckoutTaskRead, TaskCheckoutRead,
};
pub(in crate::service_plan) use history::SharedHistoryWatch;
pub use history::{LostSharedCommit, ReportedSharedCommit, SharedHistoryPorts, LOST_COMMIT_STRIDE};
pub(crate) use inspection::{SharedDispatchInspection, SharedReportInspection, SharedStart};
pub use plain_folder::{
    placement_without_git, project_history, ChangedFileHashes, FileHashes, ProjectHistory,
    NO_GIT_HISTORY,
};
pub use report::{shared_checkout_head, validate_shared_report};

#[cfg(test)]
fn canonical(plan: &Plan) -> String {
    let path = if plan.canonical_project_path.trim().is_empty() {
        &plan.project_path
    } else {
        &plan.canonical_project_path
    };
    canonical_path(path)
}

pub(in crate::service_plan) fn canonical_path(path: &str) -> String {
    let resolved = std::fs::canonicalize(path).ok();
    codeterm_paths::normalize_cwd(
        resolved
            .as_ref()
            .map(|path| path.to_string_lossy())
            .as_deref()
            .unwrap_or(path),
    )
}

#[cfg(test)]
pub fn validate_exclusive_checkout(
    plans: &[CheckoutPeerRead],
    plan: &Plan,
    task_id: &str,
) -> Result<(), String> {
    validate_exclusive_checkout_at(plans, &plan.id, task_id, &canonical(plan))
}

pub(crate) fn validate_exclusive_checkout_at(
    plans: &[CheckoutPeerRead],
    plan_id: &str,
    task_id: &str,
    checkout: &str,
) -> Result<(), String> {
    for owner_plan in plans
        .iter()
        .filter(|other| canonical_path(&other.checkout) == canonical_path(checkout))
    {
        for owner in &owner_plan.tasks {
            if owner_plan.id == plan_id && owner.id == task_id {
                continue;
            }
            if owner
                .execution_attempts
                .iter()
                .any(|attempt| !attempt.placement.is_isolated_lane())
            {
                return Err(format!(
                    "canonical checkout is held by task '{}' in plan '{}'",
                    owner.id, owner_plan.id
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub fn validate_shared_files(
    plans: &[CheckoutPeerRead],
    plan: &Plan,
    task: &Task,
    files: &TaskFiles,
) -> Result<(), String> {
    files.shared_edit_paths()?;
    validate_shared_files_at(plans, &plan.id, &task.id, &canonical(plan), files)
}

pub(crate) fn validate_shared_files_at(
    plans: &[CheckoutPeerRead],
    plan_id: &str,
    task_id: &str,
    checkout: &str,
    files: &TaskFiles,
) -> Result<(), String> {
    files.shared_edit_paths()?;
    validate_edit_links_at(checkout, files)?;
    validate_shared_ownership_at(plans, plan_id, task_id, checkout, files)
}

pub(crate) fn validate_shared_ownership_at(
    plans: &[CheckoutPeerRead],
    plan_id: &str,
    task_id: &str,
    checkout: &str,
    files: &TaskFiles,
) -> Result<(), String> {
    for owner_plan in plans
        .iter()
        .filter(|other| canonical_path(&other.checkout) == canonical_path(checkout))
    {
        for owner in &owner_plan.tasks {
            if owner_plan.id == plan_id && owner.id == task_id {
                continue;
            }
            for attempt in &owner.execution_attempts {
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

fn validate_edit_links_at(checkout: &str, files: &TaskFiles) -> Result<(), String> {
    let root = std::path::PathBuf::from(checkout);
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

#[cfg(test)]
fn choose_placement(
    plans: &[CheckoutPeerRead],
    plan: &Plan,
    task: &Task,
    requested: Option<&str>,
) -> Result<(WorkerPlacement, String), String> {
    choose_placement_facts(
        plans,
        &plan.id,
        &task.id,
        &canonical(plan),
        &task.files,
        task.lane.worktree_path.is_some() || task.lane.branch.is_some(),
        task.tags.iter().any(|tag| tag == "build-breaking"),
        requested,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn choose_placement_facts(
    plans: &[CheckoutPeerRead],
    plan_id: &str,
    task_id: &str,
    checkout: &str,
    files: &TaskFiles,
    recorded_lane: bool,
    build_breaking: bool,
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
    let checkout = canonical_path(checkout);
    let lane_reason = if recorded_lane {
        Some("resume recorded lane".into())
    } else if build_breaking {
        Some("task is marked build-breaking".into())
    } else {
        files.shared_edit_paths().map_err(|error| format!("{error}. Declare edit files: domios plan task files {} {} --edit-file <path>; or request isolation: domios plan task dispatch {} {} --execution-intent mutating --placement lane", plan_id, task_id, plan_id, task_id))?;
        validate_edit_links_at(&checkout, files)?;
        match validate_shared_ownership_at(plans, plan_id, task_id, &checkout, files) {
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
    pub fn extend_task_edit_files(
        &self,
        plan_id: &str,
        task_id: &str,
        paths: Vec<String>,
        now: u64,
    ) -> Result<(), PlanError> {
        let writer_turn = self.mutation_writer.enter();
        let task = self.task_checkout_projection(plan_id, task_id, true)?;
        if !task.live_shared {
            return Err(PlanError::InvalidPlanText(
                "only a live shared Worker can extend edit files".into(),
            ));
        }
        let added = TaskFiles {
            edit: paths
                .iter()
                .filter(|path| !task.files.owns_edit_path(path))
                .cloned()
                .collect(),
            ..Default::default()
        };
        let mut files = task.files.clone();
        files.edit.extend(paths);
        let files = files.validated().map_err(PlanError::InvalidPlanText)?;
        validate_shared_files_at(
            &self.checkout_peer_facts(&task.checkout)?,
            plan_id,
            &task.id,
            &canonical_path(&task.checkout),
            &files,
        )
        .map_err(PlanError::InvalidPlanText)?;
        // Without git, a newly declared path needs its own start hash to show a change later.
        let added_start = if !added.edit.is_empty()
            && project_history(&task.checkout).map_err(PlanError::InvalidPlanText)?
                == ProjectHistory::NoGit
        {
            plain_folder::hash_declared_files(&task.checkout, &added)
                .map_err(PlanError::InvalidPlanText)?
        } else {
            FileHashes::new()
        };
        self.with_task_rows_in_writer(
            &writer_turn,
            plan_id,
            task_id,
            if added_start.is_empty() {
                super::store_db::hot_write::TaskHistory::Core
            } else {
                super::store_db::hot_write::TaskHistory::LiveAndReported
            },
            now,
            |edit| {
                let task = &mut edit.task;
                task.files = files;
                for attempt in task.execution_attempts.iter_mut().filter(|attempt| {
                    attempt.placement == WorkerPlacement::Shared
                        && attempt.closed_at.is_none()
                        && attempt.retired_at.is_none()
                }) {
                    if let Some(start) = attempt.shared_start_files.as_mut() {
                        for (path, hash) in &added_start {
                            start.entry(path.clone()).or_insert_with(|| hash.clone());
                        }
                    }
                }
                task.updated_at = now;
                Ok(())
            },
        )
    }
}

#[cfg(test)]
mod tests;
