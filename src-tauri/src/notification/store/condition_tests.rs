use codeterm_types::notification::NotificationAction;
use codeterm_types::pty::{PendingPrompt, PromptIdentity, PromptKind, PromptOption};

use super::*;
use crate::notification::{persist, push};

fn provenance(tab_id: &str) -> NotificationProvenance {
    NotificationProvenance {
        tab_id: Some(tab_id.to_string()),
        ..Default::default()
    }
}

fn condition_request(tab_id: &str, condition: TabCondition, body: &str) -> PushRequest {
    let provenance = provenance(tab_id);
    PushRequest {
        scope: push::scope_key(&provenance),
        level: NotifyLevel::ActionNeeded,
        event_key: push::event_key(Some(&condition.key(tab_id)), "opaque title"),
        title: "opaque title".into(),
        body: Some(body.into()),
        severity: NotificationSeverity::Warning,
        provenance,
        per_scope_limit: 100,
        ask: Ask::about(condition, None),
    }
}

fn prompt_actions() -> Vec<NotificationAction> {
    let prompt = PendingPrompt {
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

fn open_ids(store: &NotificationStore) -> Vec<String> {
    store
        .list(false)
        .into_iter()
        .filter(|record| !record.resolved)
        .map(|record| record.id.clone())
        .collect()
}

#[test]
fn one_live_record_per_tab_and_condition() {
    let mut store = NotificationStore::new(10);
    let first = store.add_push(condition_request("tab-a", TabCondition::AgentExited, "b1"));
    store.mark_read(first.id());
    let again = store.add_push(condition_request("tab-a", TabCondition::AgentExited, "b1"));

    assert!(first.should_push());
    assert_eq!(again, PushAdmission::Duplicate(first.id().to_string()));
    assert_eq!(store.list(false).len(), 1);
}

#[test]
fn a_changed_notice_supersedes_the_standing_record() {
    let mut store = NotificationStore::new(10);
    let old = store.add_push(condition_request(
        "tab-a",
        TabCondition::PendingPrompt,
        "b1",
    ));
    let new = store.add_push(condition_request(
        "tab-a",
        TabCondition::PendingPrompt,
        "b2",
    ));

    assert_ne!(old.id(), new.id());
    assert_eq!(store.take_superseded(), vec![old.id().to_string()]);
    assert!(store.take_superseded().is_empty());
    assert_eq!(open_ids(&store), vec![new.id().to_string()]);
}

#[test]
fn resolving_one_condition_leaves_other_tabs_and_conditions_open() {
    let mut store = NotificationStore::new(10);
    let exit_a = store.add_push(condition_request("tab-a", TabCondition::AgentExited, "b"));
    let prompt_a = store.add_push(condition_request("tab-a", TabCondition::PendingPrompt, "b"));
    let exit_b = store.add_push(condition_request("tab-b", TabCondition::AgentExited, "b"));

    let resolved = store.resolve_tab_condition("tab-a", TabCondition::AgentExited);

    assert_eq!(resolved, vec![exit_a.id().to_string()]);
    let record = store.notification(exit_a.id()).unwrap();
    assert!(record.resolved && record.read);
    assert_eq!(
        open_ids(&store),
        vec![prompt_a.id().to_string(), exit_b.id().to_string()]
    );
    assert_eq!(
        store.resolve_tab_conditions("tab-a"),
        vec![prompt_a.id().to_string()]
    );
}

#[test]
fn a_condition_record_never_coalesces_with_another_notice() {
    let mut store = NotificationStore::new(10);
    let provenance = provenance("tab-a");
    let plain = store.add_push(PushRequest {
        scope: push::scope_key(&provenance),
        level: NotifyLevel::ActionNeeded,
        event_key: push::event_key(Some("caller-key"), "opaque"),
        title: "opaque".into(),
        body: None,
        severity: NotificationSeverity::Info,
        provenance,
        per_scope_limit: 100,
        ask: Ask::default(),
    });
    let exit = store.add_push(condition_request("tab-a", TabCondition::AgentExited, "b"));

    assert_ne!(plain.id(), exit.id());
    assert_eq!(store.notification(plain.id()).unwrap().condition, None);
    assert_eq!(
        store.notification(exit.id()).unwrap().condition,
        Some(TabCondition::AgentExited)
    );
}

#[test]
fn the_reported_latch_releases_when_the_condition_resolves() {
    let mut store = NotificationStore::new(10);
    assert_eq!(
        store.observe_tab_condition("tab-a", TabCondition::PendingPrompt),
        ConditionTransition::Announce
    );
    assert_eq!(
        store.observe_tab_condition("tab-a", TabCondition::PendingPrompt),
        ConditionTransition::AlreadyAnnounced
    );
    store.resolve_tab_condition("tab-a", TabCondition::PendingPrompt);
    assert_eq!(
        store.observe_tab_condition("tab-a", TabCondition::PendingPrompt),
        ConditionTransition::Announce
    );
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
