use std::collections::{HashMap, HashSet};
use std::path::Path;

use codeterm_service_core::plan::LaneOwnership;
use codeterm_service_core::service::lane_removal::{
    find_lane_holder, remove_lane, remove_lane_with_worktree_inventory, LaneRemovalRequest,
    LiveTabCwd,
};

use super::{
    branch_exists, git_output, git_output_error, lane_hold_reason, lane_hold_reason_with_inventory,
    merged_refusal, run_git, unmanaged_reason, TeamCloseLane, TeamLaneCleanupIo,
};

struct RepositoryInventory {
    worktrees: String,
    refs: HashMap<String, String>,
}

impl RepositoryInventory {
    fn load(project: &str) -> Result<Self, String> {
        Self::load_with(project, |project, args| {
            let output = git_output(project, args)?;
            if !output.status.success() {
                return Err(git_output_error(args[0], project, &output));
            }
            String::from_utf8(output.stdout)
                .map_err(|error| format!("invalid Git output in '{project}': {error}"))
        })
    }

    fn load_with(
        project: &str,
        mut git: impl FnMut(&str, &[&str]) -> Result<String, String>,
    ) -> Result<Self, String> {
        let worktrees = git(project, &["worktree", "list", "--porcelain"])?;
        let refs = git(
            project,
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/heads",
            ],
        )?
        .lines()
        .map(|row| {
            row.split_once(' ')
                .map(|(reference, tip)| (reference.to_string(), tip.to_string()))
                .ok_or_else(|| format!("invalid branch inventory in '{project}'"))
        })
        .collect::<Result<_, _>>()?;
        Ok(Self { worktrees, refs })
    }
}

pub(super) struct GitTeamLaneCleanup<'a> {
    live_tabs: &'a [LiveTabCwd],
    repositories: HashMap<String, Result<RepositoryInventory, String>>,
    removed_worktrees: HashSet<String>,
}

impl<'a> GitTeamLaneCleanup<'a> {
    pub(super) fn new(live_tabs: &'a [LiveTabCwd]) -> Self {
        Self {
            live_tabs,
            repositories: HashMap::new(),
            removed_worktrees: HashSet::new(),
        }
    }

    fn inventory(&mut self, project: &str) -> Result<&mut RepositoryInventory, String> {
        self.inventory_with(project, RepositoryInventory::load)
    }

    fn inventory_with(
        &mut self,
        project: &str,
        load: impl FnOnce(&str) -> Result<RepositoryInventory, String>,
    ) -> Result<&mut RepositoryInventory, String> {
        self.repositories
            .entry(project.to_string())
            .or_insert_with(|| load(project))
            .as_mut()
            .map_err(|error| error.clone())
    }
}

#[cfg(test)]
mod tests;

impl TeamLaneCleanupIo for GitTeamLaneCleanup<'_> {
    fn not_managed(&mut self, lane: &TeamCloseLane) -> Option<String> {
        if lane.ownership == Some(LaneOwnership::ForeignCheckout) {
            return unmanaged_reason(lane, Some(""));
        }
        let inventory = self.inventory(&lane.project_path).ok()?;
        unmanaged_reason(lane, Some(&inventory.worktrees))
    }

    fn worktree_exists(&mut self, path: &str) -> bool {
        Path::new(path).exists()
    }

    fn branch_exists(&mut self, lane: &TeamCloseLane, branch: &str) -> Result<bool, String> {
        Ok(self
            .inventory(&lane.project_path)?
            .refs
            .contains_key(&format!("refs/heads/{branch}")))
    }

    fn hold_reason(&mut self, lane: &TeamCloseLane) -> Option<String> {
        if let Some(reason) = &lane.hold_reason {
            return Some(reason.clone());
        }
        if let Some(holder) = lane
            .worktree_path
            .as_deref()
            .and_then(|path| find_lane_holder(path, self.live_tabs))
        {
            return Some(format!("occupied by tab {} ({})", holder.id, holder.title));
        }
        match self.inventory(&lane.project_path) {
            Ok(inventory) => lane_hold_reason_with_inventory(
                lane,
                Some((&inventory.worktrees, &inventory.refs)),
                merged_refusal,
            ),
            Err(error) => Some(error),
        }
    }

    fn remove_lane(&mut self, lane: &TeamCloseLane) -> Result<(), String> {
        let worktree = lane
            .worktree_path
            .as_deref()
            .filter(|path| !self.removed_worktrees.contains(*path));
        let live_tabs = self.live_tabs;
        let inventory = self.inventory(&lane.project_path)?;
        let reference = lane
            .branch
            .as_ref()
            .map(|branch| format!("refs/heads/{branch}"));
        let branch = reference
            .as_deref()
            .filter(|reference| inventory.refs.contains_key(*reference));
        remove_assessed_lane(
            lane,
            live_tabs,
            worktree,
            branch,
            Some(&inventory.worktrees),
        )?;
        if let Some(reference) = branch {
            inventory.refs.remove(reference);
        }
        if let Some(worktree) = worktree {
            self.removed_worktrees.insert(worktree.to_string());
        }
        Ok(())
    }
}

