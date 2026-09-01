use super::support::*;
use crate::repowatch::backend::WatchSignal;
use crate::repowatch::identity::RepoIdentity;

/// The shared-ref fanout must follow the identity's own watcher, not a single
/// token: a partial release used to silence every remaining subscriber.
#[test]
fn repository_shared_ref_fanout_survives_partial_release() {
    if !git_available() {
        eprintln!("skipping: git is not on PATH");
        return;
    }
    let repo = primary_repo();
    let identity = RepoIdentity::resolve(repo.path()).expect("identity resolves");
    let (backend, taps) = ProbeBackend::paired();
    let (registry, rx) = registry_with(backend);

    let first = registry
        .subscribe(repo.path())
        .expect("first subscribe")
        .token;
    let second = registry
        .subscribe(repo.path())
        .expect("second subscribe")
        .token;
    assert_eq!(registry.entry_count(), 1, "two subscriptions, one identity");
    assert_eq!(registry.shared_refcount(&identity.common_key), 2);
    assert_eq!(
        registry.shared_member_count(&identity.common_key),
        1,
        "one identity is one fanout member"
    );

    assert!(registry.release(&first), "partial release accepted");
    assert_eq!(
        registry.refcount(&identity.key),
        1,
        "one subscription remains"
    );
    assert_eq!(
        registry.shared_member_count(&identity.common_key),
        1,
        "a partial release must not drop the fanout member"
    );

    let loose_ref = identity.common_dir.join("refs").join("heads").join("main");
    let fired = fire_shared(&taps, WatchSignal::Batch(vec![loose_ref.clone()]));
    assert_eq!(fired, 1, "exactly one shared-ref watch exists");
    let events = collected(&rx);
    assert_eq!(
        events,
        vec![SinkEvent::Changed(identity.id.clone())],
        "the surviving subscription still receives ref events"
    );

    assert!(registry.release(&second), "final release accepted");
    assert_eq!(registry.entry_count(), 0);
    assert_eq!(registry.shared_count(), 0);
    assert_eq!(registry.shared_member_count(&identity.common_key), 0);

    fire_shared(&taps, WatchSignal::Batch(vec![loose_ref]));
    assert!(
        collected(&rx).is_empty(),
        "after the last release the fanout is empty"
    );
}
