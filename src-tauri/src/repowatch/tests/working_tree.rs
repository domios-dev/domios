use super::support::*;
use std::path::Path;

/// R4 — working-tree events. `statusBubble` renders porcelain counts, so a
/// tracked-file edit made outside the app must move the bubble.
#[test]
fn repository_watcher_emits_working_tree_events() {
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

    exercise_working_tree(&rx, repo.path(), &primary.identity, "primary repository");
    exercise_working_tree(&rx, &lane, &worktree.identity, "linked worktree");
}

fn exercise_working_tree(
    rx: &std::sync::mpsc::Receiver<SinkEvent>,
    root: &Path,
    identity: &str,
    where_: &str,
) {
    settle(rx);
    write(&root.join("tracked.txt"), "modified externally\n");
    expect_change(rx, identity, &format!("modifying a tracked file in the {where_}"));

    settle(rx);
    write(&root.join("brand-new.txt"), "untracked\n");
    expect_change(rx, identity, &format!("creating an untracked file in the {where_}"));

    settle(rx);
    std::fs::remove_file(root.join("tracked.txt")).expect("delete tracked file");
    expect_change(rx, identity, &format!("deleting a tracked file in the {where_}"));

    // Leave the tree as found so the next half starts from a known state.
    settle(rx);
    write(&root.join("tracked.txt"), "one\n");
}
