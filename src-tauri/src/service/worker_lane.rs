use std::path::{Path, PathBuf};
use std::process::Command;

use super::orchestrator::SpawnAgentParams;
use super::ServiceContext;
use crate::process_util;

fn is_worker_spawn(role: Option<&str>, _role_profile: Option<&str>) -> bool {
    role == Some("Worker")
}

fn is_canonical_checkout(cwd: &str, workspace_cwd: Option<&str>) -> bool {
    if let Some(workspace_cwd) = workspace_cwd {
        if same_path(cwd, workspace_cwd) {
            return git_is_primary_checkout(Path::new(cwd));
        }
    }
    git_is_primary_checkout(Path::new(cwd))
}

/// Two spellings of the same directory. Resolves both sides before comparing,
/// because a textual match is not the question: on macOS a tempdir reached as
/// `/var/...` and the same directory resolved to `/private/var/...` name one
/// place and differ as strings. Falls back to the textual compare only when a
/// side cannot be resolved, so a not-yet-existing path still behaves.
pub(crate) fn same_path(a: &str, b: &str) -> bool {
    match (std::fs::canonicalize(a).ok(), std::fs::canonicalize(b).ok()) {
        (Some(a), Some(b)) => a == b,
        _ => ServiceContext::paths_equal(a, b),
    }
}

fn git_is_primary_checkout(cwd: &Path) -> bool {
    git_checkout_paths(cwd).is_some_and(|(git_dir, common_dir)| git_dir == common_dir)
}

pub(crate) fn is_linked_worktree(cwd: &Path) -> bool {
    git_checkout_paths(cwd).is_some_and(|(git_dir, common_dir)| git_dir != common_dir)
}

fn git_checkout_paths(cwd: &Path) -> Option<(PathBuf, PathBuf)> {
    let mut command = Command::new("git");
    process_util::no_window_std(&mut command);
    let output = match command
        .current_dir(cwd)
        .args([
            "rev-parse",
            "--path-format=absolute",
            "--git-dir",
            "--git-common-dir",
        ])
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return None,
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    match (lines.next(), lines.next()) {
        (Some(git_dir), Some(common_dir)) => {
            Some((normalize_path(git_dir), normalize_path(common_dir)))
        }
        _ => None,
    }
}

fn normalize_path(path: &str) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path))
}

/// Whether the guards that exist to protect mutating work apply to this lane
/// check. Constructible only from the dispatch-time intent, so the task-level
/// `TaskExecutionIntent` cannot be selected at these call sites by mistake.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LaneProtection {
    Enforced,
    Waived,
}

impl LaneProtection {
    pub(crate) fn for_dispatch_intent(intent: &crate::plan::ExecutionIntent) -> Self {
        match intent {
            crate::plan::ExecutionIntent::ReadOnly => Self::Waived,
            crate::plan::ExecutionIntent::Mutating | crate::plan::ExecutionIntent::Unspecified => {
                Self::Enforced
            }
        }
    }

    pub(crate) fn is_waived(self) -> bool {
        matches!(self, Self::Waived)
    }
}

#[derive(Debug)]
pub(crate) struct ProvisionedWorkerLane {
    canonical_cwd: String,
    pub(crate) cwd: String,
    pub(crate) branch: String,
    base: String,
}

impl ProvisionedWorkerLane {
    #[cfg(test)]
    pub(crate) fn base(&self) -> &str {
        &self.base
    }

