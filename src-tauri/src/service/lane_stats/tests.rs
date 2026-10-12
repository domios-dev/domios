use super::*;
use std::fs;
use std::process::Command;

// Failure deadline only: each checkpoint spawns several git processes, slow under a loaded Windows runner.
const CHECKPOINT_DEADLINE: Duration = Duration::from_secs(60);

fn repository() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init"]);
    git(root.path(), &["config", "user.name", "Lane Stats Test"]);
    git(
        root.path(),
        &["config", "user.email", "lane-stats@example.invalid"],
    );
    fs::write(root.path().join("tracked.txt"), "base\n").unwrap();
    git(root.path(), &["add", "tracked.txt"]);
    git(root.path(), &["commit", "-m", "base"]);
    root
}

fn git(cwd: &Path, args: &[&str]) -> Output {
    let mut command = Command::new("git");
    let output = command.current_dir(cwd).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn git_text(cwd: &Path, args: &[&str]) -> String {
    let output = git(cwd, args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
fn lane_turn_stats_capture_unstaged_staged_and_untracked_files() {
    let root = repository();
    fs::write(root.path().join("tracked.txt"), "base\nstaged\n").unwrap();
    git(root.path(), &["add", "tracked.txt"]);
    fs::write(root.path().join("tracked.txt"), "base\nstaged\nunstaged\n").unwrap();
    fs::write(root.path().join("new.txt"), "untracked\n").unwrap();

    let (unstaged, staged, untracked) =
        capture_change_stats(root.path(), Instant::now() + Duration::from_secs(4)).unwrap();

    assert!(unstaged.starts_with("1\t0\ttracked.txt"));
    assert!(staged.starts_with("1\t0\ttracked.txt"));
    assert_eq!(untracked, 1);
}

#[test]
fn lane_turn_stats_never_rewrite_the_lane_index() {
    let root = repository();
    let index = root.path().join(".git").join("index");
    let tracked = root.path().join("tracked.txt");
    let rewritten_at = fs::metadata(&index).unwrap().modified().unwrap() + Duration::from_secs(2);
    fs::write(&tracked, "base\n").unwrap();
    fs::File::options()
        .write(true)
        .open(&tracked)
        .unwrap()
        .set_modified(rewritten_at)
        .unwrap();
    let before = fs::read(&index).unwrap();

    let (unstaged, staged, _) =
        capture_change_stats(root.path(), Instant::now() + Duration::from_secs(4)).unwrap();

    assert_eq!(fs::read(&index).unwrap(), before);
    assert!(unstaged.trim().is_empty(), "{unstaged}");
    assert!(staged.trim().is_empty(), "{staged}");
}

#[test]
fn lane_turn_stats_deduplicates_repeated_settlement_for_same_transcript_turn() {
    let messages = vec![crate::agent::chat_types::ChatMsg {
        id: "assistant-message-1".into(),
        kind: "assistant".into(),
        seq: 4,
        content: "completed response".into(),
        ..Default::default()
    }];
    let first = transcript_turn_key("codex", "session-1", &messages).unwrap();
    let repeated = transcript_turn_key("codex", "session-1", &messages).unwrap();
    let next = transcript_turn_key(
        "codex",
        "session-1",
        &[crate::agent::chat_types::ChatMsg {
            id: "assistant-message-2".into(),
            kind: "assistant".into(),
            seq: 5,
            content: "next completed response".into(),
            ..Default::default()
        }],
    )
    .unwrap();
    let stats = vec![LaneTurnChangeStats {
        transcript_turn_key: Some(first.clone()),
        ..Default::default()
    }];

    assert_eq!(first, repeated);
    assert_ne!(first, next);
    assert!(is_duplicate_transcript_turn(&stats, Some(&repeated)));
    assert!(!is_duplicate_transcript_turn(&stats, Some(&next)));
    assert!(!is_duplicate_transcript_turn(&stats, None));
}

#[test]
fn lane_checkpoint_preserves_user_index_worktree_branches_and_stash() {
    let root = repository();
    fs::write(root.path().join("tracked.txt"), "base\nstaged\n").unwrap();
    git(root.path(), &["add", "tracked.txt"]);
    fs::write(root.path().join("tracked.txt"), "base\nstaged\nunstaged\n").unwrap();
    fs::write(root.path().join("new.txt"), "untracked\n").unwrap();
    let index = fs::read(root.path().join(".git").join("index")).unwrap();
    let status = git(root.path(), &["status", "--porcelain", "-z"]).stdout;
    let branches = git_text(
        root.path(),
        &["for-each-ref", "--format=%(refname)", "refs/heads"],
    );
    let stash = git_text(root.path(), &["stash", "list"]);

    let result = create_checkpoint(
        root.path(),
        "attempt_safe",
        1,
        Instant::now() + Duration::from_secs(8),
    )
    .unwrap();

    assert!(
        matches!(result, CheckpointResult::Created(ref name) if name == "refs/domios/checkpoints/attempt_safe/1")
    );
    assert_eq!(
        fs::read(root.path().join(".git").join("index")).unwrap(),
        index
    );
    assert_eq!(
        git(root.path(), &["status", "--porcelain", "-z"]).stdout,
        status
    );
    assert_eq!(
        git_text(
            root.path(),
            &["for-each-ref", "--format=%(refname)", "refs/heads"]
        ),
        branches
    );
    assert_eq!(git_text(root.path(), &["stash", "list"]), stash);
    assert_eq!(
        git_text(
            root.path(),
            &[
                "for-each-ref",
                "--format=%(refname)",
                "refs/domios/checkpoints"
            ]
        ),
        "refs/domios/checkpoints/attempt_safe/1"
    );
}

#[test]
fn lane_checkpoint_refuses_lfs_repository() {
    let root = repository();
    fs::write(
        root.path().join(".gitattributes"),
        "*.bin filter=lfs diff=lfs merge=lfs -text\n",
    )
    .unwrap();
    git(root.path(), &["add", ".gitattributes"]);
    git(root.path(), &["commit", "-m", "lfs attributes"]);

    let result = create_checkpoint(
        root.path(),
        "attempt_lfs",
        1,
        Instant::now() + CHECKPOINT_DEADLINE,
    )
    .unwrap();

    assert!(matches!(
        result,
        CheckpointResult::Refused(LaneCheckpointRefusal::LfsRepository)
    ));
}

#[test]
fn lane_checkpoint_refuses_non_cone_sparse_checkout() {
    let root = repository();
    git(root.path(), &["config", "core.sparseCheckout", "true"]);
    git(root.path(), &["config", "core.sparseCheckoutCone", "false"]);

    let result = create_checkpoint(
        root.path(),
        "attempt_sparse",
        1,
        Instant::now() + CHECKPOINT_DEADLINE,
    )
    .unwrap();

    assert!(matches!(
        result,
        CheckpointResult::Refused(LaneCheckpointRefusal::NonConeSparseCheckout)
    ));
}

#[test]
fn lane_checkpoint_retains_latest_twenty_turn_refs() {
    let root = repository();
    git(
        root.path(),
        &[
            "update-ref",
            "refs/domios/checkpoints/attempt_retention/metadata",
            "HEAD",
        ],
    );
    for turn in 1..=23 {
        create_checkpoint(
            root.path(),
            "attempt_retention",
            turn,
            Instant::now() + CHECKPOINT_DEADLINE,
        )
        .unwrap();
    }
    let refs = git_text(
        root.path(),
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/domios/checkpoints/attempt_retention",
        ],
    );
    assert!(refs
        .lines()
        .any(|reference| reference == "refs/domios/checkpoints/attempt_retention/metadata"));
    let mut turns: Vec<u32> = refs
        .lines()
        .filter_map(|reference| {
            checkpoint_turn("refs/domios/checkpoints/attempt_retention", reference)
        })
        .collect();
    assert_eq!(turns.len(), CHECKPOINTS_PER_ATTEMPT);
    turns.sort_unstable();
    assert_eq!(turns.first(), Some(&4));
    assert_eq!(turns.last(), Some(&23));
}

#[test]
fn lane_checkpoint_deletes_only_the_named_attempt_refs() {
    let root = repository();
    for (attempt, turn) in [("attempt_remove", 1), ("attempt_keep", 1)] {
        create_checkpoint(
            root.path(),
            attempt,
            turn,
            Instant::now() + CHECKPOINT_DEADLINE,
        )
        .unwrap();
    }

    delete_attempt_checkpoint_refs(root.path(), "attempt_remove").unwrap();

    assert_eq!(
        git_text(
            root.path(),
            &[
                "for-each-ref",
                "--format=%(refname)",
                "refs/domios/checkpoints"
            ]
        ),
        "refs/domios/checkpoints/attempt_keep/1"
    );
}

#[test]
fn lane_merge_offer_requires_ancestor_and_leaves_lane_intact() {
    let root = repository();
    git(root.path(), &["branch", "lane/stats"]);
    git(root.path(), &["checkout", "lane/stats"]);
    fs::write(root.path().join("tracked.txt"), "base\nlane\n").unwrap();
    git(root.path(), &["add", "tracked.txt"]);
    git(root.path(), &["commit", "-m", "lane change"]);
    git(root.path(), &["branch", "destination", "HEAD~1"]);
    assert!(!branch_is_ancestor(
        root.path(),
        "lane/stats",
        "destination",
        Instant::now() + Duration::from_secs(4),
    )
    .unwrap());

    git(root.path(), &["checkout", "destination"]);
    git(root.path(), &["merge", "--ff-only", "lane/stats"]);
    let before = git_text(root.path(), &["rev-parse", "lane/stats"]);
    let destination = git_text(root.path(), &["rev-parse", "destination"]);
    assert!(branch_is_ancestor(
        root.path(),
        "lane/stats",
        &destination,
        Instant::now() + Duration::from_secs(4),
    )
    .unwrap());
    assert_eq!(git_text(root.path(), &["rev-parse", "lane/stats"]), before);
    assert_eq!(
        git_text(root.path(), &["rev-parse", "destination"]),
        destination
    );
    assert!(root.path().join("tracked.txt").is_file());
}

#[test]
fn lane_checkpoint_ref_component_rejects_ref_path_injection() {
    assert!(!safe_ref_component("../foreign"));
    assert!(safe_ref_component("attempt-12_ab"));
    assert_eq!(
        checkpoint_turn(
            "refs/domios/checkpoints/attempt",
            "refs/domios/checkpoints/attempt/4"
        ),
        Some(4)
    );
    assert_eq!(
        checkpoint_turn(
            "refs/domios/checkpoints/attempt",
            "refs/domios/checkpoints/attempt/metadata"
        ),
        None
    );
    assert_eq!(
        checkpoint_turn(
            "refs/domios/checkpoints/attempt",
            "refs/domios/checkpoints/attempt/1/nested"
        ),
        None
    );
}
