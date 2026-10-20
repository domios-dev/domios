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
