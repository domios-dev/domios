use std::path::Path;
use std::process::Command;

use super::ServiceContext;
use crate::process_util;

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
    let worktree = request.worktree_path.trim();
    if worktree.is_empty() {
        return Err("lane removal requires a worktree path".into());
    }
    if let Some(holder) = find_lane_holder(worktree, live_tabs) {
        return Err(holder_refusal(worktree, holder));
    }
    let canonical = match request
        .canonical_cwd
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => codeterm_paths::canonical_cwd(value),
        None => canonical_checkout_for(worktree)?,
    };
    if super::worker_lane::same_path(&canonical, worktree) {
        return Err(format!(
            "✗ lane removal refused: '{worktree}' is the canonical checkout, not a lane worktree. Nothing was removed."
        ));
    }
    run_git(&canonical, ["worktree", "remove", "--force", worktree])
        .map_err(|error| format!("lane removal failed for '{worktree}': {error}"))?;
    let branch = request
        .branch
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(branch) = branch {
        run_git(&canonical, ["branch", "-D", branch]).map_err(|error| {
            format!("lane worktree '{worktree}' was removed but branch '{branch}' was not: {error}")
        })?;
    }
    Ok(LaneRemovalOutcome {
        canonical_cwd: canonical,
        worktree_path: worktree.to_string(),
        branch: branch.map(str::to_string),
    })
}

/// The checkout a linked worktree belongs to: its git common dir is that
/// checkout's `.git`, so the parent is the canonical working tree.
fn canonical_checkout_for(worktree_path: &str) -> Result<String, String> {
    let common = git_value(
        worktree_path,
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
        .current_dir(cwd)
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
        remove_lane(request, &self.live_tab_cwds())
    }
}

#[cfg(test)]
mod tests;
