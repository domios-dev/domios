//! The interval a readiness report attributes over, not the repository it reads.
//!
//! Worktree, sha and branch already resolve live, so a test that only checks the
//! repository passes on the broken code and proves nothing. Each case below pins
//! the changed-file SET — the interval — and asserts the repository facts in the
//! same breath, so a failure reads as *wrong interval* and never as *wrong repo*.

use super::ReportBase;
use crate::args::agent::ReportStatusArg;
use crate::cmd::report_guard::enrich_ready_report;
use crate::cmd::report_lane::TaskReadinessScope;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) {
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

fn head(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn commit(dir: &Path, file: &str, body: &str) -> String {
    let path = dir.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, body).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-m", file]);
    head(dir)
}

/// A tab re-dispatched on a new base inside the same shell: the integration
/// branch moved from `stale_env_base` to `live_base` (carrying `moved_on.rs`),
/// the attempt was re-dispatched onto `live_base`, and the worker committed
/// `lane_only.rs` on top. The launch environment still exports `stale_env_base`.
struct Redispatch {
    _dir: TempDir,
    lane: PathBuf,
    lane_branch: String,
    stale_env_base: String,
    live_base: String,
    owned_sha: String,
}

fn redispatched_lane() -> Redispatch {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    git(&root, &["init", "--initial-branch", "main"]);
    git(&root, &["config", "user.email", "test@example.invalid"]);
    git(&root, &["config", "user.name", "Test User"]);
    let stale_env_base = commit(&root, "README.md", "initial\n");
    let live_base = commit(&root, "moved_on.rs", "landed before the re-dispatch\n");

    let lane = root.join("lane");
    let lane_branch = "lane/redispatched".to_string();
    let lane_str = lane.to_string_lossy().into_owned();
    git(
        &root,
        &["worktree", "add", "-b", &lane_branch, &lane_str, &live_base],
    );
    let owned_sha = commit(&lane, "lane_only.rs", "the work actually done\n");
    Redispatch {
        _dir: dir,
        lane,
        lane_branch,
        stale_env_base,
        live_base,
        owned_sha,
    }
}

impl Redispatch {
    fn scope(&self, base: Option<&str>) -> TaskReadinessScope {
        TaskReadinessScope {
            worktree_path: Some(self.lane.clone()),
            base: base.map(str::to_string),
            requires_repository: true,
            ..TaskReadinessScope::default()
        }
    }

    fn report(
        &self,
        base: ReportBase,
        scope: &TaskReadinessScope,
    ) -> (Option<String>, Option<String>, Vec<String>) {
        let (branch, sha, changed) = enrich_ready_report(
            Some(scope),
            Some(ReportStatusArg::InReview),
            None,
            Some(self.owned_sha.clone()),
            base,
            None,
            false,
        )
        .expect("a clean lane on its own live base must produce a readiness report");
        (branch, sha, changed.unwrap_or_default())
    }

    /// The repository half of the claim, asserted alongside every interval case
    /// so a red test cannot be misread as the lane resolving to the wrong repo.
    fn assert_repo_facts(&self, branch: &Option<String>, sha: &Option<String>) {
        assert_eq!(
            branch.as_deref(),
            Some(self.lane_branch.as_str()),
            "the branch must resolve live from the lane worktree"
        );
        assert_eq!(
            sha.as_deref(),
            Some(self.owned_sha.as_str()),
            "the sha must resolve live from the lane worktree"
        );
    }
}

/// DEFECT TEST 1. Pre-change the environment base wins, so the report attributes
/// `moved_on.rs` — work that landed on the integration branch before this attempt
/// existed — to this worker. The repository facts are right in both worlds.
#[test]
fn changed_files_cover_the_live_attempt_base_after_a_redispatch() {
    let fx = redispatched_lane();
    let scope = fx.scope(Some(&fx.live_base));
    let base = ReportBase::classify(
        Some(fx.stale_env_base.clone()),
        Some(fx.stale_env_base.clone()),
    );

    let (branch, sha, changed) = fx.report(base, &scope);

    fx.assert_repo_facts(&branch, &sha);
    assert_eq!(
        changed,
        vec!["lane_only.rs".to_string()],
        "attribution must cover the live attempt base {}..HEAD, not the stale dispatch base {}..HEAD",
        fx.live_base,
        fx.stale_env_base
    );
}

#[test]
fn base_precedence_uses_explicit_then_live_then_environment() {
    for (supplied, environment, live, expected) in [
        (Some("typed"), Some("env"), Some("live"), Some("typed")),
        (Some("typed"), None, Some("stale-task-base"), Some("typed")),
        (Some("env"), Some("env"), Some("live"), Some("live")),
        (Some("env"), Some("env"), None, Some("env")),
        (None, Some("env"), Some("live"), Some("live")),
        (None, Some("env"), None, Some("env")),
        (None, None, Some("live"), Some("live")),
        (None, None, None, None),
        (Some(" typed "), Some(" env "), Some("live"), Some("typed")),
        (Some(" env "), Some("env"), Some(" live "), Some("live")),
        (Some(" "), Some("env"), Some(" "), Some("env")),
        (None, Some(" "), Some(" "), None),
    ] {
        let base = ReportBase::classify(
            supplied.map(str::to_string),
            environment.map(str::to_string),
        );
        assert_eq!(
            base.resolve(live.map(str::to_string)).as_deref(),
            expected,
            "supplied={supplied:?}, environment={environment:?}, live={live:?}"
        );
    }
}
