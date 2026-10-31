use super::*;
use parking_lot::Mutex;

#[tokio::test]
async fn admitted_storage_finishes_before_retirement_and_obsolete_queued_effects_do_nothing() {
    let owner = Coordinator::default();
    let effects = Arc::new(Mutex::new(Vec::new()));
    let (started, running) = oneshot::channel();
    let (release, held) = oneshot::channel();
    let log = effects.clone();
    let current = owner.submit(Kind::Projection, move || async move {
        started.send(()).unwrap();
        held.await.unwrap();
        log.lock().push("storage committed");
        Ok(())
    }).unwrap();
    running.await.unwrap();
    let obsolete = owner.submit(Kind::Projection, || async {
        panic!("A queued candidate retired before admission has no side effects")
    }).unwrap();
    let log = effects.clone();
    let retirement = owner.submit(Kind::Retirement, move || async move {
        log.lock().push("retired");
        Ok(())
    }).unwrap();
    assert_eq!(owner.outcome(), (3, true, true, None));
    assert!(effects.lock().is_empty());
    release.send(()).unwrap();
    assert_eq!(current.wait().await, Ok(()));
    assert_eq!(obsolete.wait().await, Err::<(), _>(Error::Revoked));
    assert_eq!(retirement.wait().await, Ok(()));
    assert_eq!(*effects.lock(), ["storage committed", "retired"]);
    assert_eq!(owner.outcome(), (0, false, false, None));
}

#[tokio::test]
async fn abandoning_admitted_and_queued_waiters_neither_aborts_nor_replays_work() {
    let owner = Coordinator::default();
    let writes = Arc::new(Mutex::new(0));
    let (started, running) = oneshot::channel();
    let (release, held) = oneshot::channel();
    let log = writes.clone();
    let current = owner.submit(Kind::Projection, move || async move {
        started.send(()).unwrap();
        held.await.unwrap();
        *log.lock() += 1;
        Ok(())
    }).unwrap();
    running.await.unwrap();
    let abandoned: Receipt<()> = owner.submit(Kind::Projection, || async {
        panic!("An abandoned unadmitted candidate must be discarded")
    }).unwrap();
    drop(current);
    drop(abandoned);
    release.send(()).unwrap();
    owner.idle().await;
    assert_eq!(*writes.lock(), 1);
    assert_eq!(owner.outcome(), (0, false, false, None));
}

#[tokio::test]
async fn newer_candidate_supersedes_queued_version_but_cannot_interrupt_admitted_lifecycle_close() {
    let owner = Coordinator::default();
    let versions = Arc::new(Mutex::new(Vec::new()));
    let (started, running) = oneshot::channel();
    let (release, held) = oneshot::channel();
    let log = versions.clone();
    let first = owner.submit(Kind::Projection, move || async move {
        started.send(()).unwrap();
        held.await.unwrap();
        log.lock().push(1);
        Ok(())
    }).unwrap();
    running.await.unwrap();
    let second = owner.submit(Kind::Projection, || async {
        panic!("Superseded queued version cannot close tabs or write state")
    }).unwrap();
    let log = versions.clone();
    let third = owner.submit(Kind::Projection, move || async move {
        log.lock().push(3);
        Ok(())
    }).unwrap();
    release.send(()).unwrap();
    first.wait().await.unwrap();
    assert_eq!(second.wait().await, Err::<(), _>(Error::Revoked));
    third.wait().await.unwrap();
    assert_eq!(*versions.lock(), [1, 3]);
}

#[tokio::test]
async fn admission_is_bounded_and_releases_abandoned_slots_without_retry() {
    let owner = Coordinator::default();
    let (started, running) = oneshot::channel();
    let (release, held) = oneshot::channel();
    let first = owner.submit(Kind::Projection, move || async move {
        started.send(()).unwrap();
        held.await.unwrap();
        Ok(())
    }).unwrap();
    running.await.unwrap();
    for _ in 1..CAPACITY - 1 {
        drop(owner.submit(Kind::Projection, || async { Ok(()) }).unwrap());
    }
    assert!(matches!(owner.submit(Kind::Projection, || async { Ok(()) }), Err(Error::Transport)));
    let retirement = owner.submit(Kind::Retirement, || async { Ok(()) }).unwrap();
    assert!(matches!(owner.submit(Kind::Retirement, || async { Ok(()) }), Err(Error::Transport)));
    release.send(()).unwrap();
    first.wait().await.unwrap();
    retirement.wait().await.unwrap();
    owner.idle().await;
    assert_eq!(owner.outcome().0, 0);
}

#[tokio::test]
async fn failed_off_cleanup_is_retained_and_blocks_successor_until_explicit_cleanup_success() {
    let owner = Coordinator::default();
    owner.stop_pending();
    let failed = owner.submit(Kind::StopLocal, || async { Err::<(), _>(Error::CursorStore) }).unwrap();
    assert_eq!(failed.wait().await, Err(Error::CursorStore));
    assert_eq!(owner.outcome(), (0, false, true, Some(Error::CursorStore)));
    assert!(matches!(owner.submit(Kind::Selection, || async { Ok(()) }), Err(Error::CursorStore)));
    owner.submit(Kind::StopLocal, || async { Ok(()) }).unwrap().wait().await.unwrap();
    owner.submit(Kind::Selection, || async { Ok(()) }).unwrap().wait().await.unwrap();
    assert_eq!(owner.outcome(), (0, false, false, None));
}

#[tokio::test]
async fn failed_current_storage_does_not_commit_dedupe_and_same_version_recovery_is_owned() {
    let owner = Coordinator::default();
    let persisted = Arc::new(Mutex::new(false));
    let notices = Arc::new(Mutex::new(0));
    assert_eq!(owner.submit(Kind::Projection, || async { Err::<(), _>(Error::CursorStore) })
        .unwrap().wait().await, Err(Error::CursorStore));
    for _recovery_or_duplicate in 0..2 {
        let persisted = persisted.clone();
        let notices = notices.clone();
        owner.submit(Kind::Projection, move || async move {
            if !*persisted.lock() {
                *persisted.lock() = true;
                *notices.lock() += 1;
            }
            Ok(())
        }).unwrap().wait().await.unwrap();
    }
    assert_eq!(*notices.lock(), 1);
}
