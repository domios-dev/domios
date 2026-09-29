use super::*;

fn duplicate_seq_journal() -> (TabLifecycleJournal, tempfile::TempDir) {
    let (journal, dir) = new_journal();
    let key = key("tab-dup", 1, "op-dup");
    journal
        .append(key.clone(), LifecycleTransition::EpochOpened)
        .unwrap();
    journal
        .append(
            key.clone(),
            LifecycleTransition::ProviderIdentityCommitted {
                provider: "claude".into(),
            },
        )
        .unwrap();
    let body = fs::read_to_string(journal.path()).unwrap();
    let last = body.lines().last().unwrap().to_string();
    fs::write(journal.path(), format!("{body}{last}\n")).unwrap();
    (journal, dir)
}

#[test]
fn lifecycle_journal_compacts_with_duplicate_seq() {
    let (journal, _dir) = duplicate_seq_journal();

    let load = journal.load();
    assert!(load.tail.is_intact());
    assert_eq!(load.duplicate_seqs, 1);
    assert_eq!(load.records.len(), 2);

    journal.compact().expect("duplicate seq is repaired, not refused");

    let repaired = journal.load();
    assert_eq!(repaired.duplicate_seqs, 0);
    assert!(repaired.tail.is_intact());
    let seqs: Vec<_> = repaired.records.iter().map(JournalRecord::seq).collect();
    assert!(seqs.windows(2).all(|pair| pair[0] < pair[1]));
}
