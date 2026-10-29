use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use crate::args::agent::ReportStatusArg;
use crate::client::RpcClient;
#[cfg(test)]
use codeterm_types::agent::ReportOutcomeClass;

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
        .call_detailed("plan.get", Some(serde_json::json!({ "plan_id": plan_id })))
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

const UNRESOLVED_LANE_REFUSAL_PREFIX: &str =
    "readiness report refused: cannot locate your lane worktree";
const UNAVAILABLE_SCOPE_REFUSAL_PREFIX: &str =
    "readiness report refused: task checkout binding is unavailable";
const UNUSABLE_ANCHOR_REFUSAL_PREFIX: &str = "readiness report refused: lane worktree";
const CANONICAL_DIRTY_REFUSAL_PREFIX: &str =
    "readiness report refused: this canonical checkout has uncommitted changes.";

/// Copied verbatim by an agent that has just lost a report, so every line must be
/// runnable: an inline body is mangled by the shell before the report is sent.
pub const REPORT_NEXT_COMMANDS: &[&str] = &[
    "git status --short",
    "git add <files>",
    "git commit -m \"<message>\"",
    "cat > \"$(domios pad path)/report.md\" <<'EOF'",
    "STATUS: in_review",
    "CHANGES: files touched or 'none'",
    "TESTS RUN: exact isolated commands with counts and raw exits, or 'none'",
    "TESTS COVERING CHANGES: covering-test commands with counts",
    "GATES REQUESTED: exact deferred selectors for canonical QA, or 'none'",
    "FOLLOWUP: Orchestrator reviews and closes the task",
    "EOF",
    "domios agent report --status in_review --sha YOUR_OWN_COMMIT_SHA --from-pad report.md",
    "# work in progress instead:",
    "cat > \"$(domios pad path)/report.md\" <<'EOF'",
    "STATUS: partial",
    "CHANGES: completed progress",
    "TESTS RUN: executed isolated checks, or 'none'",
    "GATES REQUESTED: none",
    "FOLLOWUP: exact remaining work",
    "EOF",
    "domios agent report --status partial --from-pad report.md",
];

impl ReportRefusal {
    #[cfg(test)]
    pub fn report_outcome(&self) -> Option<ReportOutcomeClass> {
        self.requires_binding_repair()
            .then_some(ReportOutcomeClass::DeliveredUnattached)
    }

    fn requires_binding_repair(&self) -> bool {
        [
            UNRESOLVED_LANE_REFUSAL_PREFIX,
            UNAVAILABLE_SCOPE_REFUSAL_PREFIX,
            UNUSABLE_ANCHOR_REFUSAL_PREFIX,
        ]
        .iter()
        .any(|prefix| self.reason.starts_with(prefix))
    }

