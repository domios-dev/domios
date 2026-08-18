use serde_json::{json, Value};

use crate::multiplexer::types::Tab;

use super::journal::{checkpoints, JournalLoad, TabLifecycleJournal, TailStatus};
use super::record::{JournalRecord, LifecycleEpoch};

/// Read-only journal telemetry for one live tab: lineage, ordered transitions,
/// surviving checkpoint, and journal-tail health. Never a restore input.
pub fn inspect_tab_lifecycle(tab: &Tab) -> Value {
    inspect_lineage(
        &tab.id,
        tab.lifecycle_epoch.map(LifecycleEpoch),
        tab.lifecycle_operation_id.as_deref(),
        &TabLifecycleJournal::shared().load(),
    )
}

fn inspect_lineage(
    tab_id: &str,
    epoch: Option<LifecycleEpoch>,
    operation_id: Option<&str>,
    load: &JournalLoad,
) -> Value {
    let checkpoint = checkpoints(load)
        .into_iter()
        .find(|checkpoint| checkpoint.key.tab_id == tab_id);
    let transitions = load
        .records
        .iter()
        .filter_map(|record| match record {
            JournalRecord::Transition { entry } if entry.key.tab_id == tab_id => Some(entry),
            _ => None,
        })
        .collect::<Vec<_>>();
    let lineage = match (epoch, operation_id) {
        (Some(epoch), Some(operation_id)) => json!({
            "epoch": epoch,
            "operation_id": operation_id,
        }),
        _ => Value::Null,
    };
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
        "journal_tail": tail,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::lifecycle::{LifecycleEntry, LifecycleKey, LifecycleTransition};

    #[test]
    fn inspection_is_scoped_to_the_tab_and_reports_journal_telemetry() {
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
        let value = inspect_lineage(
            "target",
            Some(LifecycleEpoch(1)),
            Some("op-1"),
            &JournalLoad {
                records,
                tail: TailStatus::Intact,
            },
        );

        assert_eq!(value["transitions"].as_array().unwrap().len(), 4);
        assert_eq!(value["lineage"]["operation_id"], "op-1");
        assert_eq!(value["checkpoint"]["authority"]["session_id"], "session-1");
        assert_eq!(value["journal_tail"]["status"], "intact");
        assert!(
            value.get("verdict").is_none(),
            "inspection is telemetry; there is no recovery verdict"
        );
    }
}
