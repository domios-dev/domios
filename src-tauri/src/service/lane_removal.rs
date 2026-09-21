use std::path::Path;
use std::process::Command;

use super::ServiceContext;
use crate::process_util;
use crate::service::plan::{PlanStore, TaskLaneUpdate};

/// One live tab as the removal preflight sees it: the shell cwd decides
/// ownership, the title and id are what a refusal has to name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LiveTabCwd {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) cwd: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LaneRemovalRequest {
    pub(crate) worktree_path: String,
    pub(crate) branch: Option<String>,
    /// Canonical checkout the lane was branched from. Derived from the lane's
    /// own git common dir when the caller does not name one.
    pub(crate) canonical_cwd: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LaneRemovalOutcome {
    pub(crate) canonical_cwd: String,
    pub(crate) worktree_path: String,
    pub(crate) branch: Option<String>,
    pub(crate) worktree_already_removed: bool,
    pub(crate) branch_already_removed: bool,
}

pub(crate) fn recorded_lane_remnant(
    worktree_path: Option<&str>,
    branch: Option<&str>,
    canonical_cwd: &str,
) -> Result<Option<String>, String> {
    let worktree_path = non_empty(worktree_path);
    let branch = non_empty(branch);
    if worktree_path.is_none() && branch.is_none() {
        return Ok(None);
    }
    if let Some(path) = worktree_path {
        match std::fs::symlink_metadata(strip_verbatim(&path)) {
            Ok(_) => return Ok(Some(format!("directory '{path}' still exists"))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("cannot inspect recorded lane '{path}': {error}")),
        }
        if worktree_registered(canonical_cwd, &path)? {
            return Ok(Some(format!(
                "Git worktree registration for '{path}' still exists"
            )));
        }
    }
    if let Some(branch) = branch {
        if branch_exists(canonical_cwd, &branch)? {
            return Ok(Some(format!("branch '{branch}' still exists")));
        }
    }
    Ok(None)
}

fn path_components(path: &str) -> Vec<String> {
    codeterm_paths::normalize_cwd(path)
        .replace('\\', "/")
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect()
}

/// A tab holds the lane when its shell cwd is the worktree root or anything
/// under it. Compared component-wise so a sibling lane whose name merely starts
/// with the target's cannot read as inside it.
pub(crate) fn find_lane_holder<'a>(
    worktree_path: &str,
    live_tabs: &'a [LiveTabCwd],
) -> Option<&'a LiveTabCwd> {
    let target = path_components(worktree_path);
    if target.is_empty() {
        return None;
    }
    live_tabs.iter().find(|tab| {
        let candidate = path_components(&tab.cwd);
        candidate.len() >= target.len() && candidate[..target.len()] == target[..]
    })
}

pub(crate) fn holder_refusal(worktree_path: &str, holder: &LiveTabCwd) -> String {
    format!(
        "✗ lane removal refused: tab '{title}' ({id}) is live with its shell in '{cwd}', \
inside the lane worktree '{worktree_path}', so that directory is still in use. Nothing was removed.\n\
close that tab first, then run the removal again:\n  codeterm tab close --tab {id}",
        title = holder.title,
        id = holder.id,
        cwd = holder.cwd,
    )
}

pub(crate) fn remove_lane(
    request: &LaneRemovalRequest,
    live_tabs: &[LiveTabCwd],
) -> Result<LaneRemovalOutcome, String> {
    let worktree = strip_verbatim(request.worktree_path.trim());
    let worktree = worktree.as_str();
    if worktree.is_empty() {
        return Err("lane removal requires a worktree path".into());
    }
    if let Some(holder) = find_lane_holder(worktree, live_tabs) {
        return Err(holder_refusal(worktree, holder));
    }
    let worktree_exists = match std::fs::symlink_metadata(strip_verbatim(worktree)) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            return Err(format!(
                "cannot inspect lane worktree '{worktree}': {error}"
            ))
        }
    };
    let canonical = match request
        .canonical_cwd
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => codeterm_paths::canonical_cwd(value),
        None if worktree_exists => canonical_checkout_for(worktree)?,
        None => {
            return Err(format!(
                "lane worktree '{worktree}' is already absent; pass --project to check its branch"
            ))
        }
    };
    if super::worker_lane::same_path(&canonical, worktree) {
        return Err(format!(
            "✗ lane removal refused: '{worktree}' is the canonical checkout, not a lane worktree. Nothing was removed."
        ));
    }
    let registered = worktree_registered(&canonical, worktree)?;
    if worktree_exists {
        if !registered {
            return Err(format!(
                "lane path '{worktree}' still exists but is not registered as a Git worktree; nothing was removed"
            ));
        }
        run_git(&canonical, ["worktree", "remove", "--force", worktree])
            .map_err(|error| format!("lane removal failed for '{worktree}': {error}"))?;
    } else if registered {
        run_git(&canonical, ["worktree", "prune", "--expire", "now"]).map_err(|error| {
            format!("stale lane registration for '{worktree}' could not be removed: {error}")
        })?;
        if worktree_registered(&canonical, worktree)? {
            return Err(format!(
                "stale lane registration for '{worktree}' remains after pruning"
            ));
        }
    }
    let branch = request
        .branch
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let branch_exists = branch
        .map(|branch| branch_exists(&canonical, branch))
        .transpose()?
        .unwrap_or(false);
    if let Some(branch) = branch {
        if branch_exists {
            run_git(&canonical, ["branch", "-D", branch]).map_err(|error| {
                format!(
                    "lane worktree '{worktree}' was removed but branch '{branch}' was not: {error}"
                )
            })?;
        }
    }
    let branch_already_removed = branch.is_some() && !branch_exists;
    Ok(LaneRemovalOutcome {
        canonical_cwd: canonical,
        worktree_path: worktree.to_string(),
        branch: branch.map(str::to_string),
        worktree_already_removed: !worktree_exists,
        branch_already_removed,
    })
}