    pub fn next_commands(&self) -> Vec<&'static str> {
        if self.requires_binding_repair() {
            vec!["domios agent report --status partial --from-pad report.md"]
        } else {
            REPORT_NEXT_COMMANDS.to_vec()
        }
    }

    pub fn message(&self) -> String {
        if self.requires_binding_repair() {
            return format!(
                "{}\nNext command for current progress (write the report body in your pad first):\n\
                 domios agent report --status partial --from-pad report.md",
                self.reason
            );
        }
        let mut lines = vec![self.reason.clone(), "Next commands:".to_string()];
        lines.extend(REPORT_NEXT_COMMANDS.iter().copied().map(str::to_string));
        lines.push(
            "Replace YOUR_OWN_COMMIT_SHA with your own owned commit SHA as a hexadecimal literal, never a shell expression, variable, or HEAD. Never use another Worker's newer HEAD."
                .to_string(),
        );
        if !self.dirty_files.is_empty() {
            lines.push(if self.reason.starts_with(CANONICAL_DIRTY_REFUSAL_PREFIX) {
                "Dirty canonical checkout files:".to_string()
            } else {
                "Dirty lane files:".to_string()
            });
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
    pub canonical_writer: bool,
    pub recorded_checkout: bool,
}

impl<'a> ReadinessScope<'a> {
    #[cfg(test)]
    fn pane(pane_cwd: &'a Path) -> Self {
        Self {
            pane_cwd,
            lane_worktree: None,
            canonical_writer: false,
            recorded_checkout: false,
        }
    }

    fn repo(&self) -> Result<&'a Path, ReportRefusal> {
        let Some(lane) = self.lane_worktree else {
            return Ok(self.pane_cwd);
        };
        let top_level = git_one(lane, &["rev-parse", "--show-toplevel"])
            .ok()
            .and_then(|root| std::fs::canonicalize(root).ok());
        let anchor = std::fs::canonicalize(lane).ok();
        let matches_anchor = match (anchor, top_level) {
            (Some(anchor), Some(root)) => codeterm_paths::paths_equal(
                &anchor.to_string_lossy(),
                &root.to_string_lossy(),
            ),
            _ => false,
        };
        if !matches_anchor {
            return Err(ReportRefusal {
                reason: format!(
                    "{UNUSABLE_ANCHOR_REFUSAL_PREFIX} '{}' is not a git worktree root on this host. \
                     Ask the Orchestrator to repair the recorded checkout or re-dispatch with a worktree; \
                     report partial progress meanwhile.",
                    lane.display()
                ),
                dirty_files: Vec::new(),
            });
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
    let canonical_writer = task_scope.is_some_and(|scope| scope.canonical_writer);
    let validation_worktree = task_scope.and_then(|scope| {
        if canonical_writer {
            scope.canonical_checkout_path.as_deref()
        } else {
            scope.worktree_path.as_deref()
        }
    });
    if task_scope.is_some()
        && is_readiness_status(status)
        && !canonical_writer
        && validation_worktree.is_none()
    {
        return Err(unresolved_lane_refusal());
    }
    if canonical_writer && is_readiness_status(status) && validation_worktree.is_none() {
        return Err(canonical_checkout_refusal());
    }
    let (base, acknowledge_dirty) = readiness_validation_options(task_scope, base, acknowledge_dirty);
    enrich_ready_report_in_scope(
        ReadinessScope {
            pane_cwd: Path::new("."),
            lane_worktree: validation_worktree,
            canonical_writer,
            recorded_checkout: task_scope.is_some_and(|scope| scope.recorded_checkout),
        },
        status,
        branch,
        sha,
        base,
        changed_files,
        acknowledge_dirty,
    )
}

fn readiness_validation_options(
    task_scope: Option<&crate::cmd::report_lane::TaskReadinessScope>,
    base: crate::cmd::report_attribution::ReportBase,
    acknowledge_dirty: bool,
) -> (Option<String>, bool) {
    if task_scope.is_some_and(|scope| scope.canonical_writer || scope.recorded_checkout) {
        (None, false)
    } else {
        (
            base.resolve(task_scope.and_then(|scope| scope.base.clone())),
            acknowledge_dirty,
        )
    }
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
        let resolved_given = resolve_commit(cwd, given);
        if report_sha_mismatches_head(resolved_given.as_deref(), head, scope.recorded_checkout) {
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
            return Err(readiness_dirty_error(dirty, scope.canonical_writer));
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

struct GitWorktreeInspection {
    metadata: ReportGitMetadata,
    base: Option<String>,
}

fn report_sha_mismatches_head(resolved_sha: Option<&str>, head: &str, recorded_checkout: bool) -> bool {
    if recorded_checkout {
        resolved_sha != Some(head)
    } else {
        lane_report_sha_is_stale(resolved_sha, Some(head))
    }
}

fn lane_report_sha_is_stale(resolved_sha: Option<&str>, lane_head: Option<&str>) -> bool {
    matches!((resolved_sha, lane_head), (Some(sha), Some(head)) if sha != head)
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
    Ok(Some(GitWorktreeInspection {
        metadata: ReportGitMetadata {
            branch,
            sha: Some(sha),
            changed_files: Some(changed),
        },
        base,
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

fn git_refusal(reason: String) -> ReportRefusal {
    ReportRefusal {
        reason: format!("readiness report refused: {reason}"),
        dirty_files: Vec::new(),
    }
}

pub fn unavailable_scope_refusal(detail: &str) -> ReportRefusal {
    ReportRefusal {
        reason: format!(
            "{UNAVAILABLE_SCOPE_REFUSAL_PREFIX}: {detail}. Ask the Orchestrator to repair the task binding \
             or re-dispatch; report partial progress meanwhile. No caller checkout was inspected."
        ),
        dirty_files: Vec::new(),
    }
}

fn unresolved_lane_refusal() -> ReportRefusal {
    ReportRefusal {
        reason: format!(
            "{UNRESOLVED_LANE_REFUSAL_PREFIX}. This task requires a \
                 repository but neither a lane worktree nor a launch checkout is recorded for your report, \
                 so --sha cannot be validated against your own HEAD. Ask the Orchestrator to \
                 re-dispatch the task with a worktree. Use a partial report for current progress."
        ),
        dirty_files: Vec::new(),
    }
}

fn canonical_checkout_refusal() -> ReportRefusal {
    ReportRefusal {
        reason: "readiness report refused: cannot locate the plan's canonical checkout for this canonical-writer attempt."
            .to_string(),
        dirty_files: Vec::new(),
    }
}

fn readiness_dirty_error(dirty_files: Vec<String>, canonical_writer: bool) -> ReportRefusal {
    ReportRefusal {
        reason: if canonical_writer {
            CANONICAL_DIRTY_REFUSAL_PREFIX.to_string()
        } else {
            "readiness report refused: this git worktree has uncommitted lane changes.".to_string()
        },
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
    path == ".domios/plans" || path.starts_with(".domios/plans/")
}

fn git_command(cwd: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(cwd);
    sanitize_git_environment(&mut command);
    command
}

fn sanitize_git_environment(command: &mut Command) {
    let keys: BTreeSet<_> = std::env::vars_os()
        .map(|(key, _)| key)
        .chain(command.get_envs().map(|(key, _)| key.to_os_string()))
        .filter(|key| {
            key.to_string_lossy()
                .to_ascii_uppercase()
                .starts_with("GIT_")
        })
        .collect();
    for key in keys {
        command.env_remove(key);
    }
}

fn git_success(cwd: &Path, args: &[&str]) -> bool {
    git_command(cwd)
        .args(args)
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
    let out = git_command(cwd)
        .args(args)
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