pub(crate) fn cleanup_lane_with_tabs(
    lane: &TeamCloseLane,
    live_tabs: &[LiveTabCwd],
) -> Result<(), String> {
    if let Some(reason) = lane_hold_reason(lane) {
        return Err(reason);
    }
    let branch = match lane.branch.as_deref() {
        Some(branch) if branch_exists(lane, branch)? => Some(format!("refs/heads/{branch}")),
        _ => None,
    };
    remove_assessed_lane(
        lane,
        live_tabs,
        lane.worktree_path.as_deref(),
        branch.as_deref(),
        None,
    )
}

fn remove_assessed_lane(
    lane: &TeamCloseLane,
    live_tabs: &[LiveTabCwd],
    worktree: Option<&str>,
    branch: Option<&str>,
    worktrees: Option<&str>,
) -> Result<(), String> {
    let branch_tip = if let Some(reference) = branch {
        let output = git_output(&lane.project_path, &["rev-parse", "--verify", reference])?;
        if !output.status.success() {
            return Err(git_output_error("rev-parse", &lane.project_path, &output));
        }
        let tip = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let destination = lane
            .destination
            .as_deref()
            .ok_or_else(|| "lane destination is not recorded".to_string())?;
        if let Some(reason) = merged_refusal(&lane.project_path, &tip, destination) {
            return Err(reason);
        }
        Some((reference, tip))
    } else {
        None
    };
    if let Some(path) = worktree {
        if let Some(branch) = lane.branch.as_deref().filter(|_| Path::new(path).exists()) {
            let output = git_output(path, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
            if !output.status.success() || String::from_utf8_lossy(&output.stdout).trim() != branch
            {
                return Err("worktree HEAD does not match its recorded lane branch".into());
            }
        }
        let request = LaneRemovalRequest {
            worktree_path: path.to_string(),
            branch: None,
            canonical_cwd: Some(lane.project_path.clone()),
            force: false,
            ownership: lane.ownership,
        };
        match worktrees {
            Some(worktrees) => remove_lane_with_worktree_inventory(&request, live_tabs, worktrees)?,
            None => remove_lane(&request, live_tabs)?,
        };
    }
    if let Some((reference, tip)) = branch_tip {
        let current = git_output(&lane.project_path, &["rev-parse", "--verify", reference])?;
        if !current.status.success() || String::from_utf8_lossy(&current.stdout).trim() != tip {
            return Err(format!(
                "branch {reference} changed during lane removal; branch retained"
            ));
        }
        let branch = reference
            .strip_prefix("refs/heads/")
            .ok_or("invalid lane branch reference")?;
        run_git(&lane.project_path, &["branch", "-d", "--", branch])
            .map_err(|error| format!("branch {reference} could not be deleted: {error}"))?;
    }
    Ok(())
}
