use super::*;

fn key() -> LifecycleKey {
    LifecycleKey::new("own-tab", LifecycleEpoch(1), "own-operation")
}

#[test]
fn indexed_conversation_hit_does_not_wait_for_the_journal_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let journal = TabLifecycleJournal::for_dir(dir.path().into());
    journal
        .note_conversation_started(key(), "own-session")
        .unwrap();
    let observed = std::thread::scope(|scope| {
        let guard = journal.cursor.lock();
        let (done, received) = std::sync::mpsc::channel();
        let writer = &journal;
        let child = scope.spawn(move || {
            let result = writer.note_conversation_started(key(), "own-session");
            done.send(result).unwrap();
        });
        let observed = received.recv_timeout(std::time::Duration::from_secs(1));
        drop(guard);
        child.join().unwrap();
        observed
    });
    observed
        .expect("indexed hit must complete while cursor is held")
        .unwrap();
    assert_eq!(journal.load().records.len(), 1);
}

#[test]
fn concurrent_conversation_misses_append_one_record() {
    let dir = tempfile::tempdir().unwrap();
    let journal = TabLifecycleJournal::for_dir(dir.path().into());
    let start = std::sync::Barrier::new(3);
    std::thread::scope(|scope| {
        let mut children = Vec::new();
        for _ in 0..2 {
            let journal = &journal;
            let start = &start;
            children.push(scope.spawn(move || {
                start.wait();
                journal
                    .note_conversation_started(key(), "own-session")
                    .unwrap();
            }));
        }
        start.wait();
        for child in children {
            child.join().unwrap();
        }
    });
    assert_eq!(journal.load().records.len(), 1);
}

#[test]
fn published_restore_index_keeps_the_latest_own_record_after_compaction_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let journal = TabLifecycleJournal::for_dir(dir.path().into());
    for session in ["old", "new"] {
        journal
            .append(
                key(),
                LifecycleTransition::EmptySessionReplaced {
                    session_id: session.into(),
                },
            )
            .unwrap();
    }
    journal.note_conversation_started(key(), "new").unwrap();
    journal.compact().unwrap();
    let reopened = TabLifecycleJournal::for_dir(dir.path().into());
    std::fs::rename(reopened.path(), dir.path().join("detached.jsonl")).unwrap();
    let _guard = reopened.cursor.lock();
    assert_eq!(
        reopened.latest_empty_session_restore("own-tab").as_deref(),
        Some("new")
    );
    assert_eq!(reopened.latest_empty_session_restore("other"), None);
    assert!(reopened
        .read_index
        .load()
        .has_conversation("own-tab", "new"));
    assert!(!reopened.read_index.load().has_conversation("other", "new"));
}

#[test]
fn failed_appends_never_publish_restore_or_conversation_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let journal = TabLifecycleJournal::for_dir(dir.path().into());
    std::fs::create_dir(journal.path()).unwrap();
    assert!(journal
        .append(
            key(),
            LifecycleTransition::EmptySessionReplaced {
                session_id: "missing".into()
            }
        )
        .is_err());
    assert!(journal.note_conversation_started(key(), "missing").is_err());
    assert_eq!(journal.latest_empty_session_restore("own-tab"), None);
    assert!(!journal
        .read_index
        .load()
        .has_conversation("own-tab", "missing"));
    std::fs::remove_dir(journal.path()).unwrap();
    journal.note_conversation_started(key(), "missing").unwrap();
    assert_eq!(journal.load().records.len(), 1);
}
