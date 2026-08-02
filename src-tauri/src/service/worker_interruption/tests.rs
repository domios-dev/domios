use super::*;

fn run(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .unwrap();
    assert!(status.success(), "git {}", args.join(" "));
}

#[test]
fn exhaustion_after_dirty_edit_seals_and_verifies_exact_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    run(dir.path(), &["init"]);
    run(dir.path(), &["config", "user.email", "fixture@example.invalid"]);
    run(dir.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(dir.path().join("tracked.txt"), "base\n").unwrap();
    run(dir.path(), &["add", "tracked.txt"]);
    run(dir.path(), &["commit", "-m", "base"]);
    let base = git(dir.path(), &["rev-parse", "HEAD"]).unwrap();
    std::fs::write(dir.path().join("tracked.txt"), "dirty\n").unwrap();
    std::fs::write(dir.path().join("untracked.bin"), [0_u8, 1, 255]).unwrap();

    let checkpoint = seal_git_checkpoint(CheckpointRequest {
        task_id: "task",
        attempt_id: "attempt",
        tab_id: "tab",
        session_id: "session",
        owner_epoch: 1,
        repo_path: dir.path(),
        worktree_path: dir.path(),
        base_head: base.trim(),
        completed_tests: vec![],
        unresolved_work: "continue".into(),
        next_acceptance_gate: "focused QA".into(),
    })
    .unwrap();

    assert!(checkpoint.git_status.contains("tracked.txt"));
    assert!(checkpoint.git_status.contains("untracked.bin"));
    assert!(checkpoint.diff_artifact.contains("untracked.bin\0"));
    verify_git_checkpoint(&checkpoint).unwrap();
    std::fs::write(dir.path().join("tracked.txt"), "changed again\n").unwrap();
    assert!(verify_git_checkpoint(&checkpoint).is_err());
}

#[test]
fn interruption_events_are_provider_neutral() {
    assert_eq!(
        WorkerInterruptionEvent::ProviderExhausted.reason(),
        "provider_exhausted"
    );
    assert_eq!(WorkerInterruptionEvent::ProcessExited.reason(), "process_exited");
    assert_eq!(
        WorkerInterruptionEvent::ForcedContextReset.reason(),
        "forced_context_reset"
    );
}
