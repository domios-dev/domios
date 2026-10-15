use super::*;
use crate::session::lifecycle::{LifecycleEntry, LifecycleEpoch, LifecycleKey};

fn record(seq: u64, tab: &str, transition: LifecycleTransition) -> JournalRecord {
    JournalRecord::Transition {
        entry: LifecycleEntry {
            seq,
            key: LifecycleKey::new(tab, LifecycleEpoch(1), "operation"),
            transition,
            recorded_at: String::new(),
        },
    }
}

#[test]
fn index_projects_latest_restores_and_exact_conversation_keys() {
    let rows = vec![
        record(
            1,
            "a",
            LifecycleTransition::EmptySessionReplaced {
                session_id: "old".into(),
            },
        ),
        record(
            2,
            "b",
            LifecycleTransition::EmptySessionReplaced {
                session_id: "other".into(),
            },
        ),
        record(
            3,
            "a",
            LifecycleTransition::ConversationStarted {
                session_id: "old".into(),
            },
        ),
        record(
            4,
            "a",
            LifecycleTransition::EmptySessionReplaced {
                session_id: "new".into(),
            },
        ),
        record(
            5,
            "a",
            LifecycleTransition::ConversationStarted {
                session_id: "new".into(),
            },
        ),
    ];
    let index = JournalReadIndex::from_records(&rows);
    assert_eq!(index.restored_session("a").as_deref(), Some("new"));
    assert_eq!(index.restored_session("b").as_deref(), Some("other"));
    assert!(index.has_conversation("a", "old"));
    assert!(index.has_conversation("a", "new"));
    assert!(!index.has_conversation("b", "new"));
    assert!(!index.has_conversation("a", "missing"));
    assert!(index
        .updated(&record(6, "a", LifecycleTransition::EpochOpened))
        .is_none());
}
