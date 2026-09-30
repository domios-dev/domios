use super::*;

fn bind_after_readiness(journal: &TabLifecycleJournal, key: &LifecycleKey, session_id: &str) {
    for transition in [
        LifecycleTransition::EpochOpened,
        LifecycleTransition::ProviderIdentityCommitted {
            provider: "provider-a".into(),
        },
        LifecycleTransition::LaunchReady,
        LifecycleTransition::SessionBound {
            session_id: session_id.into(),
            binding_nonce: format!("nonce-{session_id}"),
            source: Default::default(),
        },
    ] {
        journal.append(key.clone(), transition).unwrap();
    }
}

fn strictly_increasing(records: &[JournalRecord]) -> bool {
    records.windows(2).all(|pair| pair[0].seq() < pair[1].seq())
}

#[test]
fn compaction_never_duplicates_a_binding_that_sealed_the_checkpoint() {
    let (journal, _dir) = new_journal();
    bind_after_readiness(&journal, &key("tab-a", 1, "op-a"), "session-a");
    bind_after_readiness(&journal, &key("tab-b", 1, "op-b"), "session-b");
    let before = checkpoints(&journal.load());

    journal.compact().unwrap();
    let once = journal.load();
    journal.compact().unwrap();
    let twice = journal.load();

    for load in [&once, &twice] {
        assert!(load.tail.is_intact(), "compacted journal must replay whole");
        assert!(strictly_increasing(&load.records));
        assert_eq!(checkpoints(load), before);
    }
}

#[test]
fn appends_after_compaction_stay_visible_across_reopen() {
    let (journal, dir) = new_journal();
    bind_after_readiness(&journal, &key("tab-a", 1, "op-a"), "session-a");
    journal.compact().unwrap();
    drop(journal);

    let reopened = TabLifecycleJournal::for_dir(dir.path().to_path_buf());
    bind_after_readiness(&reopened, &key("tab-a", 2, "op-a2"), "session-a2");
    let load = reopened.load();

    assert!(load.tail.is_intact());
    assert!(load.records.iter().any(|record| matches!(
        record,
        JournalRecord::Transition { entry } if matches!(
            &entry.transition,
            LifecycleTransition::SessionBound { session_id, .. } if session_id == "session-a2"
        )
    )));
}

#[test]
fn refused_tail_is_quarantined_on_open_so_new_appends_replay() {
    let (journal, dir) = new_journal();
    bind_after_readiness(&journal, &key("tab-a", 1, "op-a"), "session-a");
    let path = dir.path().join(JOURNAL_FILE_NAME);
    let body = fs::read_to_string(&path).unwrap();
    let first = body.lines().next().unwrap().to_string();
    fs::write(&path, format!("{body}{first}\n")).unwrap();
    assert!(!journal.load().tail.is_intact());
    drop(journal);

    let reopened = TabLifecycleJournal::for_dir(dir.path().to_path_buf());
    bind_after_readiness(&reopened, &key("tab-a", 2, "op-a2"), "session-a2");
    let load = reopened.load();

    assert!(load.tail.is_intact());
    assert!(strictly_increasing(&load.records));
    assert_eq!(
        fs::read_to_string(refused_tail_path(&path)).unwrap(),
        format!("{first}\n"),
        "the refused tail is kept verbatim, never replayed"
    );
    let sessions: Vec<_> = checkpoints(&load)
        .into_iter()
        .map(|checkpoint| checkpoint.authority.session_id)
        .collect();
    assert_eq!(sessions, vec!["session-a2".to_string()]);
}

#[test]
fn compaction_quarantines_a_refused_tail_instead_of_failing_forever() {
    let (journal, dir) = new_journal();
    bind_after_readiness(&journal, &key("tab-a", 1, "op-a"), "session-a");
    let path = dir.path().join(JOURNAL_FILE_NAME);
    let mut body = fs::read_to_string(&path).unwrap();
    body.push_str("{not json\n");
    fs::write(&path, body).unwrap();

    journal.compact().unwrap();
    bind_after_readiness(&journal, &key("tab-a", 2, "op-a2"), "session-a2");

    let load = journal.load();
    assert!(load.tail.is_intact());
    assert!(fs::read_to_string(refused_tail_path(&path))
        .unwrap()
        .contains("{not json"));
}

#[test]
fn rolled_back_binding_keeps_its_proof_through_later_compactions() {
    let (journal, _dir) = new_journal();
    let launch = key("tab-a", 1, "op-a");
    bind_after_readiness(&journal, &launch, "session-a");
    journal.compact().unwrap();
    journal
        .append(
            launch,
            LifecycleTransition::RolledBack {
                reason: "provider exited".into(),
            },
        )
        .unwrap();
    journal.compact().unwrap();
    journal.compact().unwrap();

    let load = journal.load();
    assert!(load.tail.is_intact());
    assert!(strictly_increasing(&load.records));
    assert!(
        checkpoints(&load).is_empty(),
        "a rolled-back operation never becomes restorable authority"
    );
    assert!(load.records.iter().any(|record| matches!(
        record,
        JournalRecord::Checkpoint { checkpoint }
            if checkpoint.authority.session_id == "session-a"
    )));
}
