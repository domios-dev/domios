use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

fn git(dir: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    codeterm_util::process_util::no_window_std(&mut command);
    let output = command.current_dir(dir).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repository(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
    git(path, &["init", "-q"]);
    git(path, &["config", "user.email", "test@example.com"]);
    git(path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("README.md"), "fixture").unwrap();
    git(path, &["add", "README.md"]);
    git(path, &["commit", "-qm", "fixture"]);
}

struct Fixture {
    _root: tempfile::TempDir,
    project: PathBuf,
    lanes: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        repository(&project);
        let lanes = root.path().join("project-worktrees");
        std::fs::create_dir_all(&lanes).unwrap();
        Self {
            _root: root,
            project,
            lanes,
        }
    }

    fn canonical(&self) -> &str {
        self.project.to_str().unwrap()
    }

    fn worktree(&self, path: &Path, branch: &str) -> String {
        git(
            &self.project,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                branch,
                path.to_str().unwrap(),
            ],
        );
        path.display().to_string()
    }

    fn unrecorded(&self, worktree: &str, branch: &str) -> Option<String> {
        unmanaged_lane_reason_git(None, self.canonical(), Some(worktree), Some(branch))
    }
}

#[test]
fn unrecorded_lane_with_the_exact_provisioned_fingerprint_is_managed() {
    let fixture = Fixture::new();
    let lane = fixture.worktree(&fixture.lanes.join("task-a1b2"), "lane/task-a1b2");
    assert_eq!(fixture.unrecorded(&lane, "lane/task-a1b2"), None);
}

#[test]
fn unrecorded_lane_whose_branch_slug_differs_from_its_directory_is_not_managed() {
    let fixture = Fixture::new();
    let lane = fixture.worktree(&fixture.lanes.join("task-a1b2"), "lane/other-slug");
    let reason = fixture.unrecorded(&lane, "lane/other-slug").unwrap();
    assert!(reason.contains("not managed"), "{reason}");
}

#[test]
fn unrecorded_lane_outside_the_lane_root_is_not_managed() {
    let fixture = Fixture::new();
    let elsewhere = fixture.project.parent().unwrap().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let sibling = fixture.worktree(&elsewhere.join("task-a1b2"), "lane/task-a1b2");
    let nested = fixture.worktree(&fixture.lanes.join("group").join("deep"), "lane/deep");
    for (path, branch) in [(sibling, "lane/task-a1b2"), (nested, "lane/deep")] {
        let reason = fixture.unrecorded(&path, branch).unwrap();
        assert!(
            reason.contains("outside the Domios worktree root"),
            "{reason}"
        );
    }
}

#[test]
fn unrecorded_lane_on_a_non_lane_branch_is_not_managed() {
    let fixture = Fixture::new();
    let lane = fixture.worktree(&fixture.lanes.join("develop"), "develop");
    let reason = fixture.unrecorded(&lane, "develop").unwrap();
    assert!(reason.contains("not a Domios lane branch"), "{reason}");
    let bare = fixture.unrecorded(&lane, "lane/").unwrap();
    assert!(bare.contains("not a Domios lane branch"), "{bare}");
}

#[test]
fn unrecorded_directory_in_the_lane_root_that_is_not_a_registered_worktree_is_not_managed() {
    let fixture = Fixture::new();
    let plain = fixture.lanes.join("task-a1b2");
    std::fs::create_dir_all(&plain).unwrap();
    let reason = fixture
        .unrecorded(plain.to_str().unwrap(), "lane/task-a1b2")
        .unwrap();
    assert!(reason.contains("not managed"), "{reason}");
}

#[test]
fn unrecorded_lane_registered_to_another_repository_is_not_managed() {
    let fixture = Fixture::new();
    let other = fixture.project.parent().unwrap().join("other");
    repository(&other);
    let lane = fixture.lanes.join("task-a1b2");
    git(
        &other,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "lane/task-a1b2",
            lane.to_str().unwrap(),
        ],
    );
    let reason = fixture
        .unrecorded(lane.to_str().unwrap(), "lane/task-a1b2")
        .unwrap();
    assert!(reason.contains("not managed"), "{reason}");
}

