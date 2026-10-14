use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

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

pub(crate) fn primary_checkout(cwd: &Path) -> Option<PathBuf> {
    let (_, common_dir) = git_checkout_paths(cwd)?;
    let primary = normalize_path(common_dir.parent()?.to_str()?);
    if !git_is_primary_checkout(&primary) {
        return None;
    }
    let top_level =
        normalize_path(&git_value(primary.to_str()?, ["rev-parse", "--show-toplevel"]).ok()?);
    (top_level == primary).then_some(primary)
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
        let worktree =
            super::lane_removal::remove_lane_worktree(&self.canonical_cwd, &self.cwd, true);
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

pub(crate) fn legacy_repository_identity(cwd: &str) -> Result<String, String> {
    let remotes = git_value(cwd, ["remote"])?;
    let mut identities = Vec::new();
    for remote in remotes
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        let urls = git_value(cwd, ["remote", "get-url", "--all", remote])?;
        identities.extend(urls.lines().filter_map(sanitized_legacy_remote_identity));
    }
    identities.sort();
    identities.dedup();
    if identities.is_empty() {
        return Err("checkout has no configured repository remote identity".into());
    }
    use sha2::{Digest, Sha256};
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(identities.join("\n").as_bytes())
    ))
}

pub(crate) fn repository_identities(cwd: &str) -> Result<Vec<String>, String> {
    let remotes = git_value(cwd, ["remote"])?;
    let mut identities = Vec::new();
    for remote in remotes
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        let Ok(urls) = git_value(cwd, ["remote", "get-url", "--all", remote]) else {
            continue;
        };
        identities.extend(urls.lines().filter_map(sanitized_remote_identity));
    }
    identities.sort();
    identities.dedup();
    if identities.is_empty() {
        return Err("checkout has no configured repository remote identity".into());
    }
    Ok(identities)
}

pub(crate) fn repository_matches(
    cwd: &str,
    expected_identities: &[String],
    legacy_identity: &str,
) -> bool {
    matching_remote_for_identity(cwd, expected_identities, legacy_identity).is_some()
}

pub(crate) fn matching_remote_for_identity(
    cwd: &str,
    expected_identities: &[String],
    legacy_identity: &str,
) -> Option<String> {
    if !expected_identities.is_empty() {
        return matching_remote(cwd, expected_identities);
    }
    if !legacy_repository_identity(cwd).is_ok_and(|identity| identity == legacy_identity) {
        return None;
    }
    git_value(cwd, ["remote"])
        .ok()?
        .lines()
        .map(str::trim)
        .find(|name| !name.is_empty())
        .map(str::to_string)
}

pub(crate) fn matching_remote(cwd: &str, expected_identities: &[String]) -> Option<String> {
    let remotes = git_value(cwd, ["remote"]).ok()?;
    let mut remotes = remotes
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    remotes.sort_unstable();
    for remote in remotes {
        let Ok(urls) = git_value(cwd, ["remote", "get-url", "--all", remote]) else {
            continue;
        };
        let matches = urls
            .lines()
            .filter_map(sanitized_remote_identity)
            .any(|identity| {
                expected_identities
                    .iter()
                    .any(|expected| expected == &identity)
            });
        if matches {
            return Some(remote.to_string());
        }
    }
    None
}

pub(crate) fn fetch_base_commit(cwd: &str, remote: &str, commit: &str) -> Result<(), String> {
    const DEADLINE: Duration = Duration::from_secs(30);
    if remote.is_empty() || remote.starts_with('-') {
        return Err("matched Git remote name is invalid".into());
    }
    if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("requested base is not a full commit id".into());
    }
    let mut command = Command::new("git");
    process_util::no_window_std(&mut command);
    let mut child = command
        .current_dir(cwd)
        .args(["fetch", "--quiet", "--no-tags", remote, commit])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "git fetch could not start".to_string())?;
    let status = match child.wait_timeout(DEADLINE) {
        Ok(status) => status,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("git fetch could not be observed".into());
        }
    };
    match status {
        Some(status) if status.success() => Ok(()),
        Some(status) => Err(format!("git fetch exited with status {status}")),
        None => {
            let _ = child.kill();
            let _ = child.wait();
            Err("git fetch exceeded the 30 second deadline".into())
        }
    }
}

