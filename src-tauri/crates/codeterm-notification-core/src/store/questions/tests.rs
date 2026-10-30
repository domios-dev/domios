use super::*;

fn question(store: &mut NotificationStore, tab: &str) -> String {
    store.add_ext("opaque question".into(), None, NotificationSeverity::Info,
        NotificationProvenance { tab_id: Some(tab.into()), ..Default::default() },
        Ask::offering(Some(vec![NotificationAction::Answer { label: "opaque choice".into() }])),
        None, Some(NotifyLevel::ActionNeeded))
}

#[test]
fn notify_chat_answer_resolves_only_prior_questions_in_the_asking_tab() {
    let mut store = NotificationStore::new(20);
    let first = question(&mut store, "asker");
    let second = question(&mut store, "asker");
    let foreign = question(&mut store, "other");
    let future = question(&mut store, "asker");
    for record in &mut store.notifications { record.timestamp = 100; }
    store.notifications.iter_mut().find(|n| n.id == future).unwrap().timestamp = 102;
    assert_eq!(store.answer_questions_in_chat("asker", 101), vec![first.clone(), second.clone()]);
    for id in [first, second] {
        let n = store.notification(&id).unwrap();
        assert!(n.resolved && n.read && n.actions.is_none());
        let slot = n.answer.as_ref().unwrap();
        assert_eq!(slot.answered_by.as_deref(), Some("chat"));
        assert!(slot.delivered && slot.answered.is_some());
    }
    for id in [foreign, future] { assert!(!store.notification(&id).unwrap().resolved); }
    assert!(store.answer_questions_in_chat("asker", 101).is_empty());
}

#[test]
fn notify_resolve_enforces_asking_tab_and_is_idempotent() {
    let mut store = NotificationStore::new(10);
    let id = question(&mut store, "asker");
    assert!(store.resolve_question(&id, Some("other")).is_err());
    assert!(!store.notification(&id).unwrap().resolved);
    store.resolve_question(&id, Some("asker")).unwrap();
    store.resolve_question(&id, Some("asker")).unwrap();
    assert!(store.notification(&id).unwrap().resolved);
    assert!(store.resolve_question("missing", Some("asker")).is_err());
}

#[test]
fn notify_chat_or_resolve_cannot_grant_host_decisions() {
    let mut store = NotificationStore::new(10);
    let id = question(&mut store, "asker");
    store.notifications[0].answer.as_mut().unwrap().daemon_request = Some("host-decision".into());
    assert!(store.answer_questions_in_chat("asker", u64::MAX).is_empty());
    assert!(store.resolve_question(&id, Some("asker")).is_err());
}

#[test]
fn notify_tapped_choice_is_preserved_when_chat_follows() {
    let mut store = NotificationStore::new(10);
    let id = question(&mut store, "asker");
    store.record_answer(&id, "opaque choice");
    assert!(store.answer_questions_in_chat("asker", u64::MAX).is_empty());
    assert_eq!(store.answer_slot(&id).unwrap().answered.as_deref(), Some("opaque choice"));
}
