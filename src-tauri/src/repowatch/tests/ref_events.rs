use super::support::*;
use crate::repowatch::filter::{batch_is_relevant, WatchHalf};
use crate::repowatch::identity::RepoIdentity;

/// R3 — branch and index events, including a ref update that lands in
/// `packed-refs`.
#[test]
fn repository_watcher_emits_ref_and_index_events() {
    if !git_available() {
        eprintln!("skipping: git is not on PATH");
        return;
    }
    let repo = primary_repo();
    let holder = linked_worktree(repo.path(), "lane");
    let lane = holder.path().join("lane");

    let (registry, rx) = live_registry();
    let primary = registry.subscribe(repo.path()).expect("primary subscribe");
    let worktree = registry.subscribe(&lane).expect("lane subscribe");

    // 3. staging writes the index.
    settle(&rx);
    write(&repo.path().join("tracked.txt"), "one\ntwo\n");
    git(repo.path(), &["add", "tracked.txt"]);
    expect_change(&rx, &primary.identity, "staging a file");

    // 1. commit writes index and HEAD.
    settle(&rx);
    git(repo.path(), &["commit", "-q", "-m", "second"]);
    expect_change(&rx, &primary.identity, "a commit");

    // 2. checkout -b moves HEAD and creates a loose ref.
    settle(&rx);
    git(repo.path(), &["checkout", "-q", "-b", "topic"]);
    expect_change(&rx, &primary.identity, "a branch checkout");

    // 4. the packed file itself is in the watch set — watching refs/ alone
    // would silently miss an update that lands there.
    let identity = RepoIdentity::resolve(repo.path()).expect("identity resolves");
    assert!(
        batch_is_relevant(
            &identity,
            WatchHalf::SharedRefs,
            &[identity.common_dir.join("packed-refs")]
        )
        .expect("classification succeeds"),
        "packed-refs under commondir must be relevant"
    );
    settle(&rx);
    git(repo.path(), &["pack-refs", "--all"]);
    expect_change(&rx, &primary.identity, "git pack-refs --all");

    // 5. the linked worktree emits for its OWN identity.
    settle(&rx);
    git(&lane, &["checkout", "-q", "-b", "lane-topic"]);
    expect_change(&rx, &worktree.identity, "a checkout in the linked worktree");
}
