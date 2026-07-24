use std::path::{Path, PathBuf};
use std::process::Command;

use super::orchestrator::SpawnAgentParams;
use super::spawn_guard::{is_canonical_checkout, is_worker_spawn};
use super::ServiceContext;
use crate::process_util;

#[derive(Debug)]
pub(crate) struct ProvisionedWorkerLane {
    canonical_cwd: String,
    pub(crate) cwd: String,
    branch: String,
    base: String,
}

impl ProvisionedWorkerLane {
    pub(crate) fn verify(&self) -> Result<WorkerLaneProvenance, String> {
        verify_worker_lane(
            &self.canonical_cwd,
            &self.cwd,
            Some(&self.branch),
            Some(&self.base),
        )
    }

    pub(crate) fn cleanup(self) -> Result<(), String> {
        let worktree = run_git(
            &self.canonical_cwd,
            ["worktree", "remove", "--force", &self.cwd],
        );
        let branch = run_git(&self.canonical_cwd, ["branch", "-D", &self.branch]);
        match (worktree, branch) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(worktree), Ok(())) => Err(worktree),
            (Ok(()), Err(branch)) => Err(branch),
            (Err(worktree), Err(branch)) => Err(format!("{worktree}; {branch}")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkerLaneProvenance {
    pub(crate) canonical_cwd: String,
    pub(crate) worktree_path: String,
    pub(crate) branch: String,
    pub(crate) base: String,
}

impl ServiceContext {
    pub(crate) fn provision_worker_lane_for_spawn(
        &self,
        params: &mut SpawnAgentParams,
        lane_label: &str,
    ) -> Result<Option<ProvisionedWorkerLane>, String> {
        if !is_worker_spawn(params.role.as_deref(), params.role_profile.as_deref()) {
            return Ok(None);
        }
        let workspace_cwd = self.resolve_worker_spawn_workspace_cwd(params);
        let effective_cwd = params.cwd.clone().or_else(|| workspace_cwd.clone());
        let Some(cwd) = effective_cwd else {
            return Ok(None);
        };
        if !is_canonical_checkout(&cwd, workspace_cwd.as_deref()) {
            return Ok(None);
        }

        let lane = provision_lane(&cwd, lane_label)?;
        params.cwd = Some(lane.cwd.clone());
        params.provisioned_cwd = Some(lane.cwd.clone());
        Ok(Some(lane))
    }

    fn resolve_worker_spawn_workspace_cwd(&self, params: &SpawnAgentParams) -> Option<String> {
        let mux = self.mux.read();
        let id = crate::service::spawn::resolve_spawn_workspace_id(
            params.workspace_id.clone(),
            params.parent_pane_id.as_deref(),
            |pane_id| mux.find_workspace_for_pane(pane_id),
            mux.get_active_workspace()
                .map(|workspace| workspace.id.clone()),
            mux.list_workspaces()
                .first()
                .map(|workspace| workspace.id.clone()),
        )
        .ok()?;
        mux.get_workspace(&id)
            .and_then(|workspace| workspace.cwd.clone())
    }
}

fn provision_lane(project_cwd: &str, lane_label: &str) -> Result<ProvisionedWorkerLane, String> {
    let project_cwd = codeterm_paths::canonical_cwd(project_cwd);
    let project = Path::new(&project_cwd);
    let repo = project
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            format!("cannot derive a lane container from workspace cwd '{project_cwd}'")
        })?;
    let slug = format!(
        "{}-{}",
        lane_slug(lane_label),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let branch = format!("lane/{slug}");
    let container = project
        .parent()
        .ok_or_else(|| {
            format!("cannot derive a sibling lane container from workspace cwd '{project_cwd}'")
        })?
        .join(format!("{repo}-worktrees"));
    std::fs::create_dir_all(&container).map_err(|error| {
        format!(
            "cannot create worker lane container '{}': {error}",
            container.display()
        )
    })?;
    let lane_path = container.join(&slug);
    let lane_cwd = lane_path.display().to_string();
    let base = git_value(&project_cwd, ["rev-parse", "HEAD"])?;
    if let Err(error) = run_git(
        &project_cwd,
        ["worktree", "add", "-b", &branch, &lane_cwd],
    ) {
        let _ = std::fs::remove_dir_all(&lane_path);
        let _ = run_git(&project_cwd, ["worktree", "prune"]);
        let _ = run_git(&project_cwd, ["branch", "-D", &branch]);
        return Err(error);
    }
    Ok(ProvisionedWorkerLane {
        canonical_cwd: project_cwd,
        cwd: lane_cwd,
        branch,
        base,
    })
}

pub(crate) fn verify_worker_lane(
    canonical_cwd: &str,
    worktree_path: &str,
    expected_branch: Option<&str>,
    expected_base: Option<&str>,
) -> Result<WorkerLaneProvenance, String> {
    let canonical = canonical_path(canonical_cwd)?;
    let worktree = canonical_path(worktree_path)?;
    if canonical == worktree {
        return Err("worker lane must not be the canonical checkout".into());
    }
    if !worktree.is_dir() {
        return Err(format!(
            "worker lane worktree does not exist: {}",
            worktree.display()
        ));
    }
    let root = canonical_path(&git_value(worktree_path, ["rev-parse", "--show-toplevel"])?)?;
    if root != worktree {
        return Err(format!(
            "worker lane root '{}' does not match requested worktree '{}'",
            root.display(),
            worktree.display()
        ));
    }
    let branch = git_value(worktree_path, ["branch", "--show-current"])?;
    if branch.is_empty() {
        return Err("worker lane is on a detached HEAD".into());
    }
    if expected_branch.is_some_and(|expected| expected != branch) {
        return Err(format!(
            "worker lane branch mismatch: expected '{}', found '{}'",
            expected_branch.unwrap_or_default(),
            branch
        ));
    }
    let base = match expected_base
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(expected) => {
            let commit_ref = format!("{expected}^{{commit}}");
            let resolved = git_value(canonical_cwd, ["rev-parse", "--verify", &commit_ref])?;
            run_git(
                canonical_cwd,
                ["merge-base", "--is-ancestor", &resolved, &branch],
            )
            .map_err(|_| {
                format!(
                    "worker lane base '{}' is not an ancestor of branch '{}'",
                    expected, branch
                )
            })?;
            resolved
        }
        None => git_value(canonical_cwd, ["merge-base", "HEAD", &branch])?,
    };
    Ok(WorkerLaneProvenance {
        canonical_cwd: canonical.display().to_string(),
        worktree_path: worktree.display().to_string(),
        branch,
        base,
    })
}

