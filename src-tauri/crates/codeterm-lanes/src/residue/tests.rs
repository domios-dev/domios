use super::*;

fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let cwd = project.to_str().unwrap();
    git(cwd, &["init", "-q"]).unwrap();
    git(
        cwd,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-qm",
            "fixture",
        ],
    )
    .unwrap();
    let root = worktree_root(&project).unwrap();
    std::fs::create_dir(&root).unwrap();
    (temp, project, root)
}

#[test]
fn residue_model_protects_registered_canonical_linked_and_source_directories() {
    assert!(residue_decision(true, false, false, true).is_some());
    assert!(residue_decision(false, true, false, true).is_some());
    assert!(residue_decision(false, false, true, true).is_some());
    assert!(residue_decision(false, false, false, false).is_some());
    assert_eq!(residue_decision(false, false, false, true), None);
}

#[test]
fn residue_sweep_removes_only_artifacts_and_rechecks_registration() {
    let (_temp, project, root) = fixture();
    let residue = root.join("orphan");
    std::fs::create_dir_all(residue.join("node_modules/package")).unwrap();
    std::fs::create_dir_all(residue.join("src-tauri/target/debug")).unwrap();
    std::fs::create_dir_all(residue.join("cli/target/release")).unwrap();
    std::fs::write(residue.join("README.md"), "fixture").unwrap();
    let source = root.join("source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("main.rs"), "owned source").unwrap();
    let nested_git = root.join("repository");
    std::fs::create_dir_all(nested_git.join("target/.git")).unwrap();
    let live = root.join("registered");
    let cwd = project.to_str().unwrap();
    git(
        cwd,
        &["worktree", "add", "-b", "lane/live", live.to_str().unwrap()],
    )
    .unwrap();
    let rows = inventory(cwd, &[]).unwrap();
    assert_eq!(rows.iter().filter(|row| row.refusal.is_none()).count(), 1);
    assert!(residue.exists(), "inventory is read-only");
    remove(cwd, &residue, &[]).unwrap();
    assert!(!residue.exists());
    assert!(remove(cwd, &source, &[]).is_err());
    assert!(remove(cwd, &live, &[]).is_err());
    assert!(remove(cwd, &nested_git, &[]).is_err());
    assert!(remove(cwd, project.parent().unwrap(), &[]).is_err());
    let raced = root.join("registered-after-preview");
    std::fs::create_dir(&raced).unwrap();
    assert!(inventory(cwd, &[])
        .unwrap()
        .iter()
        .any(|row| row.path == raced && row.refusal.is_none()));
    git(
        cwd,
        &[
            "worktree",
            "add",
            "-b",
            "lane/raced",
            raced.to_str().unwrap(),
        ],
    )
    .unwrap();
    assert!(remove(cwd, &raced, &[]).is_err());
    assert!(raced.exists());
}

#[test]
fn residue_sweep_keeps_live_holders() {
    let (_temp, project, root) = fixture();
    let residue = root.join("occupied");
    std::fs::create_dir(&residue).unwrap();
    let tabs = vec![LiveTabCwd {
        id: "holder".into(),
        title: "Shell".into(),
        cwd: residue.display().to_string(),
    }];
    let error = remove(project.to_str().unwrap(), &residue, &tabs).unwrap_err();
    assert!(error.contains("holder"));
    assert!(residue.exists());
}

#[test]
fn residue_sweep_unlinks_node_modules_junction_without_entering_target() {
    let (_temp, project, root) = fixture();
    let outside = project.join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("proof"), "preserved").unwrap();
    let residue = root.join("junction-residue");
    std::fs::create_dir(&residue).unwrap();
    let link = residue.join("node_modules");
    #[cfg(windows)]
    {
        let mut command = std::process::Command::new("cmd");
        codeterm_util::process_util::no_window_std(&mut command);
        let output = command
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    remove(project.to_str().unwrap(), &residue, &[]).unwrap();
    assert!(!residue.exists());
    assert_eq!(
        std::fs::read_to_string(outside.join("proof")).unwrap(),
        "preserved"
    );
}
