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
    let roots = [repo.path(), lane.as_path()];

    exercise_working_tree(&roots, 0, "primary repository");
    exercise_working_tree(&roots, 1, "linked worktree");
}

/// Both repositories are watched every time, and the assertion names one of
/// them: that is what proves the event carries the CORRECT identity.
fn exercise_working_tree(roots: &[&Path; 2], target: usize, where_: &str) {
    let root = roots[target];
    expect_change_from(roots, target, &format!("modifying a tracked file in the {where_}"), || {
        write(&root.join("tracked.txt"), "modified externally\n");
    });
    expect_change_from(roots, target, &format!("creating an untracked file in the {where_}"), || {
        write(&root.join("brand-new.txt"), "untracked\n");
    });
    expect_change_from(roots, target, &format!("deleting a tracked file in the {where_}"), || {
        std::fs::remove_file(root.join("tracked.txt")).expect("delete tracked file");
    });
    write(&root.join("tracked.txt"), "one\n");
}
