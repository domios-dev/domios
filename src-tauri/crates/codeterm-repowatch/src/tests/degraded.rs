use super::support::*;
use crate::backend::WatchSignal;
use crate::events::RepoWatchUnavailable;
use crate::identity::RepoIdentity;
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
    expect_unavailable(
        &rx,
        RepoWatchUnavailable::NotARepository,
        "a non-repository path",
    );
    assert_eq!(registry.entry_count(), 0, "no entry for a failed subscribe");

    // 3. a watch the OS refuses, injected at the construction seam.
    let repo = primary_repo();
    let (refusing, refused_rx) = registry_with(Arc::new(RefusingBackend));
    let error = refusing
        .subscribe(repo.path())
        .expect_err("a refused watch must not report success");
    assert_eq!(error.reason, RepoWatchUnavailable::WatchRefused);
    expect_unavailable(
        &refused_rx,
        RepoWatchUnavailable::WatchRefused,
        "an OS-refused watch",
    );
    assert_eq!(
        refusing.entry_count(),
        0,
        "a refused watch leaves no entry behind"
    );
    assert_eq!(refusing.shared_count(), 0);

    // 4. a RUNTIME backend failure — the watch died after construction. It must
    // reach the UI as a named discriminant, never only the log.
    let probe_repo = primary_repo();
    let probe_id = RepoIdentity::resolve(probe_repo.path()).expect("identity resolves");
    let (backend, taps) = ProbeBackend::paired();
    let (probed, probe_rx) = registry_with(backend);
    let token = probed
        .subscribe(probe_repo.path())
        .expect("subscribe")
        .token;
    assert!(
        collected(&probe_rx).is_empty(),
        "a healthy watch says nothing"
    );

    assert_eq!(
        fire_worktree(&taps, WatchSignal::Failed("inotify watch died".into())),
        1
    );
    assert_eq!(
        collected(&probe_rx),
        vec![SinkEvent::Unavailable(
            probe_id.id.clone(),
            RepoWatchUnavailable::BackendFailed
        )],
        "a runtime backend error is reported, not swallowed"
    );

    assert_eq!(
        fire_shared(&taps, WatchSignal::Failed("refs watch died".into())),
        1
    );
    assert_eq!(
        collected(&probe_rx),
        vec![SinkEvent::Unavailable(
            probe_id.id.clone(),
            RepoWatchUnavailable::BackendFailed
        )],
        "the shared half reports for every member identity"
    );
    assert_eq!(
        probed.entry_count(),
        1,
        "the failure is reported, and no polling fallback replaces the watch"
    );
    probed.release(&token);

    // 2. a linked worktree deleted while watched.
    let holder = linked_worktree(repo.path(), "doomed");
    let lane = holder.path().join("doomed");
    let (live, live_rx) = live_registry();
    let sub = live.subscribe(&lane).expect("lane subscribe");
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

#[test]
fn refusal_sink_can_reenter_registry_after_both_watch_failures() {
    use std::sync::{mpsc, Weak};

    struct ReentrantSink {
        registry: parking_lot::Mutex<Weak<crate::RepoWatchRegistry>>,
    }

    impl crate::RepoEventSink for ReentrantSink {
        fn repository_changed(&self, _identity: &str) {}

        fn repository_unavailable(&self, _identity: &str, _reason: RepoWatchUnavailable) {
            let registry = self.registry.lock().upgrade().unwrap();
            assert_eq!(registry.entry_count(), 0);
            assert_eq!(registry.shared_count(), 0);
        }
    }

    struct FailAt {
        call: std::sync::atomic::AtomicUsize,
        fail_at: usize,
    }

    impl crate::WatchBackend for FailAt {
        fn build(
            &self,
            specs: &[crate::WatchSpec],
            handler: crate::BatchHandler,
        ) -> Result<Box<dyn crate::ActiveWatch>, String> {
            if self.call.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == self.fail_at {
                Err("injected refusal".into())
            } else {
                InertBackend.build(specs, handler)
            }
        }
    }

    for fail_at in [0, 1] {
        let repo = primary_repo();
        let sink = Arc::new(ReentrantSink {
            registry: parking_lot::Mutex::new(Weak::new()),
        });
        let registry = Arc::new(crate::RepoWatchRegistry::with_backend(
            sink.clone(),
            Arc::new(FailAt {
                call: std::sync::atomic::AtomicUsize::new(0),
                fail_at,
            }),
        ));
        *sink.registry.lock() = Arc::downgrade(&registry);
        let (tx, rx) = mpsc::channel();
        let join = std::thread::spawn(move || {
            let result = registry.subscribe(repo.path());
            tx.send(result.unwrap_err().reason).unwrap();
        });
        assert_eq!(
            await_reentry_result(&rx),
            RepoWatchUnavailable::WatchRefused
        );
        join.join().unwrap();
    }
}
