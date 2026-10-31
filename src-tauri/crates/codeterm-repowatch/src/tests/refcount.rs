use super::support::*;
use crate::identity::RepoIdentity;
use crate::open_handle_count;
use std::sync::Arc;

/// R1 — one registry entry and one owning watcher per canonical identity,
/// regardless of subscriber count.
#[test]
fn repository_watch_registry_is_refcounted_per_identity() {
    if !git_available() {
        eprintln!("skipping: git is not on PATH");
        return;
    }
    let repo = primary_repo();
    let identity = RepoIdentity::resolve(repo.path()).expect("identity resolves");

    let ledger = Arc::new(WatchLedger::default());
    let (registry, _rx) = registry_with(Arc::new(CountingBackend {
        ledger: ledger.clone(),
    }));

    // MEASURED EVIDENCE, not an asserted equality: notify does not portably
    // expose per-watcher handle counts. Visible with `-- --nocapture`.
    let before = open_handle_count();
    println!("[R1] handle/descriptor count before subscribe: {before:?}");

    let tokens: Vec<String> = (0..10)
        .map(|i| {
            registry
                .subscribe(repo.path())
                .unwrap_or_else(|e| panic!("subscriber {i} failed: {e}"))
                .token
        })
        .collect();

    let peak = open_handle_count();
    println!("[R1] handle/descriptor count with 10 subscribers: {peak:?}");

    assert_eq!(registry.entry_count(), 1, "ten subscribers, one entry");
    assert_eq!(
        registry.refcount(&identity.key),
        10,
        "refcount tracks subscribers"
    );
    assert_eq!(registry.shared_count(), 1, "one shared ref watch");
    assert_eq!(ledger.built(), 1, "one repository watch set, built once");

    for token in tokens.iter().take(9) {
        assert!(registry.release(token), "release accepts a live token");
    }
    assert_eq!(registry.entry_count(), 1, "nine releases leave the entry");
    assert_eq!(registry.refcount(&identity.key), 1);
    assert_eq!(ledger.dropped(), 0, "no watcher dropped while referenced");

    assert!(registry.release(&tokens[9]));
    assert_eq!(registry.entry_count(), 0, "last release removes the entry");
    assert_eq!(registry.shared_count(), 0, "and its shared ref watch");
    assert_eq!(ledger.dropped(), 1, "the shared watch set stopped");

    let after = open_handle_count();
    println!("[R1] handle/descriptor count after full release: {after:?}");

    assert!(
        !registry.release(&tokens[9]),
        "a released token is not released twice"
    );
}

#[test]
fn repository_watch_set_is_shared_across_tabs_worktrees_and_lane_checks() {
    let repo = primary_repo();
    let holder = linked_worktree(repo.path(), "shared-watch-set");
    let lane = holder.path().join("shared-watch-set");
    let ledger = Arc::new(WatchLedger::default());
    let (registry, _) = registry_with(Arc::new(CountingBackend {
        ledger: ledger.clone(),
    }));
    let first = registry.subscribe(repo.path()).unwrap();
    let second = registry.subscribe(repo.path()).unwrap();
    let worktree = registry.subscribe(&lane).unwrap();
    registry
        .retain_lane_watch("lane-check", repo.path())
        .unwrap();
    registry
        .retain_lane_watch("lane-check", repo.path())
        .unwrap();
    assert_eq!(registry.entry_count(), 2);
    assert_eq!(registry.shared_count(), 1);
    assert_eq!(ledger.built(), 1);
    assert_eq!(ledger.updated.load(std::sync::atomic::Ordering::SeqCst), 1);
    registry.release(&first.token);
    registry.release(&second.token);
    registry.release(&worktree.token);
    assert_eq!(ledger.dropped(), 0);
    assert!(registry.release_lane_watch("lane-check"));
    assert_eq!(ledger.dropped(), 1);
    assert_eq!(registry.shared_count(), 0);
}
