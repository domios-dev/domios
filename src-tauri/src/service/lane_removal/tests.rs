use std::path::Path;
use std::process::Command;

use super::*;
use crate::service::worker_lane::provision_lane_for_test;

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

fn fixture_project(root: &Path) -> std::path::PathBuf {
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q"]);
    git(&project, &["config", "user.email", "test@example.com"]);
    git(&project, &["config", "user.name", "Test"]);
    std::fs::write(project.join("README.md"), "fixture").unwrap();
    git(&project, &["add", "README.md"]);
    git(&project, &["commit", "-qm", "fixture"]);
    project
}

fn branch_exists(project: &Path, branch: &str) -> bool {
    let mut command = Command::new("git");
    process_util::no_window_std(&mut command);
    command
        .current_dir(project)
        .args(["rev-parse", "--verify", "--quiet", branch])
        .output()
        .unwrap()
        .status
        .success()
}

#[test]
fn lane_removal_refuses_when_tab_holds_worktree() {
    let root = tempfile::tempdir().unwrap();
    let project = fixture_project(root.path());
    let lane = provision_lane_for_test(project.to_str().unwrap(), "holder").unwrap();

    let nested = Path::new(&lane.cwd).join("src");
    std::fs::create_dir_all(&nested).unwrap();
    let holder = LiveTabCwd {
        id: "tab_holder".into(),
        title: "Planck".into(),
        cwd: nested.display().to_string(),
    };
    let elsewhere = LiveTabCwd {
        id: "tab_other".into(),
        title: "Canonical".into(),
        cwd: project.display().to_string(),
    };

    let error = remove_lane(
        &LaneRemovalRequest {
            worktree_path: lane.cwd.clone(),
            branch: Some(lane.branch.clone()),
            canonical_cwd: Some(project.display().to_string()),
        },
        &[elsewhere, holder],
    )
    .unwrap_err();

    assert!(error.contains("tab_holder"), "{error}");
    assert!(error.contains("Planck"), "{error}");
    assert!(error.contains("still in use"), "{error}");
    assert!(
        error.contains("codeterm tab close --tab tab_holder"),
        "{error}"
    );
    assert!(
        Path::new(&lane.cwd).is_dir(),
        "a refused removal must leave the worktree on disk"
    );
    assert!(branch_exists(&project, &lane.branch));
    lane.cleanup().unwrap();
}

#[test]
fn lane_removal_succeeds_when_worktree_unheld() {
    let root = tempfile::tempdir().unwrap();
    let project = fixture_project(root.path());
    let lane = provision_lane_for_test(project.to_str().unwrap(), "unheld").unwrap();
    let branch = lane.branch.clone();
    let lane_cwd = lane.cwd.clone();

    // A tab in the canonical checkout and one in a sibling path that merely
    // shares the lane's name prefix are not holders of this worktree.
    let live_tabs = vec![
        LiveTabCwd {
            id: "tab_canonical".into(),
            title: "Canonical".into(),
            cwd: project.display().to_string(),
        },
        LiveTabCwd {
            id: "tab_sibling".into(),
            title: "Sibling".into(),
            cwd: format!("{lane_cwd}-sibling"),
        },
    ];

    let outcome = remove_lane(
        &LaneRemovalRequest {
            worktree_path: lane_cwd.clone(),
            branch: Some(branch.clone()),
            canonical_cwd: Some(project.display().to_string()),
        },
        &live_tabs,
    )
    .expect("an unheld owned lane removes through the existing cleanup path");

    assert_eq!(outcome.branch.as_deref(), Some(branch.as_str()));
    assert!(!Path::new(&lane_cwd).exists());
    assert!(!branch_exists(&project, &branch));
}

#[test]
fn canonical_checkout_is_refused_as_a_lane() {
    let root = tempfile::tempdir().unwrap();
    let project = fixture_project(root.path());

    let error = remove_lane(
        &LaneRemovalRequest {
            worktree_path: project.display().to_string(),
            branch: None,
            canonical_cwd: Some(project.display().to_string()),
        },
        &[],
    )
    .unwrap_err();

    assert!(error.contains("canonical checkout"), "{error}");
    assert!(project.is_dir());
}
