use super::*;

#[test]
fn runtime_exit_diagnostics_survive_compaction_without_granting_authority() {
    let (journal, _dir) = new_journal();
    let transition = LifecycleTransition::RuntimeExited {
        diagnostics: "code=1 stderr=opaque after_compaction=true".into(),
    };
    journal
        .append(key("runtime-tab", 1, "runtime-exit"), transition.clone())
        .unwrap();
    assert!(checkpoints(&journal.load()).is_empty());
    journal.compact().unwrap();
    let records = journal.load().records;
    assert!(records.iter().any(|record| matches!(record, JournalRecord::Transition { entry } if entry.transition == transition)));
    assert!(checkpoints(&journal.load()).is_empty());
}