pub(crate) fn contains_commit(cwd: &str, commit: &str) -> bool {
    !commit.trim().is_empty()
        && git_value(
            cwd,
            ["cat-file", "-e", &format!("{}^{{commit}}", commit.trim())],
        )
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
    let dirty = git_value(
        canonical_cwd,
        ["status", "--porcelain", "--untracked-files=all"],
    )?;
    if !dirty.trim().is_empty() {
        return Err("origin checkout must be clean before remote lane integration".into());
    }
    let temp_dir = tempfile::tempdir()
        .map_err(|error| format!("cannot allocate remote lane bundle: {error}"))?;
    let bundle_path = temp_dir.path().join("remote-lane.bundle");
    std::fs::write(&bundle_path, bundle)
        .map_err(|error| format!("cannot write remote lane bundle: {error}"))?;
    run_git(
        canonical_cwd,
        ["bundle", "verify", bundle_path.to_string_lossy().as_ref()],
    )?;
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
    if run_git(
        canonical_cwd,
        ["merge-base", "--is-ancestor", base, &lane_tip],
    )
    .is_err()
    {
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
        || !git_value(
            canonical_cwd,
            ["status", "--porcelain", "--untracked-files=all"],
        )?
        .is_empty()
    {
        return Err("origin checkout changed before remote lane merge".into());
    }
    if let Err(error) = run_git(
        canonical_cwd,
        ["merge", "--no-ff", "--no-edit", &target_ref],
    ) {
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
        if !matches!(
            scheme.to_ascii_lowercase().as_str(),
            "ssh" | "https" | "git"
        ) {
            return None;
        }
        let authority_end = rest.find('/').unwrap_or(rest.len());
        let host = normalized_host(&rest[..authority_end])?;
        let path = normalized_repository_path(&rest[authority_end..])?;
        return Some(format!("{host}/{path}"));
    }
    let (user, host_path) = remote.split_once('@')?;
    if user.trim().is_empty() {
        return None;
    }
    let (host, path) = host_path.split_once(':')?;
    let host = normalized_host(host)?;
    Some(format!("{host}/{}", normalized_repository_path(path)?))
}

fn normalized_host(authority: &str) -> Option<String> {
    let host_port = authority.rsplit('@').next()?.trim();
    if host_port.is_empty() {
        return None;
    }
    let host = if let Some(bracketed) = host_port.strip_prefix('[') {
        let end = bracketed.find(']')?;
        let host = &bracketed[..end];
        let suffix = &bracketed[end + 1..];
        if !suffix.is_empty()
            && !suffix
                .strip_prefix(':')
                .is_some_and(|port| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()))
        {
            return None;
        }
        format!("[{}]", host.to_ascii_lowercase())
    } else if let Some((host, port)) = host_port.split_once(':') {
        if host.is_empty() || port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        host.to_ascii_lowercase()
    } else {
        host_port.to_ascii_lowercase()
    };
    (!host
        .bytes()
        .any(|byte| byte.is_ascii_whitespace() || byte == b'/'))
    .then_some(host)
}

fn normalized_repository_path(path: &str) -> Option<String> {
    let path = path.split(['?', '#']).next()?.trim_matches('/');
    let (owner, repo) = path.split_once('/')?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') || owner == "." || owner == ".." {
        return None;
    }
    let repo = if repo.to_ascii_lowercase().ends_with(".git") {
        &repo[..repo.len() - 4]
    } else {
        repo
    };
    if repo.is_empty() || repo == "." || repo == ".." {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

fn sanitized_legacy_remote_identity(remote: &str) -> Option<String> {
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
        [
            "rev-parse",
            "--verify",
            &format!("{}^{{commit}}", requested_base),
        ],
    )?;
    if let Err(error) = run_git(
        &project_cwd,
        ["worktree", "add", "-b", &branch, &lane_cwd, &base],
    ) {
        let _ = super::lane_removal::remove_lane_residue(&lane_path);
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
    let (_, common_dir) = git_checkout_paths(&worktree)
        .ok_or_else(|| "worker lane repository identity is unavailable".to_string())?;
    let repository = common_dir
        .parent()
        .and_then(Path::to_str)
        .ok_or_else(|| "worker lane repository has no primary checkout path".to_string())?;
    let repository = canonical_path(repository)?;
    if !git_is_primary_checkout(&repository) {
        return Err("worker lane repository has no verified primary checkout".into());
    }
    if !protection.is_waived() && worktree == repository && canonical != repository {
        return Err("worker lane is the owning repository's primary checkout".into());
    }
    let repository_cwd = repository
        .to_str()
        .ok_or_else(|| "worker lane repository path is not UTF-8".to_string())?;
    let head_ref = git_value(worktree_path, ["rev-parse", "--verify", "HEAD^{commit}"])?;
    let base = match expected_base
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(expected) => {
            let commit_ref = format!("{expected}^{{commit}}");
            let resolved = git_value(repository_cwd, ["rev-parse", "--verify", &commit_ref])
                .map_err(|error| format!("worker lane base '{expected}' does not resolve in owning repository '{repository_cwd}': {error}"))?;
            run_git(
                repository_cwd,
                ["merge-base", "--is-ancestor", &resolved, &head_ref],
            )
            .map_err(|_| {
                format!("worker lane base '{expected}' is not an ancestor of '{head_ref}'")
            })?;
            resolved
        }
        None => git_value(repository_cwd, ["merge-base", "HEAD", &head_ref])?,
    };
    let destination = match git_value(repository_cwd, ["symbolic-ref", "--short", "-q", "HEAD"]) {
        Ok(name) if !name.is_empty() => Some(name),
        Ok(_) => None,
        Err(error) => {
            log::warn!(
                "[worker-lane] no destination branch recorded for {repository_cwd}: {error}"
            );
            None
        }
    };
    Ok(WorkerLaneProvenance {
        canonical_cwd: repository.display().to_string(),
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
    command
        .current_dir(project_cwd)
        .args(args)
        .status()
        .is_ok_and(|status| status.success())
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
        .env("GIT_OPTIONAL_LOCKS", "0")
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
mod tests;
