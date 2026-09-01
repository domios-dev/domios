use super::support::*;
use crate::repowatch::identity::RepoIdentity;
use crate::repowatch::open_handle_count;
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
    assert_eq!(
        ledger.built(),
        2,
        "one worktree watcher plus one shared ref watcher, built once each"
    );

    for token in tokens.iter().take(9) {
        assert!(registry.release(token), "release accepts a live token");
    }
    assert_eq!(registry.entry_count(), 1, "nine releases leave the entry");
    assert_eq!(registry.refcount(&identity.key), 1);
    assert_eq!(ledger.dropped(), 0, "no watcher dropped while referenced");

    assert!(registry.release(&tokens[9]));
    assert_eq!(registry.entry_count(), 0, "last release removes the entry");
    assert_eq!(registry.shared_count(), 0, "and its shared ref watch");
    assert_eq!(ledger.dropped(), 2, "both owning watchers dropped");

    let after = open_handle_count();
    println!("[R1] handle/descriptor count after full release: {after:?}");

    assert!(
        !registry.release(&tokens[9]),
        "a released token is not released twice"
    );
}
