use std::path::Path;
use std::process::Command;

use super::orchestrator::SpawnAgentParams;
use super::spawn_guard::{is_canonical_checkout, is_worker_spawn};
use super::ServiceContext;
use crate::process_util;

#[derive(Debug)]
pub(crate) struct ProvisionedWorkerLane {
    project_cwd: String,
    pub(crate) cwd: String,
    branch: String,
}

impl ProvisionedWorkerLane {
    pub(crate) fn cleanup(self) {
        let _ = run_git(&self.project_cwd, ["worktree", "remove", &self.cwd]);
        let _ = run_git(&self.project_cwd, ["branch", "-D", &self.branch]);
    }
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
    let project = Path::new(project_cwd);
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
    run_git(project_cwd, ["worktree", "add", "-b", &branch, &lane_cwd])?;
    Ok(ProvisionedWorkerLane {
        project_cwd: project_cwd.into(),
        cwd: lane_cwd,
        branch,
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

        let lane = provision_lane(project.to_str().unwrap(), "team_test").unwrap();
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
        lane.cleanup();
    }
}
