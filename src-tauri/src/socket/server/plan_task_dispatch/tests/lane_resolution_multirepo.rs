use super::super::resolve_dispatch_lane;
use crate::plan::TaskLaneMetadata;
use crate::service::LaneProtection;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(path: &Path, args: &[&str]) {
    let output = Command::new("git").current_dir(path).args(args).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

fn repository(container: &Path, name: &str) -> PathBuf {
    let project = container.join(name);
    std::fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q"]);
    git(&project, &["config", "user.email", "test@example.com"]);
    git(&project, &["config", "user.name", "Test"]);
    std::fs::write(project.join("README.md"), name).unwrap();
    git(&project, &["add", "README.md"]);
    git(&project, &["commit", "-qm", "fixture"]);
    project
}

fn assert_resolves_to(resolved: &str, expected: &Path) {
    assert_eq!(
        std::fs::canonicalize(resolved).unwrap(),
        std::fs::canonicalize(expected).unwrap()
    );
}

#[test]
fn lane_resolution_multirepo_explicit_repo_beats_nonrepo_workspace() {
    let container = tempfile::tempdir().unwrap();
    let backend = repository(container.path(), "backend");

    let resolved = resolve_dispatch_lane(
        Some(backend.to_str().unwrap()),
        Some(container.path().to_str().unwrap()),
        None,
        &TaskLaneMetadata::default(),
        LaneProtection::Enforced,
    )
    .unwrap();

    assert_resolves_to(&resolved.spawn_cwd, &backend);
}

#[test]
fn lane_resolution_multirepo_honours_each_address_before_the_next() {
    let container = tempfile::tempdir().unwrap();
    let recorded_repo = repository(container.path(), "recorded");
    let explicit_repo = repository(container.path(), "explicit");
    let workspace_repo = repository(container.path(), "workspace");
    let caller_repo = repository(container.path(), "caller");
    let linked = container.path().join("linked");
    git(
        &recorded_repo,
        &["worktree", "add", "-b", "lane/recorded", linked.to_str().unwrap()],
    );

    let recorded = resolve_dispatch_lane(
        Some(explicit_repo.to_str().unwrap()),
        Some(workspace_repo.to_str().unwrap()),
        Some(caller_repo.to_str().unwrap()),
        &TaskLaneMetadata {
            worktree_path: Some(linked.display().to_string()),
            branch: Some("lane/recorded".into()),
            ..Default::default()
        },
        LaneProtection::Enforced,
    )
    .unwrap();
    assert_resolves_to(&recorded.spawn_cwd, &linked);

    let explicit = resolve_dispatch_lane(
        Some(explicit_repo.to_str().unwrap()),
        Some(workspace_repo.to_str().unwrap()),
        Some(caller_repo.to_str().unwrap()),
        &TaskLaneMetadata::default(),
        LaneProtection::Enforced,
    )
    .unwrap();
    assert_resolves_to(&explicit.spawn_cwd, &explicit_repo);

    let workspace = resolve_dispatch_lane(
        None,
        Some(workspace_repo.to_str().unwrap()),
        Some(caller_repo.to_str().unwrap()),
        &TaskLaneMetadata::default(),
        LaneProtection::Enforced,
    )
    .unwrap();
    assert_resolves_to(&workspace.spawn_cwd, &workspace_repo);

    let caller = resolve_dispatch_lane(
        None,
        Some(container.path().to_str().unwrap()),
        Some(caller_repo.to_str().unwrap()),
        &TaskLaneMetadata::default(),
        LaneProtection::Enforced,
    )
    .unwrap();
    assert_resolves_to(&caller.spawn_cwd, &caller_repo);

    git(&recorded_repo, &["worktree", "remove", "--force", linked.to_str().unwrap()]);
}

#[test]
fn lane_resolution_multirepo_accepts_a_recorded_linked_worktree() {
    let container = tempfile::tempdir().unwrap();
    let project = repository(container.path(), "backend");
    let linked = container.path().join("lanes").join("worker");
    std::fs::create_dir_all(linked.parent().unwrap()).unwrap();
    git(
        &project,
        &["worktree", "add", "-b", "lane/linked", linked.to_str().unwrap()],
    );
    assert!(linked.join(".git").is_file());

    let resolved = resolve_dispatch_lane(
        None,
        Some(container.path().to_str().unwrap()),
        None,
        &TaskLaneMetadata {
            worktree_path: Some(linked.display().to_string()),
            branch: Some("lane/linked".into()),
            ..Default::default()
        },
        LaneProtection::Enforced,
    )
    .unwrap();

    assert_resolves_to(&resolved.spawn_cwd, &linked);
    git(&project, &["worktree", "remove", "--force", linked.to_str().unwrap()]);
}

#[test]
fn lane_resolution_multirepo_keeps_single_repo_workspace_resolution() {
    let container = tempfile::tempdir().unwrap();
    let project = repository(container.path(), "project");

    let resolved = resolve_dispatch_lane(
        None,
        Some(project.to_str().unwrap()),
        None,
        &TaskLaneMetadata::default(),
        LaneProtection::Enforced,
    )
    .unwrap();

    assert_resolves_to(&resolved.spawn_cwd, &project);
}

#[test]
fn lane_resolution_multirepo_names_every_unresolvable_address() {
    let container = tempfile::tempdir().unwrap();
    let recorded = container.path().join("missing-recorded");
    let explicit = container.path().join("missing-explicit");
    let workspace = container.path().join("missing-workspace");
    let caller = container.path().join("missing-caller");
    let error = resolve_dispatch_lane(
        Some(explicit.to_str().unwrap()),
        Some(workspace.to_str().unwrap()),
        Some(caller.to_str().unwrap()),
        &TaskLaneMetadata {
            worktree_path: Some(recorded.display().to_string()),
            ..Default::default()
        },
        LaneProtection::Enforced,
    )
    .unwrap_err();
    let message = error.message;
    let positions = [
        recorded.to_str().unwrap(),
        explicit.to_str().unwrap(),
        workspace.to_str().unwrap(),
        caller.to_str().unwrap(),
    ]
    .map(|path| message.find(path).unwrap());
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
}
