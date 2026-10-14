use crate::dedupe::TabCondition;
use crate::persist;
use crate::push::NotifyLevel;
use crate::store::{Ask, NotificationProvenance, NotificationSeverity, NotificationStore};
use codeterm_types::notification::NotificationAction;
use codeterm_types::pty::{PendingPrompt, PromptIdentity, PromptKind, PromptOption};

fn provenance(tab_id: &str) -> NotificationProvenance {
    NotificationProvenance {
        tab_id: Some(tab_id.to_string()),
        ..Default::default()
    }
}

fn prompt_actions() -> Vec<NotificationAction> {
    let prompt = PendingPrompt {
        request_id: None,
        question: "opaque".into(),
        options: vec![PromptOption {
            label: "opaque choice".into(),
            value: "1".into(),
        }],
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    };
    vec![NotificationAction::RespondPromptOption {
        label: "opaque choice".into(),
        value: "1".into(),
        identity: PromptIdentity::new(PromptKind::Provider, &prompt),
    }]
}

fn sticky_prompt(store: &mut NotificationStore, tab_id: &str) -> String {
    store.add_ext(
        "opaque title".into(),
        Some("opaque body".into()),
        NotificationSeverity::Warning,
        provenance(tab_id),
        Ask::about(TabCondition::PendingPrompt, Some(prompt_actions())),
        Some(true),
        Some(NotifyLevel::ActionNeeded),
    )
}

#[test]
fn sticky_unread_survives_restart() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join(persist::FILE_NAME);
    let (waiting, read_sticky) = {
        let mut store = NotificationStore::open_persisted(path.clone(), 10);
        let waiting = sticky_prompt(&mut store, "tab-a");
        let read_sticky = sticky_prompt(&mut store, "tab-b");
        store.mark_read(&read_sticky);
        store.add(
            "opaque plain".into(),
            None,
            NotificationSeverity::Info,
            Some("tab-a".into()),
            None,
        );
        (waiting, read_sticky)
    };

    let mut restarted = NotificationStore::open_persisted(path.clone(), 10);

    let survivors = restarted.list(false);
    assert_eq!(survivors.len(), 1, "only unread sticky records outlive it");
    let record = survivors[0];
    assert_eq!(record.id, waiting);
    assert!(!record.read && !record.resolved);
    assert_eq!(record.sticky, Some(true));
    assert_eq!(record.condition, Some(TabCondition::PendingPrompt));
    assert_eq!(record.actions, Some(prompt_actions()));
    assert_eq!(
        record
            .answer
            .as_ref()
            .and_then(|slot| slot.asked_from_tab.as_deref()),
        Some("tab-a"),
        "the answer slot still names the tab that asked"
    );
    assert!(restarted.notification(&read_sticky).is_none());

    restarted.mark_read(&waiting);
    drop(restarted);
    assert!(NotificationStore::open_persisted(path, 10)
        .list(false)
        .is_empty());
}

#[test]
fn a_restored_condition_whose_tab_is_gone_resolves() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join(persist::FILE_NAME);
    {
        let mut store = NotificationStore::open_persisted(path.clone(), 10);
        sticky_prompt(&mut store, "tab-live");
        sticky_prompt(&mut store, "tab-gone");
    }

    let mut restarted = NotificationStore::open_persisted(path, 10);
    let retired = restarted.retire_tabless_conditions(|tab_id| tab_id == "tab-live");

    assert_eq!(retired.len(), 1);
    let open: Vec<_> = restarted
        .list(true)
        .into_iter()
        .map(|record| record.tab_id.clone())
        .collect();
    assert_eq!(open, vec![Some("tab-live".to_string())]);
}

#[test]
fn an_unreadable_store_is_kept_aside_not_overwritten() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join(persist::FILE_NAME);
    std::fs::write(&path, "{ not json").unwrap();

    let mut store = NotificationStore::open_persisted(path.clone(), 10);
    assert!(store.list(false).is_empty());
    sticky_prompt(&mut store, "tab-a");

    let aside = std::fs::read_to_string(path.with_extension("json.unreadable")).unwrap();
    assert_eq!(aside, "{ not json");
    assert_eq!(
        NotificationStore::open_persisted(path, 10)
            .list(false)
            .len(),
        1
    );
}

#[test]
fn persisted_entries_without_agent_notify_send_source_never_enter_the_bell() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join(persist::FILE_NAME);
    {
        let mut store = NotificationStore::open_persisted(path.clone(), 10);
        sticky_prompt(&mut store, "tab-a");
    }
    let mut records: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    records[0].as_object_mut().unwrap().remove("source");
    let original = serde_json::to_vec(&records).unwrap();
    std::fs::write(&path, &original).unwrap();
    let restored = NotificationStore::open_persisted(path.clone(), 10);
    assert!(restored.list(false).is_empty());
    assert_eq!(
        std::fs::read(path.with_extension("json.unreadable")).unwrap(),
        original
    );
}