#[cfg(test)]
pub(crate) fn provision_lane_for_test(
    project_cwd: &str,
    lane_label: &str,
) -> Result<ProvisionedWorkerLane, String> {
    provision_lane(project_cwd, lane_label)
}

fn lane_slug(value: &str) -> String {
    let slug: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "worker".into()
    } else {
        slug.chars().take(48).collect()
    }
}

fn run_git<'a>(project_cwd: &str, args: impl IntoIterator<Item = &'a str>) -> Result<(), String> {
    let args: Vec<&str> = args.into_iter().collect();
    let mut command = Command::new("git");
    process_util::no_window_std(&mut command);
    let output = command
        .current_dir(project_cwd)
        .args(&args)
        .output()
        .map_err(|error| format!("failed to provision worker lane: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "failed to provision worker lane: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

fn git_value<'a>(
    project_cwd: &str,
    args: impl IntoIterator<Item = &'a str>,
) -> Result<String, String> {
    let args: Vec<&str> = args.into_iter().collect();
    let mut command = Command::new("git");
    process_util::no_window_std(&mut command);
    let output = command
        .current_dir(project_cwd)
        .args(&args)
        .output()
        .map_err(|error| format!("failed to inspect worker lane: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "failed to inspect worker lane: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn canonical_path(path: &str) -> Result<PathBuf, String> {
    std::fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve worker lane path '{path}': {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn git(dir: &Path, args: &[&str]) {
        let mut command = Command::new("git");
        process_util::no_window_std(&mut command);
        let output = command.current_dir(dir).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn canonical_checkout_provisions_a_sibling_worker_lane() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        git(&project, &["init", "-q"]);
        git(&project, &["config", "user.email", "test@example.com"]);
        git(&project, &["config", "user.name", "Test"]);
        std::fs::write(project.join("README.md"), "fixture").unwrap();
        git(&project, &["add", "README.md"]);
        git(&project, &["commit", "-qm", "fixture"]);

        let canonical_project = std::fs::canonicalize(&project).unwrap();
        let lane = provision_lane(canonical_project.to_str().unwrap(), "team_test").unwrap();
        let lane_path = PathBuf::from(&lane.cwd);
        assert!(lane_path.is_dir());
        assert_eq!(
            lane_path.parent().unwrap(),
            root.path().join("project-worktrees")
        );
        assert!(lane.branch.starts_with("lane/team-test-"));
        assert!(!is_canonical_checkout(
            &lane.cwd,
            Some(project.to_str().unwrap())
        ));
        lane.cleanup().unwrap();
    }

    #[test]
    fn worker_lane_verification_reads_back_branch_and_base_commit() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        git(&project, &["init", "-q"]);
        git(&project, &["config", "user.email", "test@example.com"]);
        git(&project, &["config", "user.name", "Test"]);
        std::fs::write(project.join("README.md"), "fixture").unwrap();
        git(&project, &["add", "README.md"]);
        git(&project, &["commit", "-qm", "fixture"]);

        let lane = provision_lane(project.to_str().unwrap(), "verify").unwrap();
        let provenance = lane.verify().unwrap();
        assert_eq!(provenance.branch, lane.branch);
        assert_eq!(
            provenance.base,
            git_value(project.to_str().unwrap(), ["rev-parse", "HEAD"]).unwrap()
        );
        let error = verify_worker_lane(
            project.to_str().unwrap(),
            &lane.cwd,
            Some("lane/wrong"),
            Some(&provenance.base),
        )
        .unwrap_err();
        assert!(error.contains("branch mismatch"));
        lane.cleanup().unwrap();
    }
}
