use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::args::agent::ReportStatusArg;
use crate::client::RpcClient;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportGitMetadata {
    pub branch: Option<String>,
    pub sha: Option<String>,
    pub changed_files: Option<Vec<String>>,
}

/// Asserted by name, not wording — codeterm#305.
pub const UNBOUND_WORKER_BINDING_ADVISORY: &str = "UNBOUND WORKER BINDING";

/// Shared by `agent spawn --role Worker` and `agent report`, so the two
/// entry points cannot drift apart — codeterm#305.
pub fn worker_binding_resolved(has_plan_task: bool, has_plan: bool, has_task: bool) -> bool {
    has_plan_task || (has_plan && has_task)
}

/// A Planner holds no task by design, so it is exempt from the task half of
/// `worker_binding_resolved` — codeterm#371. Resolved by querying the plan's
/// own recorded Planner tab (durable, server-side, the same field
/// `plan_doctor`'s `planner_unpaired` check reads) rather than sniffing the
/// `CODETERM_ROLE` env var — that var is injected once at PTY launch and can
/// be stale or absent for a tab whose Planner role was attached afterward,
/// which let a live-bound Planner's report be refused anyway — codeterm#385.
pub fn report_task_binding_resolved(
    client: &mut dyn RpcClient,
    plan_id: Option<&str>,
    task_id: Option<&str>,
    caller_tab_id: &str,
) -> bool {
    task_id.is_some() || caller_is_bound_plan_planner(client, plan_id, caller_tab_id)
}

fn caller_is_bound_plan_planner(
    client: &mut dyn RpcClient,
    plan_id: Option<&str>,
    caller_tab_id: &str,
) -> bool {
    let Some(plan_id) = plan_id else {
        return false;
    };
    client
        .call_detailed(
            "plan.get",
            Some(serde_json::json!({ "plan_id": plan_id })),
        )
        .ok()
        .and_then(|plan| plan_planner_tab_id(&plan).map(str::to_string))
        .is_some_and(|bound| bound == caller_tab_id)
}

/// Mirrors `plan_doctor`'s own `plan.get("planner")` read — the plan's bound
/// Planner tab, or `None` when unset/waived.
fn plan_planner_tab_id(plan: &serde_json::Value) -> Option<&str> {
    plan.get("planner")
        .filter(|value| !value.is_null())
        .and_then(|value| value.get("tab_id"))
        .and_then(serde_json::Value::as_str)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportRefusal {
    pub reason: String,
    pub dirty_files: Vec<String>,
}

/// Copied verbatim by an agent that has just lost a report, so every line must be
/// runnable: an inline body is mangled by the shell before the report is sent.
pub const REPORT_NEXT_COMMANDS: &[&str] = &[
    "git status --short",
    "git add <files>",
    "git commit -m \"<message>\"",
    "cat > \"$(codeterm pad path)/report.md\" <<'EOF'",
    "STATUS: in_review",
    "CHANGES: files touched or 'none'",
    "TESTS RUN: 'deferred to Orchestrator canonical QA' unless explicitly authorized in canonical",
    "GATES REQUESTED: exact QA/test-runner commands needed, or 'none'",
    "FOLLOWUP: Orchestrator reviews and closes the task",
    "EOF",
    "codeterm agent report --status in_review --sha YOUR_OWN_COMMIT_SHA --from-pad report.md",
    "# work in progress instead:",
    "cat > \"$(codeterm pad path)/report.md\" <<'EOF'",
    "STATUS: partial",
    "CHANGES: completed progress",
    "TESTS RUN: narrow evidence, or 'none - isolated lane, no gate executed'",
    "GATES REQUESTED: none",
    "FOLLOWUP: exact remaining work",
    "EOF",
    "codeterm agent report --status partial --from-pad report.md",
];

impl ReportRefusal {
    pub fn message(&self) -> String {
        let mut lines = vec![self.reason.clone(), "Next commands:".to_string()];
        lines.extend(REPORT_NEXT_COMMANDS.iter().copied().map(str::to_string));
        lines.push(
            "Replace YOUR_OWN_COMMIT_SHA with your own owned commit SHA as a hexadecimal literal, never a shell expression, variable, or HEAD. Never use another Worker's newer HEAD."
                .to_string(),
        );
        if !self.dirty_files.is_empty() {
            lines.push("Dirty lane files:".to_string());
            lines.extend(self.dirty_files.iter().map(|file| format!("  {file}")));
        }
        lines.join("\n")
    }
}

/// Which checkout a readiness report is validated against: the pane the CLI was
/// invoked from, or the worktree the lane actually owns.
pub struct ReadinessScope<'a> {
    pub pane_cwd: &'a Path,
    pub lane_worktree: Option<&'a Path>,
}

