use super::*;

#[test]
fn test_add_notification() {
    let mut store = NotificationStore::new(100);
    let id = store.add("Test".into(), None, NotificationSeverity::Info, None, None);
    assert!(!id.is_empty());
    assert_eq!(store.list(false).len(), 1);
}

#[test]
fn test_notification_defaults_unread() {
    let mut store = NotificationStore::new(100);
    store.add("Test".into(), None, NotificationSeverity::Info, None, None);
    assert_eq!(store.unread_count(), 1);
    assert_eq!(store.list(true).len(), 1);
}

#[test]
fn test_mark_read() {
    let mut store = NotificationStore::new(100);
    let id = store.add("Test".into(), None, NotificationSeverity::Info, None, None);
    assert!(store.mark_read(&id));
    assert_eq!(store.unread_count(), 0);
    assert_eq!(store.list(true).len(), 0);
    assert_eq!(store.list(false).len(), 1);
}

#[test]
fn test_mark_read_nonexistent() {
    let mut store = NotificationStore::new(100);
    assert!(!store.mark_read("nonexistent"));
}

#[test]
fn test_clear_specific() {
    let mut store = NotificationStore::new(100);
    let id1 = store.add("A".into(), None, NotificationSeverity::Info, None, None);
    let _id2 = store.add("B".into(), None, NotificationSeverity::Info, None, None);
    store.clear(Some(&id1));
    assert_eq!(store.list(false).len(), 1);
    assert_eq!(store.list(false)[0].title, "B");
}

#[test]
fn test_clear_all() {
    let mut store = NotificationStore::new(100);
    store.add("A".into(), None, NotificationSeverity::Info, None, None);
    store.add("B".into(), None, NotificationSeverity::Info, None, None);
    store.clear(None);
    assert_eq!(store.list(false).len(), 0);
}

#[test]
fn test_eviction() {
    let mut store = NotificationStore::new(3);
    store.add("A".into(), None, NotificationSeverity::Info, None, None);
    store.add("B".into(), None, NotificationSeverity::Info, None, None);
    store.add("C".into(), None, NotificationSeverity::Info, None, None);
    store.add("D".into(), None, NotificationSeverity::Info, None, None);
    assert_eq!(store.list(false).len(), 3);
    let titles: Vec<&str> = store.list(false).iter().map(|n| n.title.as_str()).collect();
    assert_eq!(titles, vec!["B", "C", "D"]);
}

#[test]
fn test_notification_severities() {
    let mut store = NotificationStore::new(100);
    store.add("Info".into(), None, NotificationSeverity::Info, None, None);
    store.add(
        "Warn".into(),
        None,
        NotificationSeverity::Warning,
        None,
        None,
    );
    store.add("Err".into(), None, NotificationSeverity::Error, None, None);
    let severities: Vec<&NotificationSeverity> =
        store.list(false).iter().map(|n| &n.severity).collect();
    assert_eq!(
        severities,
        vec![
            &NotificationSeverity::Info,
            &NotificationSeverity::Warning,
            &NotificationSeverity::Error
        ]
    );
}

#[test]
fn test_notification_with_body_and_tab() {
    let mut store = NotificationStore::new(100);
    store.add(
        "Title".into(),
        Some("Body text".into()),
        NotificationSeverity::Info,
        Some("tab-1".into()),
        Some("ws-1".into()),
    );
    let n = &store.list(false)[0];
    assert_eq!(n.title, "Title");
    assert_eq!(n.body.as_deref(), Some("Body text"));
    assert_eq!(n.tab_id.as_deref(), Some("tab-1"));
    assert_eq!(n.workspace_id.as_deref(), Some("ws-1"));
}

/// Every test here is synchronous and clock-free by construction: needing a
/// runtime or a time advance would prove a timer, not the mechanism.
mod notification_admission {
    use super::*;
    use crate::notification::push;
    use std::collections::HashSet;

    const GENEROUS_LIMIT: u32 = 50;

    fn provenance_for(tab_id: &str) -> NotificationProvenance {
        NotificationProvenance {
            tab_id: Some(tab_id.to_string()),
            tab_title: Some("Worker lane".into()),
            workspace_id: Some("ws_lanes".into()),
            peer_name: None,
        }
    }

    fn request(
        tab_id: &str,
        level: NotifyLevel,
        supplied_key: Option<&str>,
        title: &str,
        body: &str,
        per_scope_limit: u32,
    ) -> PushRequest {
        let provenance = provenance_for(tab_id);
        PushRequest {
            scope: push::scope_key(&provenance),
            level,
            event_key: push::event_key(supplied_key, title),
            title: title.to_string(),
            body: Some(body.to_string()),
            severity: NotificationSeverity::Info,
            provenance,
            per_scope_limit,
            ask: Ask::default(),
        }
    }

