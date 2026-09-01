use super::support::*;
use crate::repowatch::filter::{batch_is_relevant, WatchHalf};
use crate::repowatch::identity::RepoIdentity;

/// R3 — branch and index events, including a ref update that lands in
/// `packed-refs`. Each case owns a fresh watch, so no stale event can pass it.
#[test]
fn repository_watcher_emits_ref_and_index_events() {
    if !git_available() {
        eprintln!("skipping: git is not on PATH");
        return;
    }
    let repo = primary_repo();
    let holder = linked_worktree(repo.path(), "lane");
    let lane = holder.path().join("lane");
    let root = repo.path();

    // 3. staging writes the index.
    expect_change_from(&[root], 0, "staging a file", || {
        write(&root.join("tracked.txt"), "one\ntwo\n");
        git(root, &["add", "tracked.txt"]);
    });

    // 1. commit writes index and HEAD.
    expect_change_from(&[root], 0, "a commit", || {
        git(root, &["commit", "-q", "-m", "second"]);
    });

    // 2. checkout -b moves HEAD and creates a loose ref.
    expect_change_from(&[root], 0, "a branch checkout", || {
        git(root, &["checkout", "-q", "-b", "topic"]);
    });

    // 4. the packed file itself is in the watch set — watching refs/ alone
    // would silently miss an update that lands there.
    let identity = RepoIdentity::resolve(root).expect("identity resolves");
    assert!(
        batch_is_relevant(
            &identity,
            WatchHalf::SharedRefs,
            &[identity.common_dir.join("packed-refs")]
        )
        .expect("classification succeeds"),
        "packed-refs under commondir must be relevant"
    );
    expect_change_from(&[root], 0, "git pack-refs --all", || {
        git(root, &["pack-refs", "--all"]);
    });

    // 5. the linked worktree emits for its OWN identity, with both watched.
    let watched = watch_all(&[root, lane.as_path()]);
    assert_eq!(watched.registry.entry_count(), 2, "two identities, two entries");
    git(&lane, &["checkout", "-q", "-b", "lane-topic"]);
    expect_change(
        &watched.rx,
        &watched.identities[1],
        "a checkout in the linked worktree",
    );
}