impl<'a> ReadinessScope<'a> {
    #[cfg(test)]
    fn pane(pane_cwd: &'a Path) -> Self {
        Self {
            pane_cwd,
            lane_worktree: None,
        }
    }

    fn repo(&self) -> Result<&'a Path, ReportRefusal> {
        let Some(lane) = self.lane_worktree else {
            return Ok(self.pane_cwd);
        };
        if !git_success(lane, &["rev-parse", "--is-inside-work-tree"]) {
            return Err(git_refusal(format!(
                "lane worktree '{}' is not a git worktree on this host, so --sha/--base cannot be \
                 validated where the lane's commits live.",
                lane.display()
            )));
        }
        Ok(lane)
    }
}

#[allow(clippy::type_complexity)]
pub fn enrich_ready_report(
    task_scope: Option<&crate::cmd::report_lane::TaskReadinessScope>,
    status: Option<ReportStatusArg>,
    branch: Option<String>,
    sha: Option<String>,
    base: crate::cmd::report_attribution::ReportBase,
    changed_files: Option<Vec<String>>,
    acknowledge_dirty: bool,
) -> Result<(Option<String>, Option<String>, Option<Vec<String>>), ReportRefusal> {
    if task_scope.is_some_and(|scope| !scope.requires_repository) {
        return Ok((branch, sha, changed_files));
    }
    let lane_worktree = task_scope.and_then(|scope| scope.worktree_path.as_deref());
    // A readiness report that names a commit but whose repository task bound no lane
    // worktree must never be silently validated against whatever checkout the CLI
    // process happens to run in — that path compares a lane commit to canonical HEAD.
    // Refuse loudly so the worker rebinds its lane instead.
    if task_scope.is_some()
        && is_readiness_status(status)
        && lane_worktree.is_none()
        && sha
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
    {
        return Err(unresolved_lane_refusal());
    }
    // The live attempt base is what this tab is working from now; the dispatch
    // environment is a snapshot of the attempt that launched the shell and does
    // not move with a re-dispatch. Attribution follows the live interval.
    let base = base.resolve(task_scope.and_then(|scope| scope.base.clone()));
    enrich_ready_report_in_scope(
        ReadinessScope {
            pane_cwd: Path::new("."),
            lane_worktree,
        },
        status,
        branch,
        sha,
        base,
        changed_files,
        acknowledge_dirty,
    )
}

#[cfg(test)]
#[allow(clippy::type_complexity)]
fn enrich_ready_report_in(
    cwd: &Path,
    status: Option<ReportStatusArg>,
    branch: Option<String>,
    sha: Option<String>,
    base: Option<String>,
    changed_files: Option<Vec<String>>,
    acknowledge_dirty: bool,
) -> Result<(Option<String>, Option<String>, Option<Vec<String>>), ReportRefusal> {
    enrich_ready_report_in_scope(
        ReadinessScope::pane(cwd),
        status,
        branch,
        sha,
        base,
        changed_files,
        acknowledge_dirty,
    )
}