    #[test]
    fn same_event_key_while_unread_is_a_duplicate() {
        const KEY: &str = "release-build";
        const FIRST_TITLE: &str = "Build started";
        const SECOND_TITLE: &str = "Build finished";
        const FIRST_BODY: &str = "compiling the workspace";
        const SECOND_BODY: &str = "artifacts are on disk";
        assert_ne!(
            FIRST_TITLE, SECOND_TITLE,
            "the two calls must differ in text so a text-hash dedup cannot pass this"
        );
        assert_ne!(FIRST_BODY, SECOND_BODY);

        let mut store = NotificationStore::new(10);
        let first = store.add_push(request(
            "tab_a",
            NotifyLevel::Verbose,
            Some(KEY),
            FIRST_TITLE,
            FIRST_BODY,
            GENEROUS_LIMIT,
        ));
        let second = store.add_push(request(
            "tab_a",
            NotifyLevel::Verbose,
            Some(KEY),
            SECOND_TITLE,
            SECOND_BODY,
            GENEROUS_LIMIT,
        ));

        assert!(matches!(first, PushAdmission::Deliver(_)));
        assert_eq!(second, PushAdmission::Duplicate(first.id().to_string()));
        assert!(!second.should_push());
        assert!(!second.opened_or_changed_record());
        assert_eq!(store.list(false).len(), 1, "no second record is created");
        assert_eq!(
            store.list(false)[0].title,
            FIRST_TITLE,
            "a duplicate leaves the open record untouched"
        );
    }

    #[test]
    fn reading_the_record_closes_the_boundary_for_the_same_key() {
        const KEY: &str = "release-build";
        let mut store = NotificationStore::new(10);
        let first = store.add_push(request(
            "tab_a",
            NotifyLevel::Verbose,
            Some(KEY),
            "Build started",
            "one",
            GENEROUS_LIMIT,
        ));
        assert!(store.mark_read(first.id()));

        let after_read = store.add_push(request(
            "tab_a",
            NotifyLevel::Verbose,
            Some(KEY),
            "Build started again",
            "two",
            GENEROUS_LIMIT,
        ));
        assert!(
            matches!(after_read, PushAdmission::Deliver(_)),
            "read closes the coalescing boundary — no timer does"
        );
        assert_ne!(after_read.id(), first.id());
        assert_eq!(store.list(false).len(), 2);
    }

    #[test]
    fn different_event_key_supersedes_in_place() {
        let mut store = NotificationStore::new(10);
        let first = store.add_push(request(
            "tab_a",
            NotifyLevel::Verbose,
            Some("step-1"),
            "Step one done",
            "first body",
            GENEROUS_LIMIT,
        ));
        let second = store.add_push(request(
            "tab_a",
            NotifyLevel::Verbose,
            Some("step-2"),
            "Step two done",
            "second body",
            GENEROUS_LIMIT,
        ));

        assert!(matches!(second, PushAdmission::Supersede(_)));
        assert!(second.should_push());
        assert_eq!(
            second.id(),
            first.id(),
            "in place means the same record id, not a new one"
        );
        let listed = store.list(false);
        assert_eq!(listed.len(), 1, "the inbox does not grow");
        assert_eq!(listed[0].title, "Step two done");
        assert_eq!(listed[0].body.as_deref(), Some("second body"));
        assert!(!listed[0].read);
    }

    #[test]
    fn per_tab_ceiling_suppresses_novel_event_keys() {
        const LIMIT: u32 = 3;
        let mut store = NotificationStore::new(10);
        let mut keys: Vec<String> = Vec::new();
        let mut outcomes: Vec<PushAdmission> = Vec::new();

        for step in 0..(LIMIT + 2) {
            let key = format!("novel-event-{step}");
            keys.push(key.clone());
            outcomes.push(store.add_push(request(
                "tab_a",
                NotifyLevel::Verbose,
                Some(&key),
                &format!("Progress {step}"),
                "body",
                LIMIT,
            )));
        }

        let unique: HashSet<&String> = keys.iter().collect();
        assert_eq!(
            unique.len(),
            keys.len(),
            "every key is novel, so the dedup branch cannot absorb any of them"
        );
        assert!(
            !outcomes
                .iter()
                .any(|o| matches!(o, PushAdmission::Duplicate(_))),
            "no admission may take the dedup branch in this fixture"
        );
        assert!(
            outcomes[..LIMIT as usize].iter().all(|o| o.should_push()),
            "everything under the ceiling still pushes"
        );
        assert!(
            outcomes[LIMIT as usize..]
                .iter()
                .all(|o| matches!(o, PushAdmission::Suppressed(_)) && !o.should_push()),
            "novel keys past the ceiling are suppressed"
        );
        assert_eq!(
            store.list(false).len(),
            1,
            "the durable record still lands and stays coalesced"
        );
    }

