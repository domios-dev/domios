use std::fs;
use std::path::Path;
use std::process::Command;

use super::read_workspace_essence_maps_core;

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .expect("run git fixture command");
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn read_workspace_essence_maps_ignores_linked_worktree_copy() {
    let fixture = tempfile::tempdir().expect("create git fixture");
    let canonical = fixture.path().join("canonical");
    let worktree = fixture.path().join("worktree");
    fs::create_dir(&canonical).expect("create canonical checkout");
    run_git(&canonical, &["init", "--quiet"]);
    run_git(
        &canonical,
        &[
            "-c",
            "user.name=Map Test",
            "-c",
            "user.email=map-test@example.invalid",
            "commit",
            "--allow-empty",
            "--message",
            "map fixture",
        ],
    );
    run_git(
        &canonical,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            worktree.to_str().expect("worktree path is UTF-8"),
            "HEAD",
        ],
    );

    let canonical_essences = canonical.join("essences");
    let worktree_essences = worktree.join("essences");
    fs::create_dir_all(&canonical_essences).expect("create canonical essences");
    fs::create_dir_all(&worktree_essences).expect("create worktree essences");
    fs::write(canonical_essences.join("map.jsonl"), "canonical map").expect("write canonical map");
    fs::write(worktree_essences.join("map.jsonl"), "worktree copy")
        .expect("write worktree map copy");

    let files = read_workspace_essence_maps_core(&[
        canonical.display().to_string(),
        worktree.display().to_string(),
    ]);

    assert_eq!(files.len(), 1);
    assert_eq!(
        files[0].project_path,
        canonical
            .canonicalize()
            .expect("resolve canonical checkout")
            .display()
            .to_string()
    );
    assert_eq!(files[0].content.as_deref(), Some("canonical map"));
    assert_ne!(
        files[0].project_path,
        worktree
            .canonicalize()
            .expect("resolve linked worktree")
            .display()
            .to_string()
    );
}