#[allow(clippy::type_complexity)]
fn enrich_ready_report_in_scope(
    scope: ReadinessScope<'_>,
    status: Option<ReportStatusArg>,
    branch: Option<String>,
    sha: Option<String>,
    base: Option<String>,
    changed_files: Option<Vec<String>>,
    acknowledge_dirty: bool,
) -> Result<(Option<String>, Option<String>, Option<Vec<String>>), ReportRefusal> {
    if !is_readiness_status(status) {
        return Ok((branch, sha, changed_files));
    }
    let cwd = scope.repo()?;
    let Some(inspection) = inspect_git_worktree_with_base(cwd, base.as_deref())? else {
        return Ok((branch, sha, changed_files));
    };
    let metadata = inspection.metadata;
    if let (Some(given), Some(head)) = (sha.as_deref(), metadata.sha.as_deref()) {
        if !sha_matches_readiness(
            cwd,
            given,
            head,
            inspection.base.as_deref(),
            inspection.checkout,
        ) {
            return Err(ReportRefusal {
                reason: format!(
                    "readiness report refused: provided --sha {given} does not match HEAD {head}."
                ),
                dirty_files: Vec::new(),
            });
        }
    }
    if let (Some(ref given), Some(ref head)) = (branch.as_deref(), metadata.branch.as_deref()) {
        if given != head {
            return Err(ReportRefusal {
                reason: format!(
                    "readiness report refused: provided --branch '{given}' does not match current branch '{head}'."
                ),
                dirty_files: Vec::new(),
            });
        }
    }
    if !acknowledge_dirty {
        let owned_sha = sha.as_deref().and_then(|given| resolve_commit(cwd, given));
        if let Some(dirty) = load_lane_dirty(cwd, inspection.base.as_deref(), owned_sha.as_deref())?
        {
            return Err(readiness_dirty_error(dirty));
        }
    }
    Ok((
        branch.or(metadata.branch),
        sha.or(metadata.sha),
        changed_files.or(metadata.changed_files),
    ))
}

fn load_lane_dirty(
    cwd: &Path,
    base: Option<&str>,
    owned_sha: Option<&str>,
) -> Result<Option<Vec<String>>, ReportRefusal> {
    let dirty = git_lines(cwd, &["status", "--porcelain", "-uall"]).map_err(git_refusal)?;
    let mut lane_dirty = lane_dirty_files(&dirty);
    if let (Some(base), Some(owned_sha)) = (base, owned_sha) {
        let owned_paths: BTreeSet<String> =
            git_lines(cwd, &["diff", "--name-only", base, owned_sha])
                .map_err(git_refusal)?
                .into_iter()
                .collect();
        lane_dirty.retain(|path| owned_paths.contains(path));
    }
    if lane_dirty.is_empty() {
        Ok(None)
    } else {
        Ok(Some(lane_dirty))
    }
}

pub fn is_readiness_status(status: Option<ReportStatusArg>) -> bool {
    matches!(
        status,
        Some(ReportStatusArg::InReview | ReportStatusArg::Done)
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CheckoutKind {
    SharedCanonical,
    IsolatedWorktree,
}

struct GitWorktreeInspection {
    metadata: ReportGitMetadata,
    base: Option<String>,
    checkout: CheckoutKind,
}

fn sha_matches_readiness(
    cwd: &Path,
    given: &str,
    head: &str,
    base: Option<&str>,
    checkout: CheckoutKind,
) -> bool {
    let Some(given) = resolve_commit(cwd, given) else {
        return false;
    };
    if checkout == CheckoutKind::IsolatedWorktree || base.is_none() {
        return given == head;
    }
    let base = base.unwrap_or_default();
    given != base
        && git_success(cwd, &["merge-base", "--is-ancestor", base, &given])
        && git_success(cwd, &["merge-base", "--is-ancestor", &given, head])
}

#[cfg(test)]
fn inspect_git_worktree(
    cwd: &Path,
    base: Option<&str>,
) -> Result<Option<ReportGitMetadata>, ReportRefusal> {
    Ok(inspect_git_worktree_with_base(cwd, base)?.map(|inspection| inspection.metadata))
}

fn inspect_git_worktree_with_base(
    cwd: &Path,
    base: Option<&str>,
) -> Result<Option<GitWorktreeInspection>, ReportRefusal> {
    if !git_success(cwd, &["rev-parse", "--is-inside-work-tree"]) {
        return Ok(None);
    }
    let base = resolve_base_commit(cwd, base).map_err(git_refusal)?;
    let sha = git_one(cwd, &["rev-parse", "HEAD"]).map_err(git_refusal)?;
    let branch = git_one(cwd, &["branch", "--show-current"]).ok();
    let changed = load_changed_files(cwd, base.as_deref()).map_err(git_refusal)?;
    let checkout = checkout_kind(cwd).map_err(git_refusal)?;
    Ok(Some(GitWorktreeInspection {
        metadata: ReportGitMetadata {
            branch,
            sha: Some(sha),
            changed_files: Some(changed),
        },
        base,
        checkout,
    }))
}

fn load_changed_files(cwd: &Path, base: Option<&str>) -> Result<Vec<String>, String> {
    let committed = match base {
        Some(base) => git_lines(cwd, &["diff", "--name-only", &base, "HEAD"])?,
        None => Vec::new(),
    };
    let overlay = lane_dirty_files(&git_lines(cwd, &["status", "--porcelain", "-uall"])?);
    let changed: BTreeSet<String> = committed.into_iter().chain(overlay).collect();
    Ok(changed.into_iter().collect())
}

fn resolve_base_commit(cwd: &Path, base: Option<&str>) -> Result<Option<String>, String> {
    let Some(base) = base.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if base.starts_with('-') {
        return Err("invalid report base: refs starting with '-' are rejected".into());
    }
    git_one(
        cwd,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{base}^{{commit}}"),
        ],
    )
    .map(Some)
    .map_err(|_| format!("invalid report base: '{base}' is not a commit in this worktree"))
}

fn resolve_commit(cwd: &Path, value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.starts_with('-') {
        return None;
    }
    git_one(
        cwd,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{value}^{{commit}}"),
        ],
    )
    .ok()
}