    #[test]
    fn clearing_the_inbox_does_not_reset_the_lifetime_ceiling() {
        const LIMIT: u32 = 1;
        let mut store = NotificationStore::new(10);
        let first = store.add_push(request(
            "tab_a",
            NotifyLevel::Verbose,
            Some("event-1"),
            "One",
            "body",
            LIMIT,
        ));
        assert!(first.should_push());
        store.clear(None);

        let after_clear = store.add_push(request(
            "tab_a",
            NotifyLevel::Verbose,
            Some("event-2"),
            "Two",
            "body",
            LIMIT,
        ));
        assert!(
            matches!(after_clear, PushAdmission::Suppressed(_)),
            "the ceiling is a lifetime count, so clearing cannot buy more pushes"
        );
    }

    #[test]
    fn closing_the_tab_scope_releases_its_slot_and_counter() {
        const LIMIT: u32 = 1;
        let mut store = NotificationStore::new(10);
        let scope = push::scope_key(&provenance_for("tab_a"));
        store.add_push(request(
            "tab_a",
            NotifyLevel::Verbose,
            Some("event-1"),
            "One",
            "body",
            LIMIT,
        ));
        store.close_push_scope(&scope);

        let reopened = store.add_push(request(
            "tab_a",
            NotifyLevel::Verbose,
            Some("event-2"),
            "Two",
            "body",
            LIMIT,
        ));
        assert!(matches!(reopened, PushAdmission::Deliver(_)));
        assert_eq!(store.list(false).len(), 2);
    }

    #[test]
    fn missing_event_key_still_dedups_by_derived_identity() {
        let mut store = NotificationStore::new(10);
        let first = store.add_push(request(
            "tab_a",
            NotifyLevel::ActionNeeded,
            None,
            "Approval required",
            "first body",
            GENEROUS_LIMIT,
        ));
        let second = store.add_push(request(
            "tab_a",
            NotifyLevel::ActionNeeded,
            None,
            "Approval required",
            "second body",
            GENEROUS_LIMIT,
        ));

        assert_eq!(
            second,
            PushAdmission::Duplicate(first.id().to_string()),
            "omitting the key derives one, so dedup is never opt-in"
        );
        assert_eq!(store.list(false).len(), 1);
    }

    #[test]
    fn levels_and_scopes_do_not_share_a_slot() {
        let mut store = NotificationStore::new(10);
        let verbose = store.add_push(request(
            "tab_a",
            NotifyLevel::Verbose,
            Some("shared"),
            "A",
            "body",
            GENEROUS_LIMIT,
        ));
        let action_needed = store.add_push(request(
            "tab_a",
            NotifyLevel::ActionNeeded,
            Some("shared"),
            "B",
            "body",
            GENEROUS_LIMIT,
        ));
        let other_tab = store.add_push(request(
            "tab_b",
            NotifyLevel::Verbose,
            Some("shared"),
            "C",
            "body",
            GENEROUS_LIMIT,
        ));

        assert!(matches!(verbose, PushAdmission::Deliver(_)));
        assert!(matches!(action_needed, PushAdmission::Deliver(_)));
        assert!(matches!(other_tab, PushAdmission::Deliver(_)));
        assert_eq!(store.list(false).len(), 3);
    }

    #[test]
    fn a_failed_push_marks_the_record_without_removing_it() {
        let mut store = NotificationStore::new(10);
        let admitted = store.add_push(request(
            "tab_a",
            NotifyLevel::Failure,
            Some("event-1"),
            "Terminal failure",
            "body",
            GENEROUS_LIMIT,
        ));
        assert!(store.mark_push_failed(admitted.id()));
        assert!(!store.mark_push_failed("never-existed"));

        let listed = store.list(false);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, admitted.id());
        assert_eq!(listed[0].push_failed, Some(true));
    }
}

/// Every construction path that can carry a level writes it onto the
/// record. Payload assembly is covered in `service::notification`.
mod notification_level_field {
    use super::*;
    use crate::notification::push;

    #[test]
    fn add_ext_stores_the_level_independent_of_severity() {
        let mut store = NotificationStore::new(10);
        let id = store.add_ext(
            "Which branch?".into(),
            None,
            NotificationSeverity::Info,
            NotificationProvenance::default(),
            Ask::default(),
            None,
            Some(NotifyLevel::ActionNeeded),
        );
        let listed = store.list(false);
        assert_eq!(listed[0].id, id);
        assert_eq!(listed[0].severity, NotificationSeverity::Info);
        assert_eq!(listed[0].level, Some(NotifyLevel::ActionNeeded));
    }

