use crate::agent::chat_types::ChatMsg;
use crate::session::lifecycle::TabLifecycleJournal;

pub fn append_restore_notice(
    journal: &TabLifecycleJournal,
    tab_id: &str,
    messages: &mut Vec<ChatMsg>,
) {
    if let Some(session) = journal.latest_empty_session_restore(tab_id) {
        append_notice(tab_id, &session, messages);
    }
}

pub fn snapshot(
    journal: &TabLifecycleJournal,
    tab_id: &str,
    mut messages: Vec<ChatMsg>,
    since: usize,
) -> (Vec<ChatMsg>, usize) {
    append_restore_notice(journal, tab_id, &mut messages);
    let total = messages.len();
    (messages.into_iter().skip(since).collect(), total)
}

fn append_notice(tab_id: &str, session_id: &str, messages: &mut Vec<ChatMsg>) {
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
    use crate::session::lifecycle::{LifecycleEpoch, LifecycleKey, LifecycleTransition};

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
        std::fs::rename(reopened.path(), temp.path().join("detached.jsonl")).unwrap();
        let mut messages = Vec::new();
        append_restore_notice(&reopened, "other", &mut messages);
        assert!(messages.is_empty());
        append_restore_notice(&reopened, "own", &mut messages);
        append_restore_notice(&reopened, "own", &mut messages);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].kind, "system");
        let (empty_history, count) = snapshot(&reopened, "own", Vec::new(), 0);
        assert_eq!(count, 1);
        assert_eq!(empty_history[0].id, messages[0].id);
        let provider_message = ChatMsg {
            id: "provider-turn".into(),
            kind: "user".into(),
            ..Default::default()
        };
        let (history, count) = snapshot(&reopened, "own", vec![provider_message], 1);
        assert_eq!(count, 2);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, "provider-turn");
        let (history, count) = snapshot(&reopened, "own", messages, 1);
        assert_eq!(count, 1);
        assert!(history.is_empty());
    }
}