    pub(crate) fn verify(&self) -> Result<WorkerLaneProvenance, String> {
        verify_worker_lane(
            &self.canonical_cwd,
            &self.cwd,
            Some(&self.branch),
            Some(&self.base),
            LaneProtection::Enforced,
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
    /// The canonical branch the lane integrates into; None for a detached canonical HEAD.
    pub(crate) destination: Option<String>,
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

    pub(crate) fn resolve_spawn_workspace_identity(
        &self,
        params: &SpawnAgentParams,
    ) -> Option<(String, String)> {
        let mux = self.mux.read();
        let id = crate::service::spawn::resolve_spawn_workspace_id(
            params.workspace_id.clone(),
            params.parent_tab_id.as_deref(),
            crate::service::spawn::params_caller_tab_id(params),
            |tab_id| mux.find_workspace_for_tab(tab_id),
            mux.get_active_workspace()
                .map(|workspace| workspace.id.clone()),
            mux.list_workspaces()
                .first()
                .map(|workspace| workspace.id.clone()),
        )
        .ok()?;
        mux.get_workspace(&id)
            .map(|workspace| (workspace.id.clone(), workspace.name.clone()))
    }

    pub(crate) fn worker_spawn_workspace_cwd(&self, params: &SpawnAgentParams) -> Option<String> {
        self.resolve_worker_spawn_workspace_cwd(params)
    }

    /// Dispatcher-owned provisioning. Unlike `provision_worker_lane_for_spawn`
    /// it never no-ops: the dispatcher has already decided this checkout is the
    /// base to branch from, so inferring that again could only disagree.
    pub(crate) fn provision_dispatch_worker_lane(
        &self,
        params: &mut SpawnAgentParams,
        base_checkout: &str,
        lane_label: &str,
    ) -> Result<ProvisionedWorkerLane, String> {
        let lane = provision_lane(base_checkout, lane_label)?;
        params.cwd = Some(lane.cwd.clone());
        params.provisioned_cwd = Some(lane.cwd.clone());
        Ok(lane)
    }

    fn resolve_worker_spawn_workspace_cwd(&self, params: &SpawnAgentParams) -> Option<String> {
        let mux = self.mux.read();
        let id = crate::service::spawn::resolve_spawn_workspace_id(
            params.workspace_id.clone(),
            params.parent_tab_id.as_deref(),
            crate::service::spawn::params_caller_tab_id(params),
            |tab_id| mux.find_workspace_for_tab(tab_id),
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

/// Unconditional sibling-lane provisioning from canonical. Unlike
/// `provision_worker_lane_for_spawn` it never no-ops on an already-shared cwd.
pub(crate) fn provision_isolated_lane(
    canonical_cwd: &str,
    lane_label: &str,
) -> Result<ProvisionedWorkerLane, String> {
    provision_lane(canonical_cwd, lane_label)
}

pub(crate) fn provision_isolated_lane_at_base(
    canonical_cwd: &str,
    lane_label: &str,
    base: &str,
) -> Result<ProvisionedWorkerLane, String> {
    provision_lane_at_base(canonical_cwd, lane_label, base)
}

pub(crate) fn repository_identity(cwd: &str) -> Result<String, String> {
    let remotes = git_value(cwd, ["remote"])?;
    let mut identities = Vec::new();
    for remote in remotes.lines().map(str::trim).filter(|name| !name.is_empty()) {
        let urls = git_value(cwd, ["remote", "get-url", "--all", remote])?;
        identities.extend(urls.lines().filter_map(sanitized_remote_identity));
    }
    identities.sort();
    identities.dedup();
    if identities.is_empty() {
        return Err("checkout has no configured repository remote identity".into());
    }
    use sha2::{Digest, Sha256};
    Ok(format!("sha256:{:x}", Sha256::digest(identities.join("\n").as_bytes())))
}

pub(crate) fn contains_commit(cwd: &str, commit: &str) -> bool {
    !commit.trim().is_empty()
        && git_value(cwd, ["cat-file", "-e", &format!("{}^{{commit}}", commit.trim())])
            .is_ok()
}

pub(crate) fn is_ancestor(cwd: &str, ancestor: &str, descendant: &str) -> bool {
    git_success(cwd, &["merge-base", "--is-ancestor", ancestor, descendant])
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RemoteLaneMerge {
    pub(crate) lane_tip: String,
    pub(crate) target_tip: String,
}

pub(crate) fn integrate_remote_lane_bundle(
    canonical_cwd: &str,
    bundle: &[u8],
    branch: &str,
    attempt_id: &str,
    base: &str,
    expected_lane_tip: &str,
    destination_branch: &str,
) -> Result<RemoteLaneMerge, String> {
    let source_ref = format!("refs/heads/{branch}");
    let target_ref = format!("refs/codeterm/mesh/{attempt_id}");
    for reference in [&source_ref, &target_ref] {
        git_value(canonical_cwd, ["check-ref-format", reference])?;
    }
    if !branch.starts_with("lane/") || attempt_id.is_empty() {
        return Err("remote lane identity is invalid".into());
    }
    let _target_ref_cleanup = RemoteLaneRefCleanup {
        cwd: canonical_cwd.to_string(),
        reference: target_ref.clone(),
    };
    if current_branch(canonical_cwd)? != destination_branch {
        return Err("origin checkout is not on the recorded destination branch".into());
    }
    let dirty = run_git(canonical_cwd, ["status", "--porcelain", "--untracked-files=all"])?;
    if !dirty.trim().is_empty() {
        return Err("origin checkout must be clean before remote lane integration".into());
    }
    let temp_dir = tempfile::tempdir()
        .map_err(|error| format!("cannot allocate remote lane bundle: {error}"))?;
    let bundle_path = temp_dir.path().join("remote-lane.bundle");
    std::fs::write(&bundle_path, bundle)
        .map_err(|error| format!("cannot write remote lane bundle: {error}"))?;
    run_git(canonical_cwd, ["bundle", "verify", bundle_path.to_string_lossy().as_ref()])?;
    run_git(
        canonical_cwd,
        [
            "fetch",
            "--no-tags",
            bundle_path.to_string_lossy().as_ref(),
            &format!("{source_ref}:{target_ref}"),
        ],
    )?;
    let lane_tip = git_value(canonical_cwd, ["rev-parse", &target_ref])?;
    if lane_tip != expected_lane_tip {
        return Err("peer bundle lane tip differs from the authenticated response".into());
    }
    if run_git(canonical_cwd, ["merge-base", "--is-ancestor", base, &lane_tip]).is_err() {
        return Err("peer lane does not descend from the recorded base".to_string());
    }
    let current_tip = git_value(canonical_cwd, ["rev-parse", "HEAD"])?;
    if is_ancestor(canonical_cwd, &lane_tip, &current_tip) {
        return Ok(RemoteLaneMerge {
            lane_tip,
            target_tip: current_tip,
        });
    }
    if !is_ancestor(canonical_cwd, base, &current_tip) {
        return Err("origin target no longer descends from the dispatched base".into());
    }
    if current_branch(canonical_cwd)? != destination_branch
        || !run_git(canonical_cwd, ["status", "--porcelain", "--untracked-files=all"])?.trim().is_empty()
    {
        return Err("origin checkout changed before remote lane merge".into());
    }
    if let Err(error) = run_git(canonical_cwd, ["merge", "--no-ff", "--no-edit", &target_ref]) {
        let _ = run_git(canonical_cwd, ["merge", "--abort"]);
        return Err(format!("real merge of peer lane failed: {error}"));
    }
    let target_tip = git_value(canonical_cwd, ["rev-parse", "HEAD"])?;
    if !is_ancestor(canonical_cwd, &lane_tip, &target_tip) {
        return Err("merged target does not contain the peer lane tip".to_string());
    }
    Ok(RemoteLaneMerge {
        lane_tip,
        target_tip,
    })
}

struct RemoteLaneRefCleanup {
    cwd: String,
    reference: String,
}

impl Drop for RemoteLaneRefCleanup {
    fn drop(&mut self) {
        let _ = run_git(&self.cwd, ["update-ref", "-d", &self.reference]);
    }
}

fn sanitized_remote_identity(remote: &str) -> Option<String> {
    let remote = remote.trim();
    if remote.is_empty() {
        return None;
    }
    if let Some((scheme, rest)) = remote.split_once("://") {
        let authority_end = rest.find('/').unwrap_or(rest.len());
        let authority = &rest[..authority_end];
        let host = authority.rsplit('@').next().unwrap_or(authority);
        let path = &rest[authority_end..];
        let path = path.split(['?', '#']).next().unwrap_or(path);
        return Some(format!(
            "{}://{}{}",
            scheme.to_lowercase(),
            host.to_lowercase(),
            path.trim_end_matches('/')
        ));
    }
    Some(remote.to_string())
}

fn provision_lane(project_cwd: &str, lane_label: &str) -> Result<ProvisionedWorkerLane, String> {
    let project_cwd = codeterm_paths::canonical_cwd(project_cwd);
    let base = git_value(&project_cwd, ["rev-parse", "HEAD"])?;
    provision_lane_at_base(&project_cwd, lane_label, &base)
}

fn provision_lane_at_base(
    project_cwd: &str,
    lane_label: &str,
    requested_base: &str,
) -> Result<ProvisionedWorkerLane, String> {
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
    let base = git_value(
        &project_cwd,
        ["rev-parse", "--verify", &format!("{}^{{commit}}", requested_base)],
    )?;
    if let Err(error) = run_git(
        &project_cwd,
        ["worktree", "add", "-b", &branch, &lane_cwd, &base],
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
    protection: LaneProtection,
) -> Result<WorkerLaneProvenance, String> {
    let canonical = canonical_path(canonical_cwd)?;
    let worktree = canonical_path(worktree_path)?;
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
    let branch = current_branch(worktree_path)?;
    if branch.is_empty() && !protection.is_waived() {
        return Err("worker lane is on a detached HEAD".into());
    }
    if expected_branch.is_some_and(|expected| expected != branch) {
        return Err(format!(
            "worker lane branch mismatch: expected '{}', found '{}'",
            expected_branch.unwrap_or_default(),
            branch
        ));
    }
    // A waived lane may be detached, so ancestry is measured against the commit
    // the lane actually holds rather than a branch name that does not exist.
    let head_ref = if branch.is_empty() {
        git_value(worktree_path, ["rev-parse", "HEAD"])?
    } else {
        branch.clone()
    };
    let base = match expected_base
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(expected) => {
            let commit_ref = format!("{expected}^{{commit}}");
            let resolved = git_value(canonical_cwd, ["rev-parse", "--verify", &commit_ref])?;
            run_git(
                canonical_cwd,
                ["merge-base", "--is-ancestor", &resolved, &head_ref],
            )
            .map_err(|_| {
                format!(
                    "worker lane base '{}' is not an ancestor of '{}'",
                    expected, head_ref
                )
            })?;
            resolved
        }
        None => git_value(canonical_cwd, ["merge-base", "HEAD", &head_ref])?,
    };
    let git_cwd = codeterm_paths::strip_verbatim_prefix(canonical_cwd);
    let destination = match git_value(&git_cwd, ["symbolic-ref", "--short", "-q", "HEAD"]) {
        Ok(name) if !name.is_empty() => Some(name),
        Ok(_) => None,
        Err(error) => {
            log::warn!("[worker-lane] no destination branch recorded for {git_cwd}: {error}");
            None
        }
    };
    Ok(WorkerLaneProvenance {
        canonical_cwd: canonical.display().to_string(),
        worktree_path: worktree.display().to_string(),
        branch,
        base,
        destination,
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

fn git_success(project_cwd: &str, args: &[&str]) -> bool {
    let mut command = Command::new("git");
    process_util::no_window_std(&mut command);
    command.current_dir(project_cwd).args(args).status().is_ok_and(|status| status.success())
}

// `git branch --show-current` needs git 2.22, and a GUI-launched daemon inherits
// the launchd PATH, where Apple's bundled git can still be 2.21.
pub(crate) fn current_branch(worktree_path: &str) -> Result<String, String> {
    let output = git_output(
        worktree_path,
        ["symbolic-ref", "--quiet", "--short", "HEAD"],
    )?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }
    // `--quiet` reduces a detached HEAD to a silent exit 1; git dies with 128 and
    // cannot run at all without a binary, so neither may read as an empty branch.
    if output.status.code() == Some(1) && output.stdout.is_empty() {
        return Ok(String::new());
    }
    Err(inspect_failure(&output.stderr))
}

fn git_output<'a>(
    project_cwd: &str,
    args: impl IntoIterator<Item = &'a str>,
) -> Result<std::process::Output, String> {
    let args: Vec<&str> = args.into_iter().collect();
    let mut command = Command::new("git");
    process_util::no_window_std(&mut command);
    command
        .current_dir(project_cwd)
        .args(&args)
        .output()
        .map_err(|error| format!("failed to inspect worker lane: {error}"))
}

fn inspect_failure(stderr: &[u8]) -> String {
    format!(
        "failed to inspect worker lane: {}",
        String::from_utf8_lossy(stderr).trim()
    )
}

pub(crate) fn git_value<'a>(
    project_cwd: &str,
    args: impl IntoIterator<Item = &'a str>,
) -> Result<String, String> {
    let output = git_output(project_cwd, args)?;
    if !output.status.success() {
        return Err(inspect_failure(&output.stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// The resolved path is persisted as lane provenance and later handed to `git`
/// as a working directory, which rejects Windows verbatim (`\\?\`) prefixes.
fn canonical_path(path: &str) -> Result<PathBuf, String> {
    std::fs::canonicalize(path)
        .map(|resolved| {
            PathBuf::from(codeterm_paths::strip_verbatim_prefix(
                &resolved.to_string_lossy(),
            ))
        })
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

    fn source_only_fixture() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        git(&project, &["init", "-q"]);
        git(&project, &["config", "user.email", "test@example.com"]);
        git(&project, &["config", "user.name", "Test"]);
        std::fs::write(project.join("README.md"), "fixture").unwrap();
        git(&project, &["add", "README.md"]);
        git(&project, &["commit", "-qm", "fixture"]);
        std::fs::create_dir(project.join("node_modules")).unwrap();
        std::fs::write(project.join("node_modules/marker.txt"), "canonical").unwrap();
        (root, project)
    }

    fn remote_bundle_fixture() -> (tempfile::TempDir, PathBuf, String, String, Vec<u8>) {
        let (_root, origin) = source_only_fixture();
        std::fs::remove_dir_all(origin.join("node_modules")).unwrap();
        let base = git_value(origin.to_str().unwrap(), ["rev-parse", "HEAD"]).unwrap();
        let peer_root = tempfile::tempdir().unwrap();
        let peer = peer_root.path().join("peer");
        let peer_str = peer.to_str().unwrap();
        let mut clone = Command::new("git");
        process_util::no_window_std(&mut clone);
        let output = clone.args(["clone", "-q", "--shared"])
            .arg(&origin).arg(&peer).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        git(&peer, &["config", "user.email", "peer@example.com"]);
        git(&peer, &["config", "user.name", "Peer"]);
        git(&peer, &["switch", "-c", "lane/remote-test"]);
        std::fs::write(peer.join("remote.txt"), "peer work").unwrap();
        git(&peer, &["add", "remote.txt"]);
        git(&peer, &["commit", "-qm", "peer work"]);
        let lane_tip = git_value(peer_str, ["rev-parse", "HEAD"]).unwrap();
        let bundle_dir = tempfile::tempdir().unwrap();
        let bundle_file = bundle_dir.path().join("lane.bundle");
        let mut bundle = Command::new("git");
        process_util::no_window_std(&mut bundle);
        let output = bundle.current_dir(&peer).args(["bundle", "create"])
            .arg(&bundle_file).arg("refs/heads/lane/remote-test").output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let bundle_bytes = std::fs::read(&bundle_file).unwrap();
        (_root, origin, base, lane_tip, bundle_bytes)
    }

    #[test]
    fn mesh_remote_lane_merge_records_ancestry_after_real_merge() {
        let (_root, origin, base, lane_tip, bundle_bytes) = remote_bundle_fixture();

        let result = integrate_remote_lane_bundle(
            origin.to_str().unwrap(),
            &bundle_bytes,
            "lane/remote-test",
            "attempt_remote_test",
            &base,
            &lane_tip,
            &current_branch(origin.to_str().unwrap()).unwrap(),
        ).unwrap();
        assert_eq!(result.lane_tip, lane_tip);
        assert!(is_ancestor(origin.to_str().unwrap(), &result.lane_tip, &result.target_tip));
        let parents = git_value(origin.to_str().unwrap(), ["rev-list", "--parents", "-n", "1", "HEAD"]).unwrap();
        assert_eq!(parents.split_whitespace().count(), 3, "integration must create a merge commit");
        assert!(!git_success(origin.to_str().unwrap(), &["show-ref", "--verify", "refs/codeterm/mesh/attempt_remote_test"]));
    }

    #[test]
    fn mesh_remote_lane_refuses_tip_different_from_reviewed_sha() {
        let (_root, origin, base, _lane_tip, bundle) = remote_bundle_fixture();
        let error = integrate_remote_lane_bundle(
            origin.to_str().unwrap(), &bundle, "lane/remote-test", "attempt_tip_mismatch",
            &base, "reviewed-sha-does-not-match", &current_branch(origin.to_str().unwrap()).unwrap(),
        ).unwrap_err();
        assert!(error.contains("lane tip differs"), "{error}");
    }

    #[test]
    fn mesh_remote_lane_refuses_dirty_origin_checkout() {
        let (_root, origin, base, lane_tip, bundle) = remote_bundle_fixture();
        std::fs::write(origin.join("untracked.txt"), "local data").unwrap();
        let error = integrate_remote_lane_bundle(
            origin.to_str().unwrap(), &bundle, "lane/remote-test", "attempt_dirty",
            &base, &lane_tip, &current_branch(origin.to_str().unwrap()).unwrap(),
        ).unwrap_err();
        assert!(error.contains("must be clean"), "{error}");
    }

    #[test]
    fn mesh_remote_lane_refuses_wrong_destination_branch() {
        let (_root, origin, base, lane_tip, bundle) = remote_bundle_fixture();
        let error = integrate_remote_lane_bundle(
            origin.to_str().unwrap(), &bundle, "lane/remote-test", "attempt_wrong_destination",
            &base, &lane_tip, "unrelated-branch",
        ).unwrap_err();
        assert!(error.contains("recorded destination branch"), "{error}");
    }

    #[test]
    fn mesh_remote_lane_conflict_aborts_merge_and_keeps_checkout_clean() {
        let (_root, origin, base, lane_tip, bundle) = remote_bundle_fixture();
        std::fs::write(origin.join("remote.txt"), "origin edit").unwrap();
        git(&origin, &["add", "remote.txt"]);
        git(&origin, &["commit", "-qm", "origin edit"]);
        let destination = current_branch(origin.to_str().unwrap()).unwrap();
        let error = integrate_remote_lane_bundle(
            origin.to_str().unwrap(), &bundle, "lane/remote-test", "attempt_conflict",
            &base, &lane_tip, &destination,
        ).unwrap_err();
        assert!(error.contains("real merge of peer lane failed"), "{error}");
        assert!(git_value(origin.to_str().unwrap(), ["status", "--porcelain"]).unwrap().is_empty());
        assert!(!git_success(origin.to_str().unwrap(), &["rev-parse", "--verify", "MERGE_HEAD"]));
        assert!(!git_success(origin.to_str().unwrap(), &["show-ref", "--verify", "refs/codeterm/mesh/attempt_conflict"]));
    }

    /// The provenance is persisted and later replayed as a git working directory,
    /// so a verbatim prefix here is what refuses a finished worker's report.
    #[test]
    fn lane_provenance_is_a_path_git_accepts_as_a_working_directory() {
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
        let provenance = verify_worker_lane(
            canonical_project.to_str().unwrap(),
            &lane.cwd,
            Some(&lane.branch),
            None,
            LaneProtection::Enforced,
        )
        .unwrap();

        for path in [&provenance.worktree_path, &provenance.canonical_cwd] {
            assert!(
                !path.starts_with(r"\\?\"),
                "lane provenance carries a verbatim prefix git cannot use: {path}"
            );
        }
        run_git(
            &provenance.worktree_path,
            ["rev-parse", "--is-inside-work-tree"],
        )
        .expect("the recorded worktree must validate where the lane's commits live");
        lane.cleanup().unwrap();
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
            std::fs::canonicalize(lane_path.parent().unwrap()).unwrap(),
            std::fs::canonicalize(root.path().join("project-worktrees")).unwrap()
        );
        assert!(lane.branch.starts_with("lane/team-test-"));
        assert!(!is_canonical_checkout(
            &lane.cwd,
            Some(project.to_str().unwrap())
        ));
        lane.cleanup().unwrap();
    }

    #[test]
    fn non_git_workspace_is_not_treated_as_a_canonical_checkout() {
        let workspace = tempfile::tempdir().unwrap();
        let workspace_cwd = workspace.path().to_str().unwrap();

        assert!(!is_canonical_checkout(workspace_cwd, Some(workspace_cwd)));
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
            LaneProtection::Enforced,
        )
        .unwrap_err();
        assert!(error.contains("branch mismatch"));
        lane.cleanup().unwrap();
    }

    /// The branch lookup once used `git branch --show-current`, which needs git
    /// 2.22. A GUI-launched daemon can resolve an older git, and the failure
    /// surfaced as "no repository checkout resolved" for every lane on the box.
    #[test]
    fn branch_lookup_reads_attached_and_detached_heads() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        git(&project, &["init", "-q"]);
        git(&project, &["config", "user.email", "test@example.com"]);
        git(&project, &["config", "user.name", "Test"]);
        std::fs::write(project.join("README.md"), "fixture").unwrap();
        git(&project, &["add", "README.md"]);
        git(&project, &["commit", "-qm", "fixture"]);

        let lane = provision_lane(project.to_str().unwrap(), "head-shape").unwrap();
        assert_eq!(current_branch(&lane.cwd).unwrap(), lane.branch);

        let head = git_value(&lane.cwd, ["rev-parse", "HEAD"]).unwrap();
        git(Path::new(&lane.cwd), &["checkout", "-q", "--detach", &head]);
        assert_eq!(current_branch(&lane.cwd).unwrap(), "");

        let error = verify_worker_lane(
            project.to_str().unwrap(),
            &lane.cwd,
            None,
            None,
            LaneProtection::Enforced,
        )
        .unwrap_err();
        assert!(error.contains("detached HEAD"), "{error}");
        verify_worker_lane(
            project.to_str().unwrap(),
            &lane.cwd,
            None,
            None,
            LaneProtection::Waived,
        )
        .expect("a waived lane may sit on a detached HEAD");
        lane.cleanup().unwrap();
    }

    /// An inspection that never reached a repository is not evidence of a
    /// detached HEAD, and reading it as an empty branch once turned a missing
    /// git into a detached-lane refusal on every lane of the machine.
    #[test]
    fn missing_git_or_non_repository_is_not_reported_as_detached_head() {
        let root = tempfile::tempdir().unwrap();
        let outside = root.path().join("not-a-repository");
        std::fs::create_dir_all(&outside).unwrap();
        // A garbage gitfile pins the discovery boundary here, so the lookup fails
        // on this directory instead of finding whatever repository encloses TMPDIR.
        std::fs::write(outside.join(".git"), "not a gitfile").unwrap();
        let outside = outside.to_str().unwrap();

        let error = current_branch(outside).unwrap_err();
        assert!(error.contains("failed to inspect worker lane"), "{error}");
        assert!(!error.contains("detached HEAD"), "{error}");

        // No binary to run is the missing-git shape: the spawn fails before any
        // exit status exists to classify as a detached HEAD.
        let unspawnable = root.path().join("absent").join("deeper");
        let error = current_branch(unspawnable.to_str().unwrap()).unwrap_err();
        assert!(error.contains("failed to inspect worker lane"), "{error}");

        let error =
            verify_worker_lane(outside, outside, None, None, LaneProtection::Enforced).unwrap_err();
        assert!(error.contains("failed to inspect worker lane"), "{error}");
        assert!(!error.contains("detached HEAD"), "{error}");

        let error =
            verify_worker_lane(outside, outside, None, None, LaneProtection::Waived).unwrap_err();
        assert!(error.contains("failed to inspect worker lane"), "{error}");
    }

    #[test]
    fn lane_provisioning_source_only_skips_dependency_link_and_preserves_canonical_modules() {
        let (_root, project) = source_only_fixture();
        let canonical_marker = project.join("node_modules/marker.txt");
        let canonical_before = std::fs::read_to_string(&canonical_marker).unwrap();
        let lane = provision_lane_for_test(project.to_str().unwrap(), "source-only").unwrap();
        let lane_modules = Path::new(&lane.cwd).join("node_modules");

        assert!(
            std::fs::symlink_metadata(&lane_modules).is_err(),
            "source-only provisioning must not create a dependency link"
        );
        assert_eq!(
            std::fs::read_to_string(canonical_marker).unwrap(),
            canonical_before
        );
        lane.cleanup().unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn lane_provisioning_windows_junction_resolution_is_test_fixture_only() {
        let (_root, project) = source_only_fixture();
        let canonical_marker = project.join("node_modules/marker.txt");
        let canonical_before = std::fs::read_to_string(&canonical_marker).unwrap();
        let lane = provision_lane_for_test(project.to_str().unwrap(), "winlink").unwrap();
        let lane_modules = Path::new(&lane.cwd).join("node_modules");
        let mut command = Command::new("cmd");
        process_util::no_window_std(&mut command);
        let output = command
            .args(["/C", "mklink", "/J"])
            .arg(&lane_modules)
            .arg(project.join("node_modules"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "junction creation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        assert_eq!(
            std::fs::read_to_string(lane_modules.join("marker.txt")).unwrap(),
            canonical_before
        );
        assert_eq!(
            std::fs::read_to_string(canonical_marker).unwrap(),
            canonical_before
        );
        std::fs::remove_dir(&lane_modules).unwrap();
        lane.cleanup().unwrap();
    }
}
