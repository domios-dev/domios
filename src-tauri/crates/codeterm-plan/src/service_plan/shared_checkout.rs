use super::{PlanError, PlanStore};
#[cfg(test)]
use crate::plan::Plan;
use crate::plan::{
    ExecutionIntent, Task, TaskExecutionAttempt, TaskFiles, TaskStatus, WorkerPlacement,
};
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

pub fn placement_reserves_checkout_writer(placement: WorkerPlacement) -> bool {
    matches!(
        placement,
        WorkerPlacement::CanonicalWriter | WorkerPlacement::Shared
    )
}

pub fn has_exact_confirmed_executor_termination(attempt: &TaskExecutionAttempt) -> bool {
    attempt.mesh_peer.is_none()
        && !attempt.id.trim().is_empty()
        && !attempt.executor_tab_id.trim().is_empty()
        && attempt
            .executor_termination
            .as_ref()
            .is_some_and(|receipt| {
                receipt.attempt_id == attempt.id
                    && receipt.executor_tab_id == attempt.executor_tab_id
                    && receipt.session_id == attempt.session_id
                    && receipt.session_binding_nonce == attempt.session_binding_nonce
            })
}

pub fn attempt_reserves_checkout_writer(task: &Task, attempt: &TaskExecutionAttempt) -> bool {
    attempt.closed_at.is_none()
        && attempt.retired_at.is_none()
        && task.execution_intent.requires_repository()
        && placement_reserves_checkout_writer(attempt.placement)
        && attempt.execution_intent != Some(ExecutionIntent::ReadOnly)
        && !(task.status == TaskStatus::Cancelled
            && has_exact_confirmed_executor_termination(attempt))
}

pub fn attempt_recorded_checkout<'a>(
    attempt: &'a TaskExecutionAttempt,
    plan_checkout: &'a str,
) -> &'a str {
    attempt
        .checkout_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .or_else(|| {
            attempt
                .worktree_path
                .as_deref()
                .map(str::trim)
                .filter(|path| !path.is_empty())
        })
        .unwrap_or(plan_checkout)
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

pub fn resolved_git_primary_checkout_path(path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() || !std::path::Path::new(trimmed).exists() {
        return None;
    }
    let (git_dir, common_dir) =
        codeterm_util::git_checkout::paths(std::path::Path::new(trimmed)).ok()?;
    if let Some(primary) = common_dir
        .parent()
        .filter(|_| common_dir.file_name() == Some(std::ffi::OsStr::new(".git")))
    {
        return Some(codeterm_paths::canonical_cwd(&primary.to_string_lossy()));
    }
    if git_dir == common_dir {
        let resolved = std::fs::canonicalize(trimmed).ok();
        return Some(codeterm_paths::canonical_cwd(
            resolved
                .as_ref()
                .map(|path| path.to_string_lossy())
                .as_deref()
                .unwrap_or(trimmed),
        ));
    }
    Some(codeterm_paths::canonical_cwd(&common_dir.to_string_lossy()))
}

pub fn resolved_checkout_target(path: &str) -> String {
    resolved_git_primary_checkout_path(path)
        .map(|primary| canonical_path(&primary))
        .unwrap_or_else(|| canonical_path(path))
}

fn attempt_matches_checkout(
    attempt: &CheckoutAttemptRead,
    plan_checkout: &str,
    target_checkout: &str,
) -> bool {
    let effective = attempt
        .checkout_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .unwrap_or(plan_checkout);
    !effective.trim().is_empty() && resolved_checkout_target(effective) == target_checkout
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
    let target = resolved_checkout_target(checkout);
    for owner_plan in plans {
        for owner in &owner_plan.tasks {
            if owner_plan.id == plan_id && owner.id == task_id {
                continue;
            }
            if owner.execution_attempts.iter().any(|attempt| {
                placement_reserves_checkout_writer(attempt.placement)
                    && attempt_matches_checkout(attempt, &owner_plan.checkout, &target)
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
    let target = resolved_checkout_target(checkout);
    for owner_plan in plans {
        for owner in &owner_plan.tasks {
            if owner_plan.id == plan_id && owner.id == task_id {
                continue;
            }
            for attempt in &owner.execution_attempts {
                if !attempt_matches_checkout(attempt, &owner_plan.checkout, &target) {
                    continue;
                }
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
        validate_shared_ownership_at(plans, plan_id, task_id, &checkout, files).err()
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
                        && attempt.execution_intent != Some(ExecutionIntent::ReadOnly)
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
