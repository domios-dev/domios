use std::path::Path;
use std::process::Command;

use super::*;

fn input<'a>(
    tab_live: bool,
    worktree_exists: bool,
    branch: Option<&'a str>,
    target: Option<&'a str>,
) -> LaneCleanupInput<'a> {
    LaneCleanupInput {
        tab_id: Some("tab-1"),
        tab_live,
        worktree_path: Some("/tmp/lane"),
        worktree_exists,
        branch,
        branch_exists: branch.is_some(),
        integration_target: target,
    }
}

#[test]
fn merged_branch_cleans_worktree_then_branch() {
    let decision =
        decide_lane_cleanup(&input(false, true, Some("lane/x"), Some("main")), |_, _| {
            true
        });
    assert_eq!(
        decision,
        LaneCleanupDecision::Clean(vec![
            LaneCleanupAction::RemoveWorktree("/tmp/lane".into()),
            LaneCleanupAction::DeleteBranch("lane/x".into()),
        ])
    );
}

#[test]
fn unmerged_branch_is_refused() {
    let decision =
        decide_lane_cleanup(&input(false, true, Some("lane/x"), Some("main")), |_, _| {
            false
        });
    assert!(matches!(decision, LaneCleanupDecision::Refuse(r) if r.contains("not merged")));
}

#[test]
fn live_tab_is_refused_before_any_delete() {
    let mut merge_checked = false;
    let decision = decide_lane_cleanup(&input(true, true, Some("lane/x"), Some("main")), |_, _| {
        merge_checked = true;
        true
    });
    assert!(matches!(decision, LaneCleanupDecision::Refuse(r) if r.contains("still live")));
    assert!(!merge_checked, "must not merge-check while tab is live");
}

#[test]
fn missing_integration_target_is_refused() {
    let decision = decide_lane_cleanup(&input(false, true, Some("lane/x"), None), |_, _| true);
    assert!(
        matches!(decision, LaneCleanupDecision::Refuse(r) if r.contains("no known integration target"))
    );
}

#[test]
fn no_residue_is_noop() {
    let mut input = input(false, false, None, Some("main"));
    input.worktree_path = None;
    let decision = decide_lane_cleanup(&input, |_, _| true);
    assert_eq!(decision, LaneCleanupDecision::Noop);
}

#[test]
fn missing_worktree_and_branch_are_cleaned_as_stale_references() {
    let mut input = input(false, false, Some("lane/gone"), Some("main"));
    input.branch_exists = false;

    assert_eq!(
        decide_lane_cleanup(&input, |_, _| false),
        LaneCleanupDecision::Clean(Vec::new())
    );
}

#[test]
fn worktree_only_skips_merge_gate() {
    let mut merge_checked = false;
    let decision = decide_lane_cleanup(&input(false, true, None, Some("main")), |_, _| {
        merge_checked = true;
        true
    });
    assert_eq!(
        decision,
        LaneCleanupDecision::Clean(vec![LaneCleanupAction::RemoveWorktree("/tmp/lane".into())])
    );
    assert!(
        !merge_checked,
        "no branch -> no irreversible delete -> no gate"
    );
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} failed");
}

fn scratch_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["config", "user.email", "t@t.t"]);
    git(dir, &["config", "user.name", "t"]);
    std::fs::write(dir.join("a.txt"), "a").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "init"]);
    tmp
}

#[test]
fn branch_is_merged_reflects_ancestry() {
    let tmp = scratch_repo();
    let dir = tmp.path().to_str().unwrap();
    // Merged branch: points at an ancestor of main.
    git(tmp.path(), &["branch", "merged"]);
    // Unmerged branch: has a commit main never saw.
    git(tmp.path(), &["checkout", "-q", "-b", "unmerged"]);
    std::fs::write(tmp.path().join("b.txt"), "b").unwrap();
    git(tmp.path(), &["add", "."]);
    git(tmp.path(), &["commit", "-qm", "diverge"]);
    git(tmp.path(), &["checkout", "-q", "main"]);

    assert!(branch_is_merged(dir, "merged", "main"));
    assert!(!branch_is_merged(dir, "unmerged", "main"));
    assert!(!branch_is_merged(dir, "does-not-exist", "main"));
}

#[test]
fn run_action_removes_clean_worktree_and_deletes_branch() {
    let tmp = scratch_repo();
    let dir = tmp.path();
    let wt = tmp.path().join("wt");
    let wt_str = wt.to_str().unwrap();
    git(dir, &["worktree", "add", "-q", wt_str, "-b", "lane/x"]);
    assert!(wt.exists());

    // Merged (no new commits on the lane) -> gate passes.
    assert!(branch_is_merged(dir.to_str().unwrap(), "lane/x", "main"));
    assert!(run_action(
        dir.to_str().unwrap(),
        &LaneCleanupAction::RemoveWorktree(wt_str.into())
    )
    .is_ok());
    assert!(!wt.exists());
    assert!(run_action(
        dir.to_str().unwrap(),
        &LaneCleanupAction::DeleteBranch("lane/x".into())
    )
    .is_ok());
}

#[test]
fn run_action_refuses_dirty_worktree() {
    let tmp = scratch_repo();
    let dir = tmp.path();
    let wt = tmp.path().join("wt");
    let wt_str = wt.to_str().unwrap();
    git(dir, &["worktree", "add", "-q", wt_str, "-b", "lane/x"]);
    std::fs::write(wt.join("dirty.txt"), "uncommitted").unwrap();

    let err = run_action(
        dir.to_str().unwrap(),
        &LaneCleanupAction::RemoveWorktree(wt_str.into()),
    )
    .unwrap_err();
    assert!(err.contains("could not be removed"));
    assert!(
        wt.exists(),
        "dirty worktree must be left intact, never forced"
    );
}
