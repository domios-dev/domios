use super::*;

fn key() -> LifecycleKey {
    LifecycleKey::new("own-tab", LifecycleEpoch(1), "own-operation")
}

fn retained_history(count: usize) -> Vec<JournalRecord> {
    (1..=count)
        .map(|seq| JournalRecord::Transition {
            entry: LifecycleEntry {
                seq: seq as JournalSeq,
                key: key(),
                transition: LifecycleTransition::EpochOpened,
                recorded_at: super::super::stamp::now_utc(),
            },
        })
        .collect()
}

#[test]
fn opening_retained_history_allows_growth_before_automatic_compaction() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(JOURNAL_FILE_NAME);
    let records = retained_history(COMPACT_AT_RECORDS + 1);
    write_all_records(&path, &records).unwrap();
    let journal = TabLifecycleJournal::for_dir(dir.path().into());
    let seq = journal
        .append(key(), LifecycleTransition::LaunchReady)
        .unwrap();
    assert_eq!(seq, records.len() as JournalSeq + 1);
    let cursor = journal.cursor.lock();
    assert_eq!(
        cursor.compact_at_records,
        records.len() + COMPACT_AT_RECORDS
    );
    drop(cursor);
    assert_eq!(journal.load().records.len(), records.len() + 1);
    let reopened = TabLifecycleJournal::for_dir(dir.path().into());
    assert_eq!(reopened.load().last_seq(), seq);
}

#[test]
fn automatic_compaction_rechecks_growth_and_preserves_uncovered_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(JOURNAL_FILE_NAME);
    let records = retained_history(COMPACT_AT_RECORDS + 1);
    write_all_records(&path, &records).unwrap();
    let journal = TabLifecycleJournal::for_dir(dir.path().into());
    journal.cursor.lock().compact_at_records = records.len();
    assert_eq!(journal.compact_if_due().unwrap(), records.len());
    assert_eq!(journal.load().records.len(), records.len());
    assert_eq!(journal.load().last_seq(), records.len() as JournalSeq);
    assert_eq!(
        journal.cursor.lock().compact_at_records,
        records.len() + COMPACT_AT_RECORDS
    );
    // A queued request must skip without touching the journal after the first rewrite.
    fs::rename(&path, dir.path().join("detached.jsonl")).unwrap();
    assert_eq!(journal.compact_if_due().unwrap(), records.len());
    assert!(!path.exists());
}

#[test]
fn explicit_compaction_runs_below_the_automatic_budget() {
    let dir = tempfile::tempdir().unwrap();
    let journal = TabLifecycleJournal::for_dir(dir.path().into());
    journal
        .append(key(), LifecycleTransition::EpochOpened)
        .unwrap();
    assert_eq!(journal.compact().unwrap(), 1);
    assert_eq!(
        journal.cursor.lock().compact_at_records,
        COMPACT_AT_RECORDS + 1
    );
}

#[test]
fn prepared_compaction_refuses_concurrent_append_without_losing_history() {
    let dir = tempfile::tempdir().unwrap();
    let journal = TabLifecycleJournal::for_dir(dir.path().into());
    journal
        .append(key(), LifecycleTransition::EpochOpened)
        .unwrap();
    journal.cursor.lock().compact_at_records = 1;
    let prepared = dir.path().join("prepared.jsonl");
    write_all_records(&prepared, &journal.load().records).unwrap();
    // Model the append that races preparation without running its automatic compaction yet.
    journal.cursor.lock().compact_at_records = usize::MAX;
    journal
        .append(key(), LifecycleTransition::LaunchReady)
        .unwrap();
    journal.cursor.lock().compact_at_records = 1;
    let error = journal
        .commit_compaction(&prepared, 1, 1, 1, None)
        .unwrap_err();
    assert!(error.contains("compaction conflict"), "{error}");
    let history = journal.load();
    assert!(history.tail.is_intact());
    assert_eq!(history.records.len(), 2);
    assert_eq!(history.last_seq(), 2);
    assert_eq!(journal.cursor.lock().compact_at_records, 1);
    journal
        .append(key(), LifecycleTransition::EpochOpened)
        .unwrap();
    let history = journal.load();
    assert!(history.tail.is_intact());
    assert_eq!(history.records.len(), 3);
    assert_eq!(history.last_seq(), 3);
    assert_eq!(
        journal.cursor.lock().compact_at_records,
        3 + COMPACT_AT_RECORDS
    );
}

#[test]
fn held_file_writer_does_not_block_epoch_or_index_reads() {
    let dir = tempfile::tempdir().unwrap();
    let journal = TabLifecycleJournal::for_dir(dir.path().into());
    journal
        .note_conversation_started(key(), "own-session")
        .unwrap();
    let observed = std::thread::scope(|scope| {
        let writer = journal.writer.lock();
        let (done, received) = std::sync::mpsc::channel();
        let journal = &journal;
        let child = scope.spawn(move || {
            let epoch = journal.next_epoch_for_tab("own-tab");
            journal
                .note_conversation_started(key(), "own-session")
                .unwrap();
            done.send(epoch).unwrap();
        });
        let result = received.recv_timeout(std::time::Duration::from_secs(1));
        drop(writer);
        child.join().unwrap();
        result
    });
    assert_eq!(
        observed.expect("epoch/index reads must not wait on file work"),
        LifecycleEpoch(2)
    );
    assert_eq!(journal.load().records.len(), 1);
}

#[test]
fn stale_compaction_cannot_publish_quarantine_or_overwrite_a_concurrent_append() {
    let dir = tempfile::tempdir().unwrap();
    let journal = TabLifecycleJournal::for_dir(dir.path().into());
    journal
        .append(key(), LifecycleTransition::EpochOpened)
        .unwrap();
    OpenOptions::new()
        .append(true)
        .open(journal.path())
        .unwrap()
        .write_all(b"{not json\n")
        .unwrap();
    let load = journal.load();
    let (quarantine, original_len) = prepare_refused_tail(journal.path(), &load)
        .unwrap()
        .unwrap();
    let prepared = dir.path().join("prepared.jsonl");
    write_all_records(&prepared, &load.records).unwrap();
    journal
        .append(key(), LifecycleTransition::LaunchReady)
        .unwrap();
    let before = fs::read(journal.path()).unwrap();
    let error = journal
        .commit_compaction(&prepared, 1, 1, 1, Some((&quarantine, original_len)))
        .unwrap_err();
    assert!(error.contains("compaction conflict"), "{error}");
    assert_eq!(fs::read(journal.path()).unwrap(), before);
    assert!(!refused_tail_path(journal.path()).exists());
}
