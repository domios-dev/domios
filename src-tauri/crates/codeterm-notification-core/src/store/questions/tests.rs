use super::*;

fn question(store: &mut NotificationStore, tab: &str) -> String {
    store.add_ext(
        "opaque question".into(),
        None,
        NotificationSeverity::Info,
        NotificationProvenance {
            tab_id: Some(tab.into()),
            ..Default::default()
        },
        Ask::offering(Some(vec![NotificationAction::Answer {
            label: "opaque choice".into(),
        }])),
        None,
        Some(NotifyLevel::ActionNeeded),
    )
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
fn notify_resolve_cannot_grant_host_decisions() {
    let mut store = NotificationStore::new(10);
    let id = question(&mut store, "asker");
    store.notifications[0]
        .answer
        .as_mut()
        .unwrap()
        .daemon_request = Some("host-decision".into());
    assert!(store.resolve_question(&id, Some("asker")).is_err());
}
