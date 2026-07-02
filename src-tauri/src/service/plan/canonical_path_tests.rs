use super::*;
use crate::plan::OrchestratorRef;
use crate::process_util;
use std::path::{Path, PathBuf};
use std::process::Command;

fn run_git(cwd: &Path, args: &[&str]) {
    let mut cmd = Command::new("git");
    process_util::no_window_std(&mut cmd);
    let out = cmd.current_dir(cwd).args(args).output().unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

fn git_worktree_pair() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("main");
    let linked = tmp.path().join("linked");
    std::fs::create_dir(&main).unwrap();
    run_git(&main, &["init"]);
    run_git(&main, &["config", "user.email", "test@example.com"]);
    run_git(&main, &["config", "user.name", "Test User"]);
    std::fs::write(main.join("README.md"), "root\n").unwrap();
    run_git(&main, &["add", "README.md"]);
    run_git(&main, &["commit", "-m", "init"]);
    run_git(
        &main,
        &[
            "worktree",
            "add",
            linked.to_str().unwrap(),
            "-b",
            "linked-test",
        ],
    );
    (tmp, main, linked)
}

#[test]
fn create_from_linked_worktree_persists_canonical_project_path_and_routes_sidecars() {
    let (_tmp, main, linked) = git_worktree_pair();
    let store = PlanStore::new();
    std::fs::create_dir_all(linked.join(".codeterm/plans")).unwrap();

    let plan = store
        .create_plan(
            "Canonical route".into(),
            linked.display().to_string(),
            OrchestratorRef::from_pane("owner"),
            vec![],
            1,
        )
        .unwrap();
    store
        .add_task(
            &plan.id,
            NewTask {
                name: "Write sidecar".into(),
                kind: "fix".into(),
                body: "body".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();

    let main_root = paths::project_root(&main);
    let loaded = store.get(&plan.id).unwrap();
    assert_eq!(loaded.project_path, main_root.display().to_string());
    assert_eq!(
        loaded.canonical_project_path,
        main_root.display().to_string()
    );
    assert!(main_root
        .join(format!(".codeterm/plans/active/{}/plan.json", plan.id))
        .is_file());
    assert!(main_root
        .join(format!(
            ".codeterm/plans/active/{}/tasks/write-sidecar.md",
            plan.id
        ))
        .is_file());
    assert!(!linked
        .join(format!(".codeterm/plans/active/{}/plan.json", plan.id))
        .exists());
}

#[test]
fn stale_canonical_project_path_falls_back_to_loaded_project_root() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("main");
    let deleted = tmp.path().join("deleted-linked");
    std::fs::create_dir(&project).unwrap();

    let mut plan = crate::plan::Plan::new(
        "Stale home".into(),
        deleted.display().to_string(),
        OrchestratorRef::from_pane("owner"),
    );
    plan.canonical_project_path = deleted.display().to_string();
    paths::normalize_plan_paths(&mut plan, &project);

    assert_eq!(plan.project_path, project.display().to_string());
    assert_eq!(plan.canonical_project_path, project.display().to_string());
}
