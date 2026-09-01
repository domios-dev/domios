use super::support::*;
use crate::repowatch::events::RepoWatchUnavailable;
use std::sync::Arc;

/// R6 — every degraded path fails closed and loudly, with a named
/// discriminant and no polling fallback anywhere.
#[test]
fn repository_watch_failures_are_explicit() {
    if !git_available() {
        eprintln!("skipping: git is not on PATH");
        return;
    }

    // 1. a path that is not a repository.
    let plain = tempfile::tempdir().expect("temp dir");
    let (registry, rx) = live_registry();
    let error = registry
        .subscribe(plain.path())
        .expect_err("a non-repository must not produce a live watch");
    assert_eq!(error.reason, RepoWatchUnavailable::NotARepository);
    expect_unavailable(&rx, RepoWatchUnavailable::NotARepository, "a non-repository path");
    assert_eq!(registry.entry_count(), 0, "no entry for a failed subscribe");

    // 3. a watch the OS refuses, injected at the construction seam.
    let repo = primary_repo();
    let (refusing, refused_rx) = registry_with(Arc::new(RefusingBackend));
    let error = refusing
        .subscribe(repo.path())
        .expect_err("a refused watch must not report success");
    assert_eq!(error.reason, RepoWatchUnavailable::WatchRefused);
    expect_unavailable(&refused_rx, RepoWatchUnavailable::WatchRefused, "an OS-refused watch");
    assert_eq!(
        refusing.entry_count(),
        0,
        "a refused watch leaves no entry behind"
    );
    assert_eq!(refusing.shared_count(), 0);

    // 2. a linked worktree deleted while watched.
    let holder = linked_worktree(repo.path(), "doomed");
    let lane = holder.path().join("doomed");
    let (live, live_rx) = live_registry();
    let sub = live.subscribe(&lane).expect("lane subscribe");
    settle(&live_rx);
    std::fs::remove_dir_all(&lane).expect("delete the watched worktree");
    let reported = expect_unavailable(
        &live_rx,
        RepoWatchUnavailable::RepositoryGone,
        "deleting a watched worktree",
    );
    assert!(
        reported.to_lowercase() == sub.identity.to_lowercase(),
        "the unavailable signal names the watched identity ({reported} vs {})",
        sub.identity
    );
}
