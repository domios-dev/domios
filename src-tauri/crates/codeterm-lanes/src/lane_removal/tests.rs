use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use super::*;
use crate::worker_lane::provision_lane_for_test;

struct NullRepoEvents;

impl codeterm_repowatch::RepoEventSink for NullRepoEvents {
    fn repository_changed(&self, _: &str) {}

    fn repository_unavailable(&self, _: &str, _: codeterm_repowatch::RepoWatchUnavailable) {}
}

fn repo_watch() -> codeterm_repowatch::RepoWatchRegistryHandle {
    Arc::new(codeterm_repowatch::RepoWatchRegistry::new(Arc::new(
        NullRepoEvents,
    )))
}

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

fn link_dir(link: &Path, target: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).expect("create directory symlink");
    #[cfg(windows)]
    {
        let mut command = Command::new("cmd");
        process_util::no_window_std(&mut command);
        let output = command
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn lane_removal_links_dispatches_directory_and_file_links_by_metadata() {
    assert_eq!(
        link_removal_kind(true, true, false),
        Some(LinkRemovalKind::Directory)
    );
    assert_eq!(
        link_removal_kind(true, false, true),
        Some(LinkRemovalKind::Directory)
    );
    assert_eq!(
        link_removal_kind(true, false, false),
        Some(LinkRemovalKind::File)
    );
    assert_eq!(link_removal_kind(false, true, true), None);
}

#[test]
fn lane_removal_links_preserves_canonical_marker_and_removes_lane() {
    let root = tempfile::tempdir().unwrap();
    let project = fixture_project(root.path());
    let marker = project.join("canonical-marker");
    std::fs::create_dir(&marker).unwrap();
    let marker_file = marker.join("proof.bin");
    let bytes = [0, 1, 2, 127, 128, 254, 255];
    std::fs::write(&marker_file, bytes).unwrap();
    let lane = provision_lane_for_test(project.to_str().unwrap(), "link-marker").unwrap();
    link_dir(&Path::new(&lane.cwd).join("canonical-link"), &marker);

    remove_lane(
        &LaneRemovalRequest {
            worktree_path: lane.cwd.clone(),
            branch: Some(lane.branch.clone()),
            canonical_cwd: Some(project.display().to_string()),
            force: false,
        },
        &[],
    )
    .expect("lane with a link to canonical content removes safely");

    assert!(!Path::new(&lane.cwd).exists());
    assert_eq!(std::fs::read(marker_file).unwrap(), bytes);
}

#[test]
fn lane_removal_links_refuses_a_lane_with_leftover_content() {
    let root = tempfile::tempdir().unwrap();
    let project = fixture_project(root.path());
    let lane = provision_lane_for_test(project.to_str().unwrap(), "leftover-content").unwrap();
    let leftover = Path::new(&lane.cwd).join("untracked-leftover");
    std::fs::write(&leftover, "retain this lane content").unwrap();

    let error = remove_lane(
        &LaneRemovalRequest {
            worktree_path: lane.cwd.clone(),
            branch: Some(lane.branch.clone()),
            canonical_cwd: Some(project.display().to_string()),
            force: false,
        },
        &[],
    )
    .unwrap_err();

    assert!(error.contains("lane removal failed"), "{error}");
    assert!(error.contains(&lane.cwd), "{error}");
    assert_eq!(
        std::fs::read_to_string(leftover).unwrap(),
        "retain this lane content"
    );
    assert!(Path::new(&lane.cwd).is_dir());
}

#[test]
fn lane_removal_links_force_cleans_internal_node_modules_junctions() {
    let root = tempfile::tempdir().unwrap();
    let project = fixture_project(root.path());
    let outside_marker = project.join("outside-marker");
    std::fs::create_dir(&outside_marker).unwrap();
    let marker_file = outside_marker.join("proof.bin");
    let marker_bytes = [9, 0, 37, 128, 255];
    std::fs::write(&marker_file, marker_bytes).unwrap();

    let lane = provision_lane_for_test(project.to_str().unwrap(), "node-modules-links").unwrap();
    let modules = Path::new(&lane.cwd).join("node_modules");
    let packages = modules.join(".pnpm");
    let internal_target = packages.join("internal-target");
    std::fs::create_dir_all(&internal_target).unwrap();
    std::fs::write(internal_target.join("package.bin"), [4, 5, 6]).unwrap();
    for index in 0..4 {
        link_dir(
            &packages.join(format!("package-link-{index}")),
            &internal_target,
        );
    }
    link_dir(&modules.join("outside-marker-link"), &outside_marker);
    std::fs::write(modules.join("untracked-package.bin"), [1, 2, 3]).unwrap();

    remove_lane(
        &LaneRemovalRequest {
            worktree_path: lane.cwd.clone(),
            branch: Some(lane.branch.clone()),
            canonical_cwd: Some(project.display().to_string()),
            force: true,
        },
        &[],
    )
    .expect("forced lane removal unlinks internal and external junctions first");

    assert!(!Path::new(&lane.cwd).exists());
    assert_eq!(std::fs::read(marker_file).unwrap(), marker_bytes);
}

fn task_store(root: &Path, project: &Path) -> (PlanStore, String, String) {
    let store = PlanStore::new_for_test(root);
    let plan = store
        .create_plan_for_team(
            "Lane removal".into(),
            project.display().to_string(),
            codeterm_plan::plan::OrchestratorRef::from_tab("orch"),
            "team".into(),
            vec![],
            1,
        )
        .unwrap();
    let task = store
        .add_task(
            &plan.id,
            codeterm_plan::service_plan::NewTask {
                name: "worker".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    (store, plan.id, task)
}

fn record_lane(
    store: &PlanStore,
    plan: &str,
    task: &str,
    lane: &crate::worker_lane::ProvisionedWorkerLane,
) {
    store
        .update_task_lane(
            plan,
            task,
            TaskLaneUpdate {
                tab_id: Some(Some("worker-tab".into())),
                workspace_id: Some(Some("workspace".into())),
                workspace_name: Some(Some("Worker".into())),
                worktree_path: Some(Some(lane.cwd.clone())),
                branch: Some(Some(lane.branch.clone())),
                base: Some(Some(lane.base().to_string())),
                destination: Some(Some(lane.base().to_string())),
                ..Default::default()
            },
            3,
        )
        .unwrap();
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
            force: false,
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
            force: false,
        },
        &live_tabs,
    )
    .expect("an unheld owned lane removes through the existing cleanup path");

    assert_eq!(outcome.branch.as_deref(), Some(branch.as_str()));
    assert!(!Path::new(&lane_cwd).exists());
    assert!(!branch_exists(&project, &branch));
}

#[test]
fn lane_removal_refuses_unmerged_branch_before_deleting_worktree() {
    let root = tempfile::tempdir().unwrap();
    let project = fixture_project(root.path());
    let lane = provision_lane_for_test(project.to_str().unwrap(), "unmerged").unwrap();
    std::fs::write(Path::new(&lane.cwd).join("work.txt"), "unmerged work").unwrap();
    git(Path::new(&lane.cwd), &["add", "work.txt"]);
    git(Path::new(&lane.cwd), &["commit", "-qm", "unmerged work"]);

    let error = remove_lane(
        &LaneRemovalRequest {
            worktree_path: lane.cwd.clone(),
            branch: Some(lane.branch.clone()),
            canonical_cwd: Some(project.display().to_string()),
            force: false,
        },
        &[],
    )
    .unwrap_err();

    assert!(error.contains("commits not merged"), "{error}");
    assert!(error.contains("codeterm plan lane-remove"), "{error}");
    assert!(!error.contains("branch -D"), "{error}");
    assert!(Path::new(&lane.cwd).is_dir());
    assert!(branch_exists(&project, &lane.branch));
}

#[test]
fn canonical_checkout_is_refused_as_a_lane() {
    let root = tempfile::tempdir().unwrap();
    let project = fixture_project(&root.path().canonicalize().unwrap());

    let error = remove_lane(
        &LaneRemovalRequest {
            worktree_path: project.display().to_string(),
            branch: None,
            canonical_cwd: Some(project.display().to_string()),
            force: false,
        },
        &[],
    )
    .unwrap_err();

    assert!(error.contains("canonical checkout"), "{error}");
    assert!(project.is_dir());
}

#[test]
fn lane_remove_is_idempotent_for_an_already_removed_worktree() {
    let root = tempfile::tempdir().unwrap();
    let project = fixture_project(root.path());
    let lane = provision_lane_for_test(project.to_str().unwrap(), "already-removed").unwrap();
    let lane_path = lane.cwd.clone();
    let branch = lane.branch.clone();
    let (store, plan_id, task_id) = task_store(root.path(), &project);
    record_lane(&store, &plan_id, &task_id, &lane);
    store
        .with_plan(&plan_id, 4, |plan| {
            plan.tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .expect("fixture task")
                .execution_attempts
                .push(codeterm_plan::plan::TaskExecutionAttempt {
                    actual_reasoning_effort: None,
                    id: "attempt-kept".into(),
                    executor_tab_id: "worker-tab".into(),
                    branch: Some(branch.clone()),
                    worktree_path: Some(lane_path.clone()),
                    ..Default::default()
                });
            Ok(())
        })
        .unwrap();
    git(&project, &["worktree", "remove", "--force", &lane_path]);
    git(&project, &["branch", "-D", &branch]);

    let outcome = remove_lane_and_clear_task_references(
        &store,
        &repo_watch(),
        &LaneRemovalRequest {
            worktree_path: lane_path.clone(),
            branch: Some(branch.clone()),
            canonical_cwd: Some(project.display().to_string()),
            force: false,
        },
        &[],
    )
    .expect("repeating lane removal succeeds when the worktree and branch are gone");

    assert!(outcome.worktree_already_removed);
    assert!(outcome.branch_already_removed);
    let task = store.show_task(&task_id).unwrap().task;
    assert!(task.assigned_tab_id.is_none());
    assert!(task.lane.tab_id.is_none());
    assert!(task.lane.workspace_id.is_none());
    assert!(task.lane.workspace_name.is_none());
    assert!(task.lane.worktree_path.is_none());
    assert!(task.lane.branch.is_none());
    assert_eq!(task.lane.cleanup_status.as_deref(), Some("done"));
    assert_eq!(task.execution_attempts.len(), 1);
    assert_eq!(
        task.execution_attempts[0].worktree_path.as_deref(),
        Some(lane_path.as_str())
    );
}

#[test]
fn lane_checkpoint_refs_are_deleted_when_lane_is_removed() {
    let root = tempfile::tempdir().unwrap();
    let project = fixture_project(root.path());
    let lane = provision_lane_for_test(project.to_str().unwrap(), "checkpoint-cleanup").unwrap();
    let lane_path = lane.cwd.clone();
    let branch = lane.branch.clone();
    let (store, plan_id, task_id) = task_store(root.path(), &project);
    record_lane(&store, &plan_id, &task_id, &lane);
    store
        .with_plan(&plan_id, 4, |plan| {
            plan.tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .expect("fixture task")
                .execution_attempts
                .push(codeterm_plan::plan::TaskExecutionAttempt {
                    id: "attempt-cleanup".into(),
                    executor_tab_id: "worker-tab".into(),
                    worktree_path: Some(lane_path.clone()),
                    ..Default::default()
                });
            Ok(())
        })
        .unwrap();
    git(
        &project,
        &[
            "update-ref",
            "refs/domios/checkpoints/attempt-cleanup/1",
            "HEAD",
        ],
    );

    remove_lane_and_clear_task_references(
        &store,
        &repo_watch(),
        &LaneRemovalRequest {
            worktree_path: lane_path,
            branch: Some(branch),
            canonical_cwd: Some(project.display().to_string()),
            force: false,
        },
        &[],
    )
    .unwrap();

    let refs = Command::new("git")
        .current_dir(&project)
        .args([
            "for-each-ref",
            "--format=%(refname)",
            "refs/domios/checkpoints",
        ])
        .output()
        .unwrap();
    assert!(refs.status.success());
    assert!(refs.stdout.is_empty());
}

#[test]
fn lane_remove_prunes_a_registration_for_an_already_missing_worktree() {
    let root = tempfile::tempdir().unwrap();
    // macOS temp dirs sit behind the /var symlink, and git records the resolved path.
    let root_path = if cfg!(unix) {
        root.path().canonicalize().unwrap()
    } else {
        root.path().to_path_buf()
    };
    let project = fixture_project(&root_path);
    let lane = provision_lane_for_test(project.to_str().unwrap(), "missing-directory").unwrap();
    let lane_path = lane.cwd.clone();
    let branch = lane.branch.clone();
    let unrelated_path = root_path.join("other-stale-worktree");
    git(
        &project,
        &[
            "worktree",
            "add",
            "--detach",
            "-q",
            unrelated_path.to_str().unwrap(),
            "HEAD",
        ],
    );
    std::fs::remove_dir_all(&lane_path).unwrap();
    std::fs::remove_dir_all(&unrelated_path).unwrap();
    assert!(worktree_registered(project.to_str().unwrap(), &lane_path).unwrap());
    #[cfg(windows)]
    {
        let verbatim_path = format!("\\\\?\\{}", lane_path.replace('/', "\\"));
        assert!(worktree_registered(project.to_str().unwrap(), &verbatim_path).unwrap());
    }
    assert!(worktree_registered(
        project.to_str().unwrap(),
        &unrelated_path.display().to_string()
    )
    .unwrap());

    let outcome = remove_lane(
        &LaneRemovalRequest {
            worktree_path: lane_path.clone(),
            branch: Some(branch.clone()),
            canonical_cwd: Some(project.display().to_string()),
            force: false,
        },
        &[],
    )
    .expect("a missing worktree directory can be pruned idempotently");

    assert!(outcome.worktree_already_removed);
    assert!(!outcome.branch_already_removed);
    assert!(!worktree_registered(project.to_str().unwrap(), &lane_path).unwrap());
    assert!(worktree_registered(
        project.to_str().unwrap(),
        &unrelated_path.display().to_string()
    )
    .unwrap());
    assert!(!branch_exists(&project, &branch));
}

#[test]
fn lane_remove_reports_a_missing_branch_as_already_removed() {
    let root = tempfile::tempdir().unwrap();
    let root_path = PathBuf::from(codeterm_paths::canonical_cwd(
        &root.path().to_string_lossy(),
    ));
    let project = fixture_project(&root_path);
    let worktree = root_path.join("detached");
    git(
        &project,
        &[
            "worktree",
            "add",
            "--detach",
            "-q",
            worktree.to_str().unwrap(),
            "HEAD",
        ],
    );

    let outcome = remove_lane(
        &LaneRemovalRequest {
            worktree_path: worktree.display().to_string(),
            branch: Some("lane/already-gone".into()),
            canonical_cwd: Some(project.display().to_string()),
            force: false,
        },
        &[],
    )
    .expect("missing branch is an idempotent success");

    assert!(!outcome.worktree_already_removed);
    assert!(outcome.branch_already_removed);
    assert!(!worktree.exists());
}

#[test]
fn cleanup_status_done_clears_vanished_lane_references() {
    let root = tempfile::tempdir().unwrap();
    let project = fixture_project(root.path());
    let lane = provision_lane_for_test(project.to_str().unwrap(), "cleanup-done").unwrap();
    let lane_path = lane.cwd.clone();
    let branch = lane.branch.clone();
    let (store, plan_id, task_id) = task_store(root.path(), &project);
    record_lane(&store, &plan_id, &task_id, &lane);
    store
        .with_plan(&plan_id, 4, |plan| {
            plan.tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .expect("fixture task")
                .execution_attempts
                .push(codeterm_plan::plan::TaskExecutionAttempt {
                    actual_reasoning_effort: None,
                    id: "attempt-kept".into(),
                    executor_tab_id: "worker-tab".into(),
                    branch: Some(branch.clone()),
                    worktree_path: Some(lane_path.clone()),
                    ..Default::default()
                });
            Ok(())
        })
        .unwrap();
    git(&project, &["worktree", "remove", "--force", &lane_path]);
    git(&project, &["branch", "-D", &branch]);

    let update = clear_vanished_lane_on_cleanup_done(
        &store,
        &plan_id,
        &task_id,
        Some(TaskLaneUpdate {
            cleanup_status: Some(Some("done".into())),
            ..Default::default()
        }),
    )
    .unwrap()
    .unwrap();
    store
        .update_task_lane(&plan_id, &task_id, update, 5)
        .unwrap();

    let task = store.show_task(&task_id).unwrap().task;
    assert!(task.lane.tab_id.is_none());
    assert!(task.lane.workspace_id.is_none());
    assert!(task.lane.workspace_name.is_none());
    assert!(task.lane.worktree_path.is_none());
    assert!(task.lane.branch.is_none());
    assert!(task.assigned_tab_id.is_none());
    assert_eq!(task.lane.cleanup_status.as_deref(), Some("done"));
    assert_eq!(task.execution_attempts.len(), 1);
    assert_eq!(
        task.execution_attempts[0].worktree_path.as_deref(),
        Some(lane_path.as_str())
    );
}

#[test]
fn cleanup_status_done_clears_removed_worktree_with_merged_branch_kept() {
    let root = tempfile::tempdir().unwrap();
    let project = fixture_project(root.path());
    let lane = provision_lane_for_test(project.to_str().unwrap(), "merged-branch-kept").unwrap();
    let lane_path = lane.cwd.clone();
    let branch = lane.branch.clone();
    let (store, plan_id, task_id) = task_store(root.path(), &project);
    record_lane(&store, &plan_id, &task_id, &lane);
    git(&project, &["worktree", "remove", "--force", &lane_path]);

    let update = clear_vanished_lane_on_cleanup_done(
        &store,
        &plan_id,
        &task_id,
        Some(TaskLaneUpdate {
            cleanup_status: Some(Some("done".into())),
            ..Default::default()
        }),
    )
    .unwrap()
    .unwrap();
    store
        .update_task_lane(&plan_id, &task_id, update, 5)
        .unwrap();

    let task = store.show_task(&task_id).unwrap().task;
    assert!(branch_exists(&project, &branch));
    assert!(task.assigned_tab_id.is_none());
    assert!(task.lane.tab_id.is_none());
    assert!(task.lane.worktree_path.is_none());
    assert!(task.lane.branch.is_none());
    assert_eq!(task.lane.cleanup_status.as_deref(), Some("done"));
}
