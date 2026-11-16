use super::*;
use std::process::Command;

fn files(edit: &[&str]) -> TaskFiles {
    TaskFiles {
        edit: edit.iter().map(|path| path.to_string()).collect(),
        ..Default::default()
    }
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?} failed: {output:?}");
}

fn shared() -> Result<(WorkerPlacement, String), String> {
    Ok((WorkerPlacement::Shared, "git reason".into()))
}

#[test]
fn history_without_marker_or_commit_is_no_git_and_failures_stay_errors() {
    let unprobed = || -> HeadProbe { panic!("a folder without .git is decided without git") };
    assert_eq!(classify_history(false, unprobed), Ok(ProjectHistory::NoGit));
    assert_eq!(
        classify_history(true, || HeadProbe::Commit),
        Ok(ProjectHistory::Git)
    );
    assert_eq!(
        classify_history(true, || HeadProbe::Unborn),
        Ok(ProjectHistory::NoGit)
    );
    assert_eq!(
        classify_history(true, || HeadProbe::Failed("corrupt".into())),
        Err("corrupt".into())
    );
}

#[test]
fn project_history_follows_repository_and_first_commit() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_string_lossy().into_owned();
    assert_eq!(project_history(&path), Ok(ProjectHistory::NoGit));
    git(root.path(), &["init", "-q"]);
    assert_eq!(project_history(&path), Ok(ProjectHistory::NoGit));
    std::fs::write(root.path().join("a.txt"), "a").unwrap();
    git(root.path(), &["add", "a.txt"]);
    git(
        root.path(),
        &[
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "first",
        ],
    );
    assert_eq!(project_history(&path), Ok(ProjectHistory::Git));
}

#[test]
fn placement_without_git_refuses_lanes_and_canonical_writer_and_keeps_shared() {
    for requested in ["lane", "isolated_lane", "canonical_writer"] {
        let refusal =
            placement_without_git(Some(requested), || panic!("never chosen")).unwrap_err();
        assert!(refusal.contains(NO_GIT_HISTORY), "{refusal}");
    }
    let automatic_lane = placement_without_git(None, || {
        Ok((
            WorkerPlacement::IsolatedLane,
            "task is marked build-breaking".into(),
        ))
    })
    .unwrap_err();
    assert!(automatic_lane.contains("task is marked build-breaking"));
    assert!(automatic_lane.contains(NO_GIT_HISTORY));
    assert_eq!(
        placement_without_git(Some("shared"), || Err("shared placement refused: x".into())),
        Err("shared placement refused: x".into())
    );
    assert_eq!(
        placement_without_git(None, shared).unwrap().0,
        WorkerPlacement::Shared
    );
}

#[test]
fn declared_file_hashes_track_edits_creations_and_removals() {
    let root = tempfile::tempdir().unwrap();
    let checkout = root.path().to_string_lossy().into_owned();
    std::fs::create_dir_all(root.path().join("docs/inner")).unwrap();
    std::fs::write(root.path().join("notes.md"), "one").unwrap();
    std::fs::write(root.path().join("docs/inner/a.md"), "a").unwrap();
    std::fs::write(root.path().join("other.md"), "foreign").unwrap();
    let declared = files(&["notes.md", "docs", "new.md"]);
    let start = hash_declared_files(&checkout, &declared).unwrap();
    assert_eq!(
        start.keys().collect::<Vec<_>>(),
        ["docs/inner/a.md", "notes.md"]
    );

    std::fs::write(root.path().join("notes.md"), "two").unwrap();
    std::fs::remove_file(root.path().join("docs/inner/a.md")).unwrap();
    std::fs::write(root.path().join("new.md"), "created").unwrap();
    std::fs::write(root.path().join("other.md"), "changed").unwrap();
    let current = hash_declared_files(&checkout, &declared).unwrap();
    let changed = changed_file_hashes(&start, &current);
    assert_eq!(
        changed.keys().collect::<Vec<_>>(),
        ["docs/inner/a.md", "new.md", "notes.md"]
    );
    assert_eq!(changed["docs/inner/a.md"], None);
    assert_eq!(
        changed["notes.md"].as_deref(),
        current.get("notes.md").map(String::as_str)
    );
    assert!(changed["new.md"].as_deref().unwrap().starts_with("sha256:"));
}

#[test]
fn report_without_git_needs_no_sha_and_a_changed_declared_file() {
    let declared = files(&["notes.md"]);
    let start = FileHashes::from([("notes.md".to_string(), "sha256:a".to_string())]);
    let edited = FileHashes::from([("notes.md".to_string(), "sha256:b".to_string())]);
    assert!(matches!(
        validate_report_without_git(&declared, &start, &edited, Some(&"a".repeat(40))),
        Err(SharedReportError::Rejected(reason)) if reason.contains("--sha")
    ));
    assert!(matches!(
        validate_report_without_git(&declared, &start, &start, None),
        Err(SharedReportError::Rejected(reason)) if reason.contains("no declared edit file changed")
    ));
    assert_eq!(
        validate_report_without_git(&declared, &start, &edited, None).unwrap(),
        ChangedFileHashes::from([("notes.md".to_string(), Some("sha256:b".to_string()))])
    );
}
