use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::args::agent::ReportStatusArg;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportGitMetadata {
    pub branch: Option<String>,
    pub sha: Option<String>,
    pub changed_files: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportRefusal {
    pub reason: String,
    pub dirty_files: Vec<String>,
}

impl ReportRefusal {
    pub fn message(&self) -> String {
        let mut lines = vec![
            self.reason.clone(),
            "Next commands:".to_string(),
            "  git status --short".to_string(),
            "  git add <files>".to_string(),
            "  git commit -m \"<message>\"".to_string(),
            "  codeterm agent report --status in_review --sha YOUR_OWN_COMMIT_SHA \"...\""
                .to_string(),
            "Replace YOUR_OWN_COMMIT_SHA with your own owned commit SHA as a hexadecimal literal, never a shell expression, variable, or HEAD. Never use another Worker's newer HEAD."
                .to_string(),
            "Or report WIP with:".to_string(),
            "  codeterm agent report --status partial \"...\"".to_string(),
        ];
        if !self.dirty_files.is_empty() {
            lines.push("Dirty lane files:".to_string());
            lines.extend(self.dirty_files.iter().map(|file| format!("  {file}")));
        }
        lines.join("\n")
    }
}

#[allow(clippy::type_complexity)]
pub fn enrich_ready_report(
    status: Option<ReportStatusArg>,
    branch: Option<String>,
    sha: Option<String>,
    base: Option<String>,
    changed_files: Option<Vec<String>>,
    acknowledge_dirty: bool,
) -> Result<(Option<String>, Option<String>, Option<Vec<String>>), ReportRefusal> {
    enrich_ready_report_in(
        Path::new("."),
        status,
        branch,
        sha,
        base,
        changed_files,
        acknowledge_dirty,
    )
}

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
    if !is_readiness_status(status) {
        return Ok((branch, sha, changed_files));
    }
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
        if let Some(dirty) = load_lane_dirty(cwd) {
            return Err(readiness_dirty_error(dirty));
        }
    }
    Ok((
        branch.or(metadata.branch),
        sha.or(metadata.sha),
        changed_files.or(metadata.changed_files),
    ))
}

fn load_lane_dirty(cwd: &Path) -> Option<Vec<String>> {
    let dirty = git_lines(cwd, &["status", "--porcelain", "-uall"]).ok()?;
    let lane_dirty = lane_dirty_files(&dirty);
    if lane_dirty.is_empty() {
        None
    } else {
        Some(lane_dirty)
    }
}