/// The checkout a linked worktree belongs to: its git common dir is that
/// checkout's `.git`, so the parent is the canonical working tree.
fn canonical_checkout_for(worktree_path: &str) -> Result<String, String> {
    let common = git_value(
        &strip_verbatim(worktree_path),
        ["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    Path::new(&common)
        .parent()
        .map(|path| codeterm_paths::canonical_cwd(&path.to_string_lossy()))
        .ok_or_else(|| {
            format!("cannot resolve the canonical checkout owning lane '{worktree_path}'")
        })
}

fn git_output<'a>(
    cwd: &str,
    args: impl IntoIterator<Item = &'a str>,
) -> Result<std::process::Output, String> {
    let args: Vec<&str> = args.into_iter().collect();
    let mut command = Command::new("git");
    process_util::no_window_std(&mut command);
    command
        .current_dir(strip_verbatim(cwd))
        .args(&args)
        .output()
        .map_err(|error| format!("git could not be run in '{cwd}': {error}"))
}

fn run_git<'a>(cwd: &str, args: impl IntoIterator<Item = &'a str>) -> Result<(), String> {
    let output = git_output(cwd, args)?;
    if output.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
}

fn git_value<'a>(cwd: &str, args: impl IntoIterator<Item = &'a str>) -> Result<String, String> {
    let output = git_output(cwd, args)?;
    if !output.status.success() {
        return Err(format!(
            "cannot inspect lane '{cwd}': {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn worktree_registered(canonical_cwd: &str, worktree_path: &str) -> Result<bool, String> {
    let output = git_output(canonical_cwd, ["worktree", "list", "--porcelain"])?;
    if !output.status.success() {
        return Err(format!(
            "cannot inspect Git worktree registrations in '{}': {}",
            canonical_cwd,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let expected = codeterm_paths::normalize_cwd(&strip_verbatim(worktree_path));
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .any(|path| codeterm_paths::normalize_cwd(path) == expected))
}

fn branch_exists(canonical_cwd: &str, branch: &str) -> Result<bool, String> {
    let reference = format!("refs/heads/{branch}");
    let output = git_output(
        canonical_cwd,
        ["show-ref", "--verify", "--quiet", &reference],
    )?;
    if output.status.success() {
        Ok(true)
    } else if output.status.code() == Some(1) {
        Ok(false)
    } else {
        Err(format!(
            "cannot inspect branch '{branch}' in '{canonical_cwd}': {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn strip_verbatim(path: &str) -> String {
    codeterm_paths::strip_verbatim_prefix(path)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

pub(crate) fn lane_remove_command(
    worktree_path: &str,
    branch: Option<&str>,
    canonical_cwd: &str,
) -> String {
    let mut command = format!(
        "codeterm plan lane-remove {}",
        shell_quote(&strip_verbatim(worktree_path))
    );
    if let Some(branch) = branch {
        command.push_str(&format!(" --branch {}", shell_quote(branch)));
    }
    command.push_str(&format!(
        " --project {}",
        shell_quote(&strip_verbatim(canonical_cwd))
    ));
    command
}

pub(crate) fn lane_release_command(
    worktree_path: Option<&str>,
    branch: Option<&str>,
    canonical_cwd: &str,
) -> Option<String> {
    if let Some(worktree_path) = worktree_path {
        Some(lane_remove_command(worktree_path, branch, canonical_cwd))
    } else {
        branch.map(|branch| {
            format!(
                "git -C {} branch -D {}",
                shell_quote(&strip_verbatim(canonical_cwd)),
                shell_quote(branch)
            )
        })
    }
}

fn clear_removed_task_lane_references(
    store: &PlanStore,
    outcome: &LaneRemovalOutcome,
) -> Result<(), String> {
    for plan in store.list_full() {
        for task in &plan.tasks {
            let Some(path) = task.lane.worktree_path.as_deref() else {
                continue;
            };
            if codeterm_paths::normalize_cwd(&strip_verbatim(path))
                != codeterm_paths::normalize_cwd(&strip_verbatim(&outcome.worktree_path))
            {
                continue;
            }
            let branch_removed = task
                .lane
                .branch
                .as_deref()
                .map(|task_branch| branch_exists(&outcome.canonical_cwd, task_branch))
                .transpose()?
                .is_none_or(|exists| !exists);
            let update = TaskLaneUpdate {
                tab_id: Some(None),
                workspace_id: Some(None),
                workspace_name: Some(None),
                worktree_path: Some(None),
                branch: branch_removed.then_some(None),
                cleanup_status: branch_removed.then(|| Some("done".into())),
                ..Default::default()
            };
            store
                .update_task_lane(&plan.id, &task.id, update, unix_time_millis())
                .map_err(|error| {
                    format!(
                        "lane was removed but task '{}' in plan '{}' still has lane references: {error}",
                        task.id, plan.id
                    )
                })?;
        }
    }
    Ok(())
}

fn project_for_recorded_lane(store: &PlanStore, worktree_path: &str) -> Option<String> {
    store.list_full().into_iter().find_map(|plan| {
        plan.tasks
            .iter()
            .any(|task| {
                task.lane.worktree_path.as_deref().is_some_and(|path| {
                    codeterm_paths::normalize_cwd(&strip_verbatim(path))
                        == codeterm_paths::normalize_cwd(&strip_verbatim(worktree_path))
                })
            })
            .then(|| {
                if plan.canonical_project_path.trim().is_empty() {
                    plan.project_path
                } else {
                    plan.canonical_project_path
                }
            })
    })
}

fn remove_lane_and_clear_task_references(
    store: &PlanStore,
    request: &LaneRemovalRequest,
    live_tabs: &[LiveTabCwd],
) -> Result<LaneRemovalOutcome, String> {
    let mut request = request.clone();
    if request.canonical_cwd.as_deref().is_none_or(str::is_empty) {
        request.canonical_cwd = project_for_recorded_lane(store, &request.worktree_path);
    }
    let outcome = remove_lane(&request, live_tabs)?;
    clear_removed_task_lane_references(store, &outcome)?;
    Ok(outcome)
}

pub(crate) fn clear_vanished_lane_on_cleanup_done(
    store: &PlanStore,
    plan_id: &str,
    task_id: &str,
    update: Option<TaskLaneUpdate>,
) -> Result<Option<TaskLaneUpdate>, String> {
    let Some(mut update) = update else {
        return Ok(None);
    };
    let is_done = update
        .cleanup_status
        .as_ref()
        .and_then(Option::as_deref)
        .is_some_and(|status| status.eq_ignore_ascii_case("done"));
    if !is_done {
        return Ok(Some(update));
    }
    let Some(plan) = store.get(plan_id) else {
        return Ok(Some(update));
    };
    let Some(task) = plan.tasks.iter().find(|task| task.id == task_id) else {
        return Ok(Some(update));
    };
    let worktree_path = task.lane.worktree_path.as_deref();
    let branch = task.lane.branch.as_deref();
    if worktree_path.is_none() && branch.is_none() {
        return Ok(Some(update));
    }
    let canonical_cwd = if plan.canonical_project_path.trim().is_empty() {
        &plan.project_path
    } else {
        &plan.canonical_project_path
    };
    let remnant = recorded_lane_remnant(worktree_path, branch, canonical_cwd)?;
    if let Some(remnant) = remnant {
        let release = lane_release_command(worktree_path, branch, canonical_cwd)
            .unwrap_or_else(|| "codeterm plan lane-remove <worktree>".into());
        return Err(format!(
            "cleanup is not complete: {remnant}. Release the recorded lane with: {release}"
        ));
    }
    update.tab_id = Some(None);
    update.workspace_id = Some(None);
    update.workspace_name = Some(None);
    update.worktree_path = Some(None);
    update.branch = Some(None);
    Ok(Some(update))
}

fn unix_time_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl ServiceContext {
    pub(crate) fn live_tab_cwds(&self) -> Vec<LiveTabCwd> {
        self.mux
            .read()
            .all_tabs()
            .filter_map(|tab| {
                let cwd = tab.cwd.clone()?;
                (!cwd.trim().is_empty()).then(|| LiveTabCwd {
                    id: tab.id.clone(),
                    title: tab.title.clone(),
                    cwd,
                })
            })
            .collect()
    }

    pub(crate) fn remove_owned_lane(
        &self,
        request: &LaneRemovalRequest,
    ) -> Result<LaneRemovalOutcome, String> {
        remove_lane_and_clear_task_references(&self.plan_store, request, &self.live_tab_cwds())
    }
}

#[cfg(test)]
mod tests;