    #[test]
    fn add_ext_with_no_level_leaves_it_absent() {
        let mut store = NotificationStore::new(10);
        store.add_ext(
            "Still working".into(),
            None,
            NotificationSeverity::Info,
            NotificationProvenance::default(),
            Ask::default(),
            None,
            None,
        );
        assert_eq!(store.list(false)[0].level, None);
    }

    #[test]
    fn add_deduped_carries_a_declared_level_onto_the_record() {
        let mut store = NotificationStore::new(10);
        let admission = store.add_deduped(
            "Which branch?".into(),
            None,
            NotificationSeverity::Info,
            NotificationProvenance::default(),
            Ask::default(),
            None,
            Some(NotifyLevel::ActionNeeded),
        );
        let id = match admission {
            DeliveryAdmission::Deliver(id) => id,
            DeliveryAdmission::Duplicate(_) => panic!("first call must not dedupe"),
        };
        assert_eq!(store.level(&id), Some(NotifyLevel::ActionNeeded));
    }

    #[test]
    fn add_push_writes_the_request_level_on_both_fresh_and_superseded_records() {
        let mut store = NotificationStore::new(10);
        let provenance = NotificationProvenance {
            tab_id: Some("tab_a".into()),
            ..Default::default()
        };
        let fresh = store.add_push(PushRequest {
            scope: push::scope_key(&provenance),
            level: NotifyLevel::Failure,
            event_key: push::event_key(Some("step-1"), "Step one"),
            title: "Step one".into(),
            body: None,
            severity: NotificationSeverity::Info,
            provenance: provenance.clone(),
            per_scope_limit: 50,
            ask: Ask::default(),
        });
        assert_eq!(store.level(fresh.id()), Some(NotifyLevel::Failure));

        let superseded = store.add_push(PushRequest {
            scope: push::scope_key(&provenance),
            level: NotifyLevel::Failure,
            event_key: push::event_key(Some("step-2"), "Step two"),
            title: "Step two".into(),
            body: None,
            severity: NotificationSeverity::Info,
            provenance,
            per_scope_limit: 50,
            ask: Ask::default(),
        });
        assert_eq!(superseded.id(), fresh.id(), "supersede reuses the record");
        assert_eq!(store.level(superseded.id()), Some(NotifyLevel::Failure));
    }
}

mod notification_deep_link {
    use super::super::*;

    #[test]
    fn notification_deep_link_target_persists_and_serializes() {
        let mut store = NotificationStore::new(10);
        let id = store.add(
            "Approve?".into(),
            None,
            NotificationSeverity::Info,
            None,
            None,
        );
        assert!(store.set_target(
            &id,
            NotificationTarget::from_hints(Some("tab-target"), Some("needs-you-message-1")),
        ));

        let record = store.notification(&id).expect("record stays in the store");
        assert_eq!(record.target_tab_id.as_deref(), Some("tab-target"));
        assert_eq!(
            record.chat_message_id.as_deref(),
            Some("needs-you-message-1")
        );
        let listed = serde_json::to_value(store.list(false)).unwrap();
        assert_eq!(listed[0]["target_tab_id"], "tab-target");
        assert_eq!(listed[0]["chat_message_id"], "needs-you-message-1");
    }

    #[test]
    fn notification_deep_link_old_record_serializes_without_target_keys() {
        let mut store = NotificationStore::new(10);
        let id = store.add(
            "Legacy".into(),
            None,
            NotificationSeverity::Info,
            None,
            None,
        );
        let record = serde_json::to_value(store.notification(&id).unwrap()).unwrap();
        assert!(record.get("target_tab_id").is_none());
        assert!(record.get("chat_message_id").is_none());
        assert_eq!(record["title"], "Legacy", "the old shape stays readable");
    }

    #[test]
    fn notification_deep_link_set_target_refuses_a_missing_record() {
        let mut store = NotificationStore::new(10);
        assert!(!store.set_target(
            "never-stored",
            NotificationTarget::from_hints(Some("tab-target"), Some("m-1")),
        ));
    }

    #[test]
    fn notification_deep_link_hints_drop_blank_and_orphan_ids() {
        assert_eq!(
            NotificationTarget::from_hints(Some("  "), Some("m-1")),
            NotificationTarget::default(),
            "a message id without its tab names nothing"
        );
        let trimmed = NotificationTarget::from_hints(Some(" tab-target "), Some(" "));
        assert_eq!(trimmed.tab_id.as_deref(), Some("tab-target"));
        assert_eq!(trimmed.chat_message_id, None);
    }
}