#[test]
fn unrecorded_branch_without_a_worktree_is_not_managed() {
    let fixture = Fixture::new();
    let reason =
        unmanaged_lane_reason_git(None, fixture.canonical(), None, Some("lane/task-a1b2")).unwrap();
    assert!(reason.contains("not managed"), "{reason}");
}

#[test]
fn a_foreign_repository_root_is_never_managed_whatever_its_record() {
    let fixture = Fixture::new();
    let foreign = fixture.project.parent().unwrap().join("q6fc-foreign");
    repository(&foreign);
    let foreign = foreign.to_str().unwrap();
    for ownership in [
        Some(LaneOwnership::ForeignCheckout),
        Some(LaneOwnership::DomiosLane),
        None,
    ] {
        for branch in [
            Some("main"),
            Some("master"),
            Some("lane/q6fc-foreign"),
            None,
        ] {
            assert!(
                unmanaged_lane_reason_git(ownership, fixture.canonical(), Some(foreign), branch)
                    .is_some(),
                "{ownership:?} {branch:?}"
            );
        }
    }
    assert_eq!(
        unmanaged_lane_reason_git(
            Some(LaneOwnership::ForeignCheckout),
            fixture.canonical(),
            Some(foreign),
            Some("main")
        )
        .as_deref(),
        Some(FOREIGN_CHECKOUT)
    );
    assert!(
        unmanaged_lane_reason_git(None, fixture.canonical(), Some(fixture.canonical()), None)
            .is_some()
    );
}

#[test]
fn a_foreign_record_is_never_managed_even_on_a_lane_shaped_path() {
    let fixture = Fixture::new();
    let lane = fixture.worktree(&fixture.lanes.join("task-a1b2"), "lane/task-a1b2");
    assert_eq!(
        unmanaged_lane_reason_git(
            Some(LaneOwnership::ForeignCheckout),
            fixture.canonical(),
            Some(&lane),
            Some("lane/task-a1b2")
        )
        .as_deref(),
        Some(FOREIGN_CHECKOUT)
    );
}

#[test]
fn a_recorded_domios_lane_is_managed_inside_the_root_and_branch_namespace_only() {
    let fixture = Fixture::new();
    let lane = fixture.worktree(&fixture.lanes.join("task-a1b2"), "lane/task-a1b2");
    let owned = Some(LaneOwnership::DomiosLane);
    let probe_unused = |_: &str| -> Result<bool, String> { panic!("record decides") };
    assert_eq!(
        unmanaged_lane_reason(
            owned,
            fixture.canonical(),
            Some(&lane),
            Some("lane/task-a1b2"),
            probe_unused
        ),
        None
    );
    assert_eq!(
        unmanaged_lane_reason(
            owned,
            fixture.canonical(),
            None,
            Some("lane/task-a1b2"),
            probe_unused
        ),
        None
    );
    assert!(unmanaged_lane_reason(
        owned,
        fixture.canonical(),
        Some(&lane),
        Some("main"),
        probe_unused
    )
    .is_some());
}

#[test]
fn merged_records_keep_the_strictest_ownership() {
    use LaneOwnership::{DomiosLane, ForeignCheckout};
    assert_eq!(
        strictest_ownership(Some(DomiosLane), Some(ForeignCheckout)),
        Some(ForeignCheckout)
    );
    assert_eq!(
        strictest_ownership(None, Some(ForeignCheckout)),
        Some(ForeignCheckout)
    );
    assert_eq!(strictest_ownership(Some(DomiosLane), None), None);
    assert_eq!(
        strictest_ownership(Some(DomiosLane), Some(DomiosLane)),
        Some(DomiosLane)
    );
}
