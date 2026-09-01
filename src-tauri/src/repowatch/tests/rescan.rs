use super::support::*;
use crate::repowatch::backend::{
    is_observation_only, is_rescan, RescanRelay, RescanSink, WatchSignal,
};
use crate::repowatch::events::RepoWatchUnavailable;
use crate::repowatch::identity::RepoIdentity;
use notify::event::{AccessKind, AccessMode, Flag};
use notify::{Event, EventHandler, EventKind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// A rescan means events were lost. It must fail closed and loudly — never a
/// rescan, retry, poll, or synthesized change.
#[test]
fn repository_watch_reports_backend_rescan() {
    if !git_available() {
        eprintln!("skipping: git is not on PATH");
        return;
    }

    // The exact shape Linux emits on queue overflow: zero paths, Rescan flag.
    let overflow = Event::new(EventKind::Other).set_flag(Flag::Rescan);
    assert!(
        overflow.paths.is_empty(),
        "the overflow signal carries no paths, which is why the debouncer drops it"
    );
    assert!(
        is_rescan(&Ok(overflow.clone())),
        "the raw shape is recognised"
    );
    assert!(
        !is_rescan(&Ok(Event::new(EventKind::Other))),
        "an ordinary event is not a rescan"
    );

    // The relay lifts it out upstream of the debouncer that would destroy it.
    let lifted = Arc::new(AtomicUsize::new(0));
    let counter = lifted.clone();
    let sink: RescanSink = Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    let forwarded = Arc::new(AtomicUsize::new(0));
    let downstream = forwarded.clone();
    let mut relay = RescanRelay::new(
        move |_: notify::Result<Event>| {
            downstream.fetch_add(1, Ordering::SeqCst);
        },
        Some(sink),
    );
    relay.handle_event(Ok(overflow));
    assert_eq!(
        lifted.load(Ordering::SeqCst),
        1,
        "the rescan reached the sink"
    );
    assert_eq!(
        forwarded.load(Ordering::SeqCst),
        1,
        "and the event still reaches the debouncer untouched"
    );
    relay.handle_event(Ok(Event::new(EventKind::Other)));
    assert_eq!(
        lifted.load(Ordering::SeqCst),
        1,
        "an ordinary event raises no rescan"
    );
    assert_eq!(forwarded.load(Ordering::SeqCst), 2);

    let opened_for_read = Event::new(EventKind::Access(AccessKind::Open(AccessMode::Read)));
    assert!(is_observation_only(&Ok(opened_for_read.clone())));
    relay.handle_event(Ok(opened_for_read));
    assert_eq!(
        forwarded.load(Ordering::SeqCst),
        2,
        "reading a watched directory is not a repository change"
    );
    relay.handle_event(Ok(Event::new(EventKind::Modify(
        notify::event::ModifyKind::Any,
    ))));
    assert_eq!(
        forwarded.load(Ordering::SeqCst),
        3,
        "mutations still reach the debouncer"
    );

    // End to end: the signal reaches every affected identity as unavailable.
    let repo = primary_repo();
    let identity = RepoIdentity::resolve(repo.path()).expect("identity resolves");
    let (backend, taps) = ProbeBackend::paired();
    let (registry, rx) = registry_with(backend);
    let token = registry.subscribe(repo.path()).expect("subscribe").token;

    assert_eq!(fire_worktree(&taps, WatchSignal::Rescan), 1);
    assert_eq!(
        collected(&rx),
        vec![SinkEvent::Unavailable(
            identity.id.clone(),
            RepoWatchUnavailable::RescanRequired
        )],
        "a worktree rescan is reported, never swallowed"
    );

    assert_eq!(fire_shared(&taps, WatchSignal::Rescan), 1);
    assert_eq!(
        collected(&rx),
        vec![SinkEvent::Unavailable(
            identity.id.clone(),
            RepoWatchUnavailable::RescanRequired
        )],
        "the shared half reports for every member identity"
    );
    assert_eq!(
        registry.entry_count(),
        1,
        "the watch is reported untrusted, and nothing polls or rescans in its place"
    );
    registry.release(&token);
}
