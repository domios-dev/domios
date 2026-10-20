use super::*;

fn audit(decision: PermissionAutoConfirmDecision, n: usize) -> LifecycleTransition {
    LifecycleTransition::PermissionAutoConfirm {
        decision,
        tool_name: Some(format!("tool-{n}")),
        option_label: None,
        rule: None,
        question: None,
        prompt_fingerprint: Some(format!("fp-{n}")),
        session_id: None,
        reason: None,
    }
}

fn audit_fingerprints(load: &JournalLoad) -> Vec<String> {
    load.records
        .iter()
        .filter_map(|record| match record {
            JournalRecord::Transition { entry } => match &entry.transition {
                LifecycleTransition::PermissionAutoConfirm {
                    prompt_fingerprint, ..
                } => prompt_fingerprint.clone(),
                _ => None,
            },
            JournalRecord::Checkpoint { .. } => None,
        })
        .collect()
}

#[test]
fn permission_audits_never_change_checkpoints() {
    let (journal, _dir) = new_journal();
    let op = key("tab-a", 1, "op-a");
    commit_full_launch(&journal, &op, "session-a");
    let before = checkpoints(&journal.load());
    assert!(!before.is_empty());

    for (n, decision) in [
        PermissionAutoConfirmDecision::Enabled,
        PermissionAutoConfirmDecision::Confirmed,
        PermissionAutoConfirmDecision::Refused,
        PermissionAutoConfirmDecision::Disabled,
    ]
    .into_iter()
    .enumerate()
    {
        journal.append(op.clone(), audit(decision, n)).unwrap();
    }

    let load = journal.load();
    assert_eq!(checkpoints(&load), before);
    assert_eq!(audit_fingerprints(&load).len(), 4);
}

#[test]
fn compaction_keeps_only_the_most_recent_permission_audits() {
    let (journal, _dir) = new_journal();
    let op = key("tab-a", 1, "op-a");
    commit_full_launch(&journal, &op, "session-a");
    let extra = 5;
    for n in 0..PERMISSION_AUTO_CONFIRM_AUDIT_LIMIT + extra {
        journal
            .append(
                op.clone(),
                audit(PermissionAutoConfirmDecision::Confirmed, n),
            )
            .unwrap();
    }
    let before = checkpoints(&journal.load());

    journal.compact().unwrap();
    let load = journal.load();

    let kept = audit_fingerprints(&load);
    assert_eq!(kept.len(), PERMISSION_AUTO_CONFIRM_AUDIT_LIMIT);
    assert_eq!(kept.first(), Some(&format!("fp-{extra}")));
    assert_eq!(
        kept.last(),
        Some(&format!(
            "fp-{}",
            PERMISSION_AUTO_CONFIRM_AUDIT_LIMIT + extra - 1
        ))
    );
    assert_eq!(checkpoints(&load), before);
}