fn is_readiness_status(status: Option<ReportStatusArg>) -> bool {
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
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn run(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn repo() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        run(dir.path(), &["init", "--initial-branch", "main"]);
        run(
            dir.path(),
            &["config", "user.email", "test@example.invalid"],
        );
        run(dir.path(), &["config", "user.name", "Test User"]);
        fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        run(dir.path(), &["add", "a.txt"]);
        run(dir.path(), &["commit", "-m", "initial"]);
        dir
    }

    fn repo_with_linked_worktree() -> (TempDir, PathBuf, PathBuf) {
        let dir = repo();
        let main = dir.path().to_path_buf();
        let linked = dir.path().join("linked");
        let linked_str = linked.to_string_lossy().into_owned();
        run(
            dir.path(),
            &["worktree", "add", "-b", "lane/report", &linked_str],
        );
        (dir, main, linked)
    }

    fn branch_head(dir: &Path) -> String {
        git_one(dir, &["rev-parse", "HEAD"]).unwrap()
    }

    #[test]
    fn dirty_readiness_report_is_refused() {
        let dir = repo();
        fs::write(dir.path().join("a.txt"), "dirty\n").unwrap();

        let err = enrich_ready_report_in(
            dir.path(),
            Some(ReportStatusArg::InReview),
            None,
            None,
            None,
            None,
            false,
        )
        .unwrap_err();

        assert!(err.message().contains("readiness report refused"));
        assert!(err.message().contains("--status partial"));
        assert_eq!(err.dirty_files, vec!["a.txt"]);
    }

    #[test]
    fn acknowledge_dirty_skips_dirty_check() {
        let dir = repo();
        fs::write(dir.path().join("a.txt"), "dirty\n").unwrap();

        let (branch, sha, changed) = enrich_ready_report_in(
            dir.path(),
            Some(ReportStatusArg::InReview),
            None,
            None,
            None,
            None,
            true,
        )
        .unwrap();

        assert!(sha.unwrap().len() >= 7);
        assert!(changed.unwrap().contains(&"a.txt".to_string()));
        assert_eq!(branch.as_deref(), Some("main"));
    }

    #[test]
    fn tool_owned_plan_mirror_dirt_is_ignored() {
        let dir = repo();
        let plans_dir = dir.path().join(".codeterm/plans");
        fs::create_dir_all(&plans_dir).unwrap();
        fs::write(plans_dir.join("mirror.md"), "tool state\n").unwrap();

        let (_branch, sha, _changed) = enrich_ready_report_in(
            dir.path(),
            Some(ReportStatusArg::InReview),
            None,
            None,
            None,
            None,
            false,
        )
        .unwrap();

        assert!(sha.unwrap().len() >= 7);
    }

    #[test]
    fn mixed_tool_and_lane_dirt_refuses_only_lane_files() {
        let dir = repo();
        let plans_dir = dir.path().join(".codeterm/plans");
        fs::create_dir_all(&plans_dir).unwrap();
        fs::write(plans_dir.join("mirror.md"), "tool state\n").unwrap();
        fs::write(dir.path().join("src.rs"), "lane dirt\n").unwrap();

        let err = enrich_ready_report_in(
            dir.path(),
            Some(ReportStatusArg::InReview),
            None,
            None,
            None,
            None,
            false,
        )
        .unwrap_err();

        assert_eq!(err.dirty_files, vec!["src.rs"]);
        assert!(!err.message().contains(".codeterm/plans"));
    }

    #[test]
    fn clean_repo_returns_head_metadata() {
        let dir = repo();

        let metadata = inspect_git_worktree(dir.path(), Some("HEAD"))
            .unwrap()
            .unwrap();

        assert!(metadata.sha.unwrap().len() >= 7);
        assert!(metadata.changed_files.unwrap().is_empty());
    }

    #[test]
    fn modified_file_reports_exactly_one_changed_path() {
        let dir = repo();
        fs::write(dir.path().join("a.txt"), "dirty\n").unwrap();

        let metadata = inspect_git_worktree(dir.path(), Some("HEAD"))
            .unwrap()
            .unwrap();

        assert_eq!(metadata.changed_files.unwrap(), vec!["a.txt"]);
    }

    #[test]
    fn untracked_file_reports_exactly_one_changed_path() {
        let dir = repo();
        fs::write(dir.path().join("new.txt"), "new\n").unwrap();

        let metadata = inspect_git_worktree(dir.path(), Some("HEAD"))
            .unwrap()
            .unwrap();

        assert_eq!(metadata.changed_files.unwrap(), vec!["new.txt"]);
    }

    #[test]
    fn linked_worktree_stays_clean_when_main_checkout_is_dirty() {
        let (_dir, main, linked) = repo_with_linked_worktree();
        fs::write(main.join("a.txt"), "dirty main\n").unwrap();

        let metadata = inspect_git_worktree(&linked, Some("HEAD"))
            .unwrap()
            .unwrap();

        assert!(metadata.changed_files.unwrap().is_empty());
    }

    #[test]
    fn one_committed_lane_commit_reports_its_files_since_base() {
        let dir = repo();
        let base = branch_head(dir.path());
        fs::write(dir.path().join("a.txt"), "two\n").unwrap();
        run(dir.path(), &["commit", "-am", "lane change"]);

        let metadata = inspect_git_worktree(dir.path(), Some(&base))
            .unwrap()
            .unwrap();

        assert_eq!(metadata.changed_files.unwrap(), vec!["a.txt"]);
    }

    #[test]
    fn multiple_commits_report_union_since_base() {
        let dir = repo();
        let base = branch_head(dir.path());
        fs::write(dir.path().join("a.txt"), "two\n").unwrap();
        run(dir.path(), &["commit", "-am", "first"]);
        fs::write(dir.path().join("b.txt"), "bee\n").unwrap();
        run(dir.path(), &["add", "b.txt"]);
        run(dir.path(), &["commit", "-m", "second"]);

        let metadata = inspect_git_worktree(dir.path(), Some(&base))
            .unwrap()
            .unwrap();

        assert_eq!(metadata.changed_files.unwrap(), vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn dirty_overlay_dedupes_with_committed_diff() {
        let dir = repo();
        let base = branch_head(dir.path());
        fs::write(dir.path().join("a.txt"), "two\n").unwrap();
        run(dir.path(), &["commit", "-am", "lane change"]);
        fs::write(dir.path().join("a.txt"), "three\n").unwrap();

        let metadata = inspect_git_worktree(dir.path(), Some(&base))
            .unwrap()
            .unwrap();

        assert_eq!(metadata.changed_files.unwrap(), vec!["a.txt"]);
    }

    #[test]
    fn invalid_base_is_refused() {
        let dir = repo();
        let err = inspect_git_worktree(dir.path(), Some("not-a-real-base"))
            .unwrap_err()
            .message();
        assert!(err.contains("invalid report base"));
    }

    #[test]
    fn leading_dash_base_is_refused() {
        let dir = repo();
        let err = inspect_git_worktree(dir.path(), Some("--bad"))
            .unwrap_err()
            .message();
        assert!(err.contains("starting with '-'"));
    }

    #[test]
    fn slash_ref_base_resolves_when_it_is_a_commit_ref() {
        let dir = repo();
        run(dir.path(), &["checkout", "-b", "lane/valid"]);
        fs::write(dir.path().join("a.txt"), "two\n").unwrap();
        run(dir.path(), &["commit", "-am", "lane change"]);
        let lane_head = branch_head(dir.path());
        run(dir.path(), &["checkout", "main"]);

        let metadata = inspect_git_worktree(dir.path(), Some("refs/heads/lane/valid"))
            .unwrap()
            .unwrap();

        assert_eq!(metadata.changed_files.unwrap(), vec!["a.txt"]);
        assert_ne!(metadata.sha.unwrap(), lane_head);
    }

    #[test]
    fn stale_readiness_sha_is_refused() {
        let err = {
            let dir = repo();
            enrich_ready_report_in(
                dir.path(),
                Some(ReportStatusArg::InReview),
                None,
                Some("deadbeef".into()),
                None,
                None,
                false,
            )
            .unwrap_err()
        };

        assert!(err
            .message()
            .contains("provided --sha deadbeef does not match HEAD"));
        assert!(err.message().contains("--status partial"));
    }

    #[test]
    fn wrong_branch_is_refused() {
        let dir = repo();

        let err = enrich_ready_report_in(
            dir.path(),
            Some(ReportStatusArg::InReview),
            Some("lane/wrong".into()),
            None,
            None,
            None,
            false,
        )
        .unwrap_err();

        assert!(err.message().contains("--branch"));
        assert!(err.message().contains("does not match current branch"));
    }

    #[test]
    fn non_git_directory_is_ignored() {
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(inspect_git_worktree(dir.path(), None).unwrap(), None);
    }

    #[test]
    fn partial_report_does_not_inspect_git() {
        let out = enrich_ready_report(
            Some(ReportStatusArg::Partial),
            None,
            None,
            None,
            Some(vec!["manual.rs".into()]),
            false,
        )
        .unwrap();

        assert_eq!(out.2, Some(vec!["manual.rs".into()]));
    }

    #[test]
    fn concurrent_foreign_head_advance_accepts_owned_sha() {
        let dir = repo();
        let base = branch_head(dir.path());
        fs::write(dir.path().join("owned.txt"), "owned\n").unwrap();
        run(dir.path(), &["add", "owned.txt"]);
        run(dir.path(), &["commit", "-m", "owned"]);
        let owned = branch_head(dir.path());
        fs::write(dir.path().join("foreign.txt"), "foreign\n").unwrap();
        run(dir.path(), &["add", "foreign.txt"]);
        run(dir.path(), &["commit", "-m", "foreign"]);

        let (_, reported, _) = enrich_ready_report_in(
            dir.path(),
            Some(ReportStatusArg::InReview),
            None,
            Some(owned.clone()),
            Some(base),
            None,
            false,
        )
        .unwrap();

        assert_eq!(reported.as_deref(), Some(owned.as_str()));
    }

    #[test]
    fn stale_sha_before_attempt_base_is_refused() {
        let dir = repo();
        let stale = branch_head(dir.path());
        fs::write(dir.path().join("base.txt"), "base\n").unwrap();
        run(dir.path(), &["add", "base.txt"]);
        run(dir.path(), &["commit", "-m", "attempt base"]);
        let base = branch_head(dir.path());
        fs::write(dir.path().join("owned.txt"), "owned\n").unwrap();
        run(dir.path(), &["add", "owned.txt"]);
        run(dir.path(), &["commit", "-m", "owned"]);

        for stale_or_base in [stale, base.clone()] {
            let err = enrich_ready_report_in(
                dir.path(),
                Some(ReportStatusArg::InReview),
                None,
                Some(stale_or_base),
                Some(base.clone()),
                None,
                false,
            )
            .unwrap_err();

            assert!(err.reason.contains("does not match HEAD"));
        }
    }

    #[test]
    fn unrelated_branch_sha_is_refused() {
        let dir = repo();
        let base = branch_head(dir.path());
        run(dir.path(), &["checkout", "-b", "unrelated"]);
        fs::write(dir.path().join("unrelated.txt"), "unrelated\n").unwrap();
        run(dir.path(), &["add", "unrelated.txt"]);
        run(dir.path(), &["commit", "-m", "unrelated"]);
        let unrelated = branch_head(dir.path());
        run(dir.path(), &["checkout", "main"]);
        fs::write(dir.path().join("owned.txt"), "owned\n").unwrap();
        run(dir.path(), &["add", "owned.txt"]);
        run(dir.path(), &["commit", "-m", "owned"]);

        let err = enrich_ready_report_in(
            dir.path(),
            Some(ReportStatusArg::InReview),
            None,
            Some(unrelated),
            Some(base),
            None,
            false,
        )
        .unwrap_err();

        assert!(err.reason.contains("does not match HEAD"));
    }

    #[test]
    fn dirty_owned_worktree_is_refused() {
        let dir = repo();
        let base = branch_head(dir.path());
        fs::write(dir.path().join("owned.txt"), "owned\n").unwrap();
        run(dir.path(), &["add", "owned.txt"]);
        run(dir.path(), &["commit", "-m", "owned"]);
        let owned = branch_head(dir.path());
        fs::write(dir.path().join("foreign.txt"), "foreign\n").unwrap();
        run(dir.path(), &["add", "foreign.txt"]);
        run(dir.path(), &["commit", "-m", "foreign"]);
        fs::write(dir.path().join("owned.txt"), "dirty owned\n").unwrap();

        let err = enrich_ready_report_in(
            dir.path(),
            Some(ReportStatusArg::InReview),
            None,
            Some(owned),
            Some(base),
            None,
            false,
        )
        .unwrap_err();

        assert_eq!(
            err.reason,
            "readiness report refused: this git worktree has uncommitted lane changes."
        );
        assert_eq!(err.dirty_files, vec!["owned.txt"]);
    }

    #[test]
    fn isolated_lane_still_requires_exact_head() {
        let (_dir, _main, linked) = repo_with_linked_worktree();
        let base = branch_head(&linked);
        fs::write(linked.join("owned.txt"), "owned\n").unwrap();
        run(&linked, &["add", "owned.txt"]);
        run(&linked, &["commit", "-m", "owned"]);
        let owned = branch_head(&linked);
        fs::write(linked.join("later.txt"), "later\n").unwrap();
        run(&linked, &["add", "later.txt"]);
        run(&linked, &["commit", "-m", "later"]);

        let err = enrich_ready_report_in(
            &linked,
            Some(ReportStatusArg::InReview),
            None,
            Some(owned),
            Some(base),
            None,
            false,
        )
        .unwrap_err();

        assert!(err.reason.contains("does not match HEAD"));
    }

    #[test]
    fn hint_does_not_advise_foreign_head() {
        let refusal = ReportRefusal {
            reason: "readiness report refused".into(),
            dirty_files: Vec::new(),
        };
        let message = refusal.message();
        let prompt = include_str!("../../../src-tauri/prompts/worker_suffix.md");

        for text in [message.as_str(), prompt] {
            assert!(!text.contains("$(git rev-parse HEAD)"));
            assert!(text.contains("own owned commit SHA"));
        }
        assert!(prompt.contains("commit only this task's owned changes"));
        assert!(prompt.contains("Never use `git add -A` or `git add .`"));
        assert!(prompt.contains("Read-only tasks need no commit"));
        assert!(prompt.contains("report `partial` with the exact blocker"));
        assert!(prompt.contains("hexadecimal literal"));
    }
}
