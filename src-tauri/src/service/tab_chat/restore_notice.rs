use crate::agent::chat_types::ChatMsg;
use crate::session::lifecycle::{JournalRecord, LifecycleTransition, TabLifecycleJournal};

pub(crate) fn append_restore_notice(tab_id: &str, messages: &mut Vec<ChatMsg>) {
    let load = TabLifecycleJournal::shared().load();
    append_from_journal(&load, tab_id, messages);
}

fn append_from_journal(
    load: &crate::session::lifecycle::JournalLoad,
    tab_id: &str,
    messages: &mut Vec<ChatMsg>,
) {
    let replaced = load.records.iter().rev().find_map(|record| match record {
        JournalRecord::Transition { entry } if entry.key.tab_id == tab_id => {
            match &entry.transition {
                LifecycleTransition::EmptySessionReplaced { session_id } => Some(session_id),
                _ => None,
            }
        }
        _ => None,
    });
    let Some(session_id) = replaced else {
        return;
    };
    let id = format!("empty-session-restore-{tab_id}-{session_id}");
    if !messages.iter().any(|message| message.id == id) {
        messages.insert(0, ChatMsg {
            id,
            seq: 0,
            kind: "system".into(),
            content: "Previous session was not found; started a fresh session with the same settings.".into(),
            ..Default::default()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::lifecycle::{LifecycleEpoch, LifecycleKey};

    #[test]
    fn restore_notice_survives_reopen_and_is_added_once_to_its_own_tab() {
        let temp = tempfile::tempdir().unwrap();
        let journal = TabLifecycleJournal::for_dir(temp.path().into());
        journal
            .append(
                LifecycleKey::new("own", LifecycleEpoch(1), "restore"),
                LifecycleTransition::EmptySessionReplaced {
                    session_id: "missing".into(),
                },
            )
            .unwrap();
        journal.compact().unwrap();
        let reopened = TabLifecycleJournal::for_dir(temp.path().into());
        let mut messages = Vec::new();
        append_from_journal(&reopened.load(), "other", &mut messages);
        assert!(messages.is_empty());
        append_from_journal(&reopened.load(), "own", &mut messages);
        append_from_journal(&reopened.load(), "own", &mut messages);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].kind, "system");
    }
}
