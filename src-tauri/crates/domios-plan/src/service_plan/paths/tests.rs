use super::*;

#[test]
fn restore_plans_git_probe_cached_and_parallel() {
    let base = tempfile::tempdir().unwrap();
    let cwds: Vec<_> = (0..23)
        .map(|i| {
            let dir = base.path().join(format!("plain-{i}"));
            std::fs::create_dir_all(&dir).unwrap();
            dir
        })
        .collect();
    assert!(cwds.iter().all(|dir| !super::has_git_marker(dir)));

    super::warm_project_roots(&cwds, 8);

    for dir in &cwds {
        let expected = dir.canonicalize().unwrap();
        assert_eq!(super::project_root(dir), expected);
    }

    let repo = base.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    assert!(super::has_git_marker(&repo));
    assert!(super::has_git_marker(&repo.join("nested")));
}

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

#[test]
fn project_root_resolves_linked_worktree_to_main_worktree() {
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

    let main_root = project_root(&main);
    let linked_root = project_root(&linked);

    assert_eq!(main_root, main.canonicalize().unwrap());
    assert_eq!(linked_root, main_root);
}

#[test]
fn project_root_ignores_linked_worktree_domios_for_plan_sidecars() {
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

    std::fs::create_dir_all(linked.join(".domios").join("plans")).unwrap();

    assert_eq!(project_root(&linked), main.canonicalize().unwrap());
}

#[test]
fn project_plan_sidecars_use_domios_directory() {
    let project = tempfile::tempdir().unwrap();
    mark_test_project_root(project.path());
    let expected = project
        .path()
        .canonicalize()
        .unwrap()
        .join(".domios")
        .join("plans");
    assert_eq!(plan_state_root_for(project.path()), expected);
    assert_eq!(
        specs_dir(project.path()).unwrap(),
        project.path().join(".domios/specs")
    );
}