fn checkout_kind(cwd: &Path) -> Result<CheckoutKind, String> {
    let git_dir = git_recorded_path(cwd, &git_one(cwd, &["rev-parse", "--git-dir"])?)?;
    let common_dir = git_recorded_path(cwd, &git_one(cwd, &["rev-parse", "--git-common-dir"])?)?;
    Ok(if git_dir == common_dir {
        CheckoutKind::SharedCanonical
    } else {
        CheckoutKind::IsolatedWorktree
    })
}

fn git_recorded_path(cwd: &Path, value: &str) -> Result<PathBuf, String> {
    let path = Path::new(value);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    std::fs::canonicalize(&absolute).map_err(|error| {
        format!(
            "cannot resolve git-recorded path '{}': {error}",
            absolute.display()
        )
    })
}

fn git_refusal(reason: String) -> ReportRefusal {
    ReportRefusal {
        reason: format!("readiness report refused: {reason}"),
        dirty_files: Vec::new(),
    }
}

fn unresolved_lane_refusal() -> ReportRefusal {
    ReportRefusal {
        reason: "readiness report refused: cannot locate your lane worktree. This task requires a \
                 repository but no lane worktree is bound to your report, so the provided --sha \
                 cannot be validated against your own lane HEAD (never against the canonical \
                 checkout). Re-dispatch this task with `codeterm plan task dispatch`, or report \
                 with explicit `--plan <plan> --task <task>` so your lane is resolved."
            .to_string(),
        dirty_files: Vec::new(),
    }
}

fn readiness_dirty_error(dirty_files: Vec<String>) -> ReportRefusal {
    ReportRefusal {
        reason: "readiness report refused: this git worktree has uncommitted lane changes."
            .to_string(),
        dirty_files: dirty_files.into_iter().take(8).collect(),
    }
}

fn lane_dirty_files(status_lines: &[String]) -> Vec<String> {
    status_lines
        .iter()
        .flat_map(|line| porcelain_paths(line))
        .filter(|path| !is_tool_owned_path(path))
        .collect()
}

fn porcelain_paths(line: &str) -> Vec<String> {
    let Some(rest) = line.get(3..) else {
        return Vec::new();
    };
    rest.split(" -> ")
        .map(unquote_git_path)
        .filter(|path| !path.is_empty())
        .collect()
}

fn unquote_git_path(path: &str) -> String {
    path.trim().trim_matches('"').to_string()
}

fn is_tool_owned_path(path: &str) -> bool {
    path == ".codeterm/plans" || path.starts_with(".codeterm/plans/")
}

fn git_success(cwd: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn git_one(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let lines = git_lines(cwd, args)?;
    lines
        .into_iter()
        .next()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("git {} returned no value", args.join(" ")))
}

fn git_lines(cwd: &Path, args: &[&str]) -> Result<Vec<String>, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("git {} failed to start: {}", args.join(" "), e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("git {} failed: {}", args.join(" "), stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect())
}

#[cfg(test)]
mod tests;
