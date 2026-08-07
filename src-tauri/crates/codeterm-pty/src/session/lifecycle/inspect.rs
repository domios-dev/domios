use serde_json::{json, Value};

use crate::multiplexer::types::Tab;

use super::journal::{checkpoints, JournalLoad, TabLifecycleJournal, TailStatus};
use super::record::{JournalRecord, LifecycleEpoch};
use super::recovery::{authorize, RecoveryVerdict, SnapshotAuthorityClaim};

pub fn inspect_tab_lifecycle(tab: &Tab) -> Value {
    let claim = SnapshotAuthorityClaim {
        tab_id: tab.id.clone(),
        epoch: tab.lifecycle_epoch.map(LifecycleEpoch),
        operation_id: tab.lifecycle_operation_id.clone(),
        provider: tab.agent_type.clone(),
        chat_backend: tab.is_chat_backend(),
        session_id: tab.agent_session_id.clone(),
        binding_nonce: tab.reported_session_binding_nonce.clone(),
        role: tab.role.map(|role| role.label().to_string()),
        role_profile: tab.role_profile.clone(),
        team_id: tab.team_id.clone(),
        parent_tab_id: tab.parent_tab_id.clone(),
        can_spawn: tab.can_spawn,
        depth: tab.depth,
    };
    inspect_claim(&claim, &TabLifecycleJournal::shared().load())
}

fn inspect_claim(claim: &SnapshotAuthorityClaim, load: &JournalLoad) -> Value {
    let checkpoint = checkpoints(load)
        .into_iter()
        .find(|checkpoint| checkpoint.key.tab_id == claim.tab_id);
    let tail_refusal = match &load.tail {
        TailStatus::Intact => None,
        TailStatus::Refused { line, reason } => Some(format!("line {line}: {reason}")),
    };
    let transitions = load
        .records
        .iter()
        .filter_map(|record| match record {
            JournalRecord::Transition { entry } if entry.key.tab_id == claim.tab_id => Some(entry),
            _ => None,
        })
        .collect::<Vec<_>>();
    let lineage = match (claim.epoch, claim.operation_id.as_deref()) {
        (Some(epoch), Some(operation_id)) => json!({
            "epoch": epoch,
            "operation_id": operation_id,
        }),
        _ => Value::Null,
    };
    let verdict = verdict_json(authorize(
        claim,
        checkpoint.as_ref(),
        tail_refusal.as_deref(),
    ));
    let tail = match &load.tail {
        TailStatus::Intact => json!({ "status": "intact" }),
        TailStatus::Refused { line, reason } => json!({
            "status": "refused",
            "line": line,
            "reason": reason,
        }),
    };
    json!({
        "lineage": lineage,
        "transitions": transitions,
        "checkpoint": checkpoint,
        "verdict": verdict,
        "journal_tail": tail,
    })
}

fn verdict_json(verdict: RecoveryVerdict) -> Value {
    match verdict {
        RecoveryVerdict::NoAuthorityClaimed => json!({ "status": "no_authority_claimed" }),
        RecoveryVerdict::PreJournal => json!({ "status": "pre_journal" }),
        RecoveryVerdict::Restored(authority) => json!({
            "status": "restored",
            "authority": authority,
        }),
        RecoveryVerdict::Stripped(refusal) => json!({
            "status": "stripped",
            "reason": refusal.reason(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::lifecycle::{LifecycleEntry, LifecycleKey, LifecycleTransition};

    #[test]
    fn inspection_is_scoped_and_reports_the_live_recovery_verdict() {
        let claim = SnapshotAuthorityClaim {
            tab_id: "target".into(),
            epoch: Some(LifecycleEpoch(1)),
            operation_id: Some("op-1".into()),
            provider: Some("claude".into()),
            session_id: Some("session-1".into()),
            binding_nonce: Some("nonce-1".into()),
            ..Default::default()
        };
        let transitions = [
            LifecycleTransition::EpochOpened,
            LifecycleTransition::ProviderIdentityCommitted {
                provider: "claude".into(),
            },
            LifecycleTransition::SessionBound {
                session_id: "session-1".into(),
                binding_nonce: "nonce-1".into(),
            },
            LifecycleTransition::LaunchReady,
        ];
        let mut records = transitions
            .into_iter()
            .enumerate()
            .map(|(index, transition)| JournalRecord::Transition {
                entry: LifecycleEntry {
                    seq: index as u64 + 1,
                    key: LifecycleKey::new("target", LifecycleEpoch(1), "op-1"),
                    transition,
                    recorded_at: format!("stamp-{index}"),
                },
            })
            .collect::<Vec<_>>();
        records.push(JournalRecord::Transition {
            entry: LifecycleEntry {
                seq: 5,
                key: LifecycleKey::new("other", LifecycleEpoch(1), "op-2"),
                transition: LifecycleTransition::EpochOpened,
                recorded_at: "other-stamp".into(),
            },
        });
        let value = inspect_claim(
            &claim,
            &JournalLoad {
                records,
                tail: TailStatus::Intact,
            },
        );

        assert_eq!(value["transitions"].as_array().unwrap().len(), 4);
        assert_eq!(value["verdict"]["status"], "restored");
        assert_eq!(value["checkpoint"]["authority"]["session_id"], "session-1");
    }
}
