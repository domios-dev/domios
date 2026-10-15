use super::*;
use std::fs;
use std::process::Command;
use std::process::Output;

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
        Instant::now() + CHECKPOINT_DEADLINE,
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

#[derive(Default)]
struct FixedRepository {
    lfs: bool,
    sparse: bool,
    refs: String,
    commands: Vec<(Vec<String>, bool)>,
}

impl checkpoint::Repository for FixedRepository {
    fn uses_lfs(&mut self) -> Result<bool, CaptureFailure> {
        Ok(self.lfs)
    }
    fn non_cone_sparse(&mut self) -> Result<bool, CaptureFailure> {
        Ok(self.sparse)
    }
    fn run(&mut self, args: &[&str], isolated_index: bool) -> Result<String, CaptureFailure> {
        self.commands.push((
            args.iter().map(|arg| (*arg).to_owned()).collect(),
            isolated_index,
        ));
        Ok(match args[0] {
            "write-tree" => "tree-object\n".into(),
            "-c" => "commit-object\n".into(),
            "for-each-ref" => self.refs.clone(),
            _ => String::new(),
        })
    }
}

#[test]
fn lane_checkpoint_refuses_lfs_repository() {
    let mut repository = FixedRepository {
        lfs: true,
        ..Default::default()
    };
    assert!(matches!(
        checkpoint::create(&mut repository, "attempt_lfs", 1).unwrap(),
        CheckpointResult::Refused(LaneCheckpointRefusal::LfsRepository)
    ));
    assert!(repository.commands.is_empty());
}

#[test]
fn lane_checkpoint_refuses_non_cone_sparse_checkout() {
    let mut repository = FixedRepository {
        sparse: true,
        ..Default::default()
    };
    assert!(matches!(
        checkpoint::create(&mut repository, "attempt_sparse", 1).unwrap(),
        CheckpointResult::Refused(LaneCheckpointRefusal::NonConeSparseCheckout)
    ));
    assert!(repository.commands.is_empty());
}

#[test]
fn lane_checkpoint_retains_latest_twenty_turn_refs() {
    let prefix = "refs/domios/checkpoints/attempt_retention";
    let refs = (1..=23)
        .rev()
        .map(|turn| format!("{prefix}/{turn}\n"))
        .collect::<String>()
        + &format!("{prefix}/metadata\nrefs/domios/checkpoints/foreign/1\n{prefix}/1/nested\n");
    let mut repository = FixedRepository {
        refs,
        ..Default::default()
    };
    assert!(
        matches!(checkpoint::create(&mut repository, "attempt_retention", 23).unwrap(),
        CheckpointResult::Created(name) if name == format!("{prefix}/23"))
    );
    let deleted: Vec<_> = repository
        .commands
        .iter()
        .filter(|(args, _)| args.starts_with(&["update-ref".into(), "-d".into()]))
        .map(|(args, _)| args[2].clone())
        .collect();
    let retained: Vec<_> = repository
        .refs
        .lines()
        .filter(|reference| !deleted.iter().any(|deleted| deleted == reference))
        .filter_map(|reference| checkpoint_turn(prefix, reference))
        .collect();
    assert_eq!(retained.len(), CHECKPOINTS_PER_ATTEMPT);
    assert_eq!(retained.iter().min(), Some(&4));
    assert_eq!(retained.iter().max(), Some(&23));
    assert!(repository
        .refs
        .lines()
        .filter(|reference| !deleted.iter().any(|deleted| deleted == reference))
        .any(|reference| reference == format!("{prefix}/metadata")));
    assert_eq!(
        deleted,
        (1..=3)
            .map(|turn| format!("{prefix}/{turn}"))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        repository
            .commands
            .iter()
            .filter(|(_, isolated)| *isolated)
            .count(),
        3
    );
    assert_eq!(
        repository.commands[0],
        (vec!["read-tree".into(), "HEAD".into()], true)
    );
    assert_eq!(
        repository.commands[1],
        (
            vec!["add".into(), "-A".into(), "--".into(), ".".into()],
            true
        )
    );
    assert!(repository.commands[3].0.contains(&"tree-object".into()));
    assert_eq!(
        repository.commands[4],
        (
            vec![
                "update-ref".into(),
                format!("{prefix}/23"),
                "commit-object".into()
            ],
            false
        )
    );
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
