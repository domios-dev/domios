//! Tests for notification creation, routing surfaces, and the push leg.

use super::*;
use crate::notification::store::NotificationStore;
use codeterm_types::config::schema::NotificationConfig;
use parking_lot::Mutex;
use std::sync::Arc;

/// A user who switched the desktop surface on and opened every level's column.
/// The shipped grid routes nothing to the desktop, so an open gate has to be
/// asked for explicitly here rather than inherited from the defaults.
fn opted_in_config() -> NotificationConfig {
    let mut cfg = NotificationConfig {
        desktop: true,
        ..NotificationConfig::default()
    };
    for row in &mut cfg.agent_levels {
        row.set(
            codeterm_types::notification::NotifyDestination::Desktop,
            true,
        );
    }
    cfg
}

/// A caller that named a level, judged against a policy that admits it.
fn declared(level: NotifyLevel) -> SurfaceDispatch {
    let cfg = opted_in_config();
    SurfaceDispatch {
        declared: Some(level),
        bell: BellPolicy::from_config(&cfg),
        desktop: DesktopPolicy::from_config(&cfg),
    }
}

/// A caller that named none — refused by the gate, not by an empty policy. The
/// policy is the opted-in one on purpose: the missing level is the only thing
/// left that can refuse, so the refusal cannot be a switched-off surface.
fn undeclared() -> SurfaceDispatch {
    let cfg = opted_in_config();
    SurfaceDispatch {
        declared: None,
        bell: BellPolicy::from_config(&cfg),
        desktop: DesktopPolicy::from_config(&cfg),
    }
}

#[test]
fn explicit_notification_persists_emits_and_requests_native_delivery() {
    let store = Arc::new(Mutex::new(NotificationStore::new(10)));
    let emitted = Mutex::new(Vec::new());
    let native = Mutex::new(Vec::new());

    let id = create_notification_with_delivery(
        NotificationAdmission::Explicit,
        &store,
        |title, body| {
            assert_eq!(
                store.lock().list(false).len(),
                1,
                "durable history is written before the native sink runs"
            );
            native
                .lock()
                .push((title.to_string(), body.map(str::to_string)));
        },
        "Build finished".into(),
        Some("release artifacts ready".into()),
        Some("warning"),
        crate::notification::store::Ask::default(),
        None,
        NotificationOrigin::default(),
        |_| None,
        PushDispatch::inert(),
        declared(NotifyLevel::Verbose),
        |payload| emitted.lock().push(payload),
    );

    let guard = store.lock();
    let listed = guard.list(false);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, id, "returned id names the stored record");
    assert_eq!(listed[0].title, "Build finished");
    assert_eq!(listed[0].body.as_deref(), Some("release artifacts ready"));
    assert_eq!(listed[0].severity, NotificationSeverity::Warning);
    assert!(!listed[0].read);
    drop(guard);

    assert_eq!(
        native.lock().as_slice(),
        &[(
            "Build finished".to_string(),
            Some("release artifacts ready".to_string())
        )]
    );
    let events = emitted.lock();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["id"], id);
}

#[test]
fn native_sink_failure_does_not_drop_durable_history() {
    let store = Arc::new(Mutex::new(NotificationStore::new(10)));
    let emitted = Mutex::new(Vec::new());

    // Denied permission / unsupported platform: the sink drops the toast.
    let id = create_notification_with_delivery(
        NotificationAdmission::Explicit,
        &store,
        |_, _| {},
        "Escalation".into(),
        Some("user attention required".into()),
        Some("error"),
        crate::notification::store::Ask::default(),
        None,
        NotificationOrigin::default(),
        |_| None,
        PushDispatch::inert(),
        declared(NotifyLevel::ActionNeeded),
        |payload| emitted.lock().push(payload),
    );

    let mut guard = store.lock();
    assert_eq!(guard.list(false).len(), 1);
    assert_eq!(guard.list(false)[0].id, id);
    assert_eq!(emitted.lock().len(), 1, "bell route still fires");

    guard.clear(Some(&id));
    assert!(
        guard.list(false).is_empty(),
        "explicit clear is the only way the record leaves history"
    );
}

#[test]
fn diagnostic_callers_never_enter_notification_history() {
    let store = Arc::new(Mutex::new(NotificationStore::new(20)));
    let emitted = Mutex::new(Vec::new());
    let native = Mutex::new(Vec::new());
    let sources = [
        "service/spawn.rs:289",
        "service/spawn.rs:703",
        "cron/notify.rs",
        "cron/scheduler.rs",
        "service/orchestrator/refresh/driver.rs",
        "service/orchestrator/batch.rs",
        "service/output.rs",
        "service/tab.rs",
        "service/spawn_chatbackend.rs",
    ];

    for source in sources {
        create_notification_with_delivery(
            NotificationAdmission::Diagnostic,
            &store,
            |title, body| {
                native
                    .lock()
                    .push((title.to_string(), body.map(str::to_string)))
            },
            format!("Diagnostic from {source}"),
            Some("warning or error detail".into()),
            Some("warning"),
            crate::notification::store::Ask::default(),
            None,
            NotificationOrigin::default(),
            |_| None,
            PushDispatch::inert(),
            SurfaceDispatch::silent(),
            |payload| emitted.lock().push(payload),
        );
    }

    assert_eq!(store.lock().list(false).len(), 0);
    assert_eq!(emitted.lock().len(), 0);
    assert_eq!(native.lock().len(), 0);
}

mod notification_provenance {
    use super::*;

    /// Stand-in for the live multiplexer: only ids present here resolve, and
    /// title plus workspace come from the registry, never from the caller.
    const LIVE_TABS: [(&str, &str, &str); 2] = [
        ("tab_caller", "Worker lane", "ws_lanes"),
        ("tab_bystander", "Unrelated shell", "ws_scratch"),
    ];

    fn live_registry(candidate: &str) -> Option<NotificationProvenance> {
        LIVE_TABS
            .iter()
            .find(|(id, _, _)| *id == candidate)
            .map(|(id, title, workspace)| NotificationProvenance {
                tab_id: Some((*id).to_string()),
                tab_title: Some((*title).to_string()),
                workspace_id: Some((*workspace).to_string()),
                peer_name: None,
            })
    }

    #[test]
    fn unresolvable_claimed_tab_is_dropped_not_stored() {
        let store = Arc::new(Mutex::new(NotificationStore::new(10)));
        let emitted = Mutex::new(Vec::new());
        assert!(
            live_registry("tab_bystander").is_some(),
            "the fixture keeps an unrelated live tab so an empty registry cannot short-circuit"
        );

        let id = create_notification_with_delivery(
            NotificationAdmission::Explicit,
            &store,
            |_, _| {},
            "Attention".into(),
            Some("agent needs input".into()),
            Some("warning"),
            crate::notification::store::Ask::default(),
            None,
            NotificationOrigin::claimed(Some("tab_never_existed".into())),
            live_registry,
            PushDispatch::inert(),
            SurfaceDispatch::silent(),
            |payload| emitted.lock().push(payload),
        );

        let guard = store.lock();
        let listed = guard.list(false);
        assert_eq!(listed.len(), 1, "the notification still lands");
        assert_eq!(listed[0].id, id);
        assert_eq!(listed[0].tab_id, None, "the claim is dropped, not stored");
        assert_eq!(listed[0].workspace_id, None);
        assert_eq!(listed[0].tab_title, None);
        drop(guard);

        let events = emitted.lock();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["tab_id"], serde_json::Value::Null);
    }

    #[test]
    fn resolved_tab_overrides_a_conflicting_claim() {
        let store = Arc::new(Mutex::new(NotificationStore::new(10)));
        let emitted = Mutex::new(Vec::new());
        let origin = NotificationOrigin {
            caller_tab_id: Some("tab_caller".into()),
            claimed_tab_id: Some("tab_bystander".into()),
            peer_name: None,
        };
        assert_ne!(
            origin.caller_tab_id, origin.claimed_tab_id,
            "claim and resolution must differ for this assertion to mean anything"
        );
        assert!(
            live_registry("tab_bystander").is_some(),
            "the losing claim is itself a live tab, so only precedence can decide"
        );

        create_notification_with_delivery(
            NotificationAdmission::Explicit,
            &store,
            |_, _| {},
            "Attention".into(),
            None,
            Some("info"),
            crate::notification::store::Ask::default(),
            None,
            origin,
            live_registry,
            PushDispatch::inert(),
            SurfaceDispatch::silent(),
            |payload| emitted.lock().push(payload),
        );

        let guard = store.lock();
        let listed = guard.list(false);
        assert_eq!(listed[0].tab_id.as_deref(), Some("tab_caller"));
        assert_eq!(listed[0].tab_title.as_deref(), Some("Worker lane"));
        assert_eq!(listed[0].workspace_id.as_deref(), Some("ws_lanes"));
        drop(guard);

        assert_eq!(emitted.lock()[0]["tab_id"], "tab_caller");
    }

    #[test]
    fn mesh_peer_origin_never_stores_a_tab() {
        let store = Arc::new(Mutex::new(NotificationStore::new(10)));
        let origin = NotificationOrigin {
            caller_tab_id: None,
            claimed_tab_id: Some("tab_caller".into()),
            peer_name: Some("BLACKSTAR".into()),
        };

        create_notification_with_delivery(
            NotificationAdmission::Explicit,
            &store,
            |_, _| {},
            "Remote build finished".into(),
            None,
            Some("info"),
            crate::notification::store::Ask::default(),
            None,
            origin,
            live_registry,
            PushDispatch::inert(),
            SurfaceDispatch::silent(),
            |_| {},
        );

        let guard = store.lock();
        let listed = guard.list(false);
        assert_eq!(listed[0].tab_id, None);
        assert_eq!(listed[0].workspace_id, None);
        assert_eq!(listed[0].peer_name.as_deref(), Some("BLACKSTAR"));
    }
}

mod push_render {
    use super::*;
    use crate::notification::push::MAX_BODY_CHARS;

    fn live_tab(candidate: &str) -> Option<NotificationProvenance> {
        (candidate == "tab_worker").then(|| NotificationProvenance {
            tab_id: Some("tab_worker".into()),
            tab_title: Some("Worker lane".into()),
            workspace_id: Some("ws_lanes".into()),
            peer_name: None,
        })
    }

    fn dispatch<F>(level: NotifyLevel, deliver: F) -> PushDispatch<F>
    where
        F: FnOnce(PushEnvelope) -> Result<(), String>,
    {
        PushDispatch {
            intent: PushIntent {
                level: Some(level),
                event_key: Some("release-42".into()),
            },
            per_scope_limit: 10,
            deliver,
        }
    }

    #[test]
    fn push_send_failure_does_not_change_the_returned_id() {
        let store = Arc::new(Mutex::new(NotificationStore::new(10)));
        let emitted = Mutex::new(Vec::new());
        let attempted = Mutex::new(Vec::new());

        // Bot blocked / chat unreachable: the sink refuses the envelope.
        let id = create_notification_with_delivery(
            NotificationAdmission::Explicit,
            &store,
            |_, _| {},
            "Release cut".into(),
            Some("artifacts uploaded".into()),
            Some("info"),
            crate::notification::store::Ask::default(),
            None,
            NotificationOrigin::claimed(Some("tab_worker".into())),
            live_tab,
            dispatch(NotifyLevel::Verbose, |envelope: PushEnvelope| {
                attempted.lock().push(envelope.text().to_string());
                Err("chat unreachable".to_string())
            }),
            declared(NotifyLevel::Verbose),
            |payload| emitted.lock().push(payload),
        );

        assert_eq!(attempted.lock().len(), 1, "the sink was actually invoked");
        let guard = store.lock();
        let listed = guard.list(false);
        assert_eq!(listed.len(), 1, "durable history survives a refused push");
        assert_eq!(
            listed[0].id, id,
            "the caller's id names the stored record whether or not the push landed"
        );
        assert_eq!(
            listed[0].push_failed,
            Some(true),
            "the failure is recorded on the record, not returned"
        );
        drop(guard);

        let events = emitted.lock();
        assert_eq!(events.len(), 1, "the local routes still fire");
        assert_eq!(events[0]["id"], id);
    }

    #[test]
    fn a_delivered_push_carries_capped_text_and_leaves_the_record_clean() {
        let store = Arc::new(Mutex::new(NotificationStore::new(10)));
        let oversized = "b".repeat(MAX_BODY_CHARS + 200);
        let sent = Mutex::new(Vec::new());

        let id = create_notification_with_delivery(
            NotificationAdmission::Explicit,
            &store,
            |_, _| {},
            "Approval required".into(),
            Some(oversized.clone()),
            Some("warning"),
            crate::notification::store::Ask::default(),
            None,
            NotificationOrigin::claimed(Some("tab_worker".into())),
            live_tab,
            dispatch(NotifyLevel::ActionNeeded, |envelope: PushEnvelope| {
                sent.lock().push(envelope.text().to_string());
                Ok(())
            }),
            declared(NotifyLevel::ActionNeeded),
            |_| {},
        );

        let delivered = sent.lock();
        assert_eq!(delivered.len(), 1);
        assert_eq!(
            delivered[0].matches('b').count(),
            MAX_BODY_CHARS,
            "the agent body is truncated to the cap constant, not rejected"
        );
        assert!(delivered[0].contains("via Worker lane"));
        assert!(!delivered[0].contains("tab_worker"));
        drop(delivered);

        assert_eq!(store.lock().list(false)[0].push_failed, None);
        assert_eq!(store.lock().list(false)[0].id, id);
    }

    #[test]
    fn an_unlevelled_notification_never_reaches_the_push_sink() {
        let store = Arc::new(Mutex::new(NotificationStore::new(10)));
        let sent = Mutex::new(Vec::new());

        create_notification_with_delivery(
            NotificationAdmission::Explicit,
            &store,
            |_, _| {},
            "Still working".into(),
            Some("step 4 of 9".into()),
            Some("info"),
            crate::notification::store::Ask::default(),
            None,
            NotificationOrigin::claimed(Some("tab_worker".into())),
            live_tab,
            PushDispatch {
                intent: PushIntent::default(),
                per_scope_limit: 10,
                deliver: |envelope: PushEnvelope| {
                    sent.lock().push(envelope.text().to_string());
                    Ok(())
                },
            },
            undeclared(),
            |_| {},
        );

        assert!(
            sent.lock().is_empty(),
            "routine progress has no level, so it cannot reach the channel"
        );
        assert_eq!(store.lock().list(false).len(), 1);
    }
}

/// Both sinks a first arrival reaches: the OS toast and the bell event.
/// The gate is open here on purpose — dedupe decides whether a message is
/// new, and that question is only interesting once it is allowed at all.
fn deliver(
    store: &NotifState,
    title: &str,
    body: Option<&str>,
    native: &Mutex<Vec<String>>,
    emitted: &Mutex<Vec<serde_json::Value>>,
) -> String {
    create_notification_with_delivery(
        NotificationAdmission::Explicit,
        store,
        |title, _| native.lock().push(title.to_string()),
        title.to_string(),
        body.map(str::to_string),
        Some("info"),
        crate::notification::store::Ask::default(),
        None,
        NotificationOrigin::default(),
        |_| None,
        PushDispatch::inert(),
        declared(NotifyLevel::ActionNeeded),
        |payload| emitted.lock().push(payload),
    )
}

#[test]
fn notify_dedupe_same_message_reaches_the_sinks_once() {
    let store = Arc::new(Mutex::new(NotificationStore::new(10)));
    let native = Mutex::new(Vec::new());
    let emitted = Mutex::new(Vec::new());

    let title = "agent needs input";
    let body = Some("approve the destructive step?");
    let first = deliver(&store, title, body, &native, &emitted);
    let second = deliver(&store, title, body, &native, &emitted);

    assert_eq!(second, first, "the duplicate names the record that exists");
    assert_eq!(native.lock().len(), 1, "one native toast, not two");
    assert_eq!(emitted.lock().len(), 1, "one bell event, not two");
    assert_eq!(store.lock().list(false).len(), 1, "one record, not two");
}

#[test]
fn notify_dedupe_distinct_messages_both_reach_the_sinks() {
    let store = Arc::new(Mutex::new(NotificationStore::new(10)));
    let native = Mutex::new(Vec::new());
    let emitted = Mutex::new(Vec::new());

    let first = deliver(
        &store,
        "build failed",
        Some("linker error"),
        &native,
        &emitted,
    );
    let other_body = deliver(
        &store,
        "build failed",
        Some("test error"),
        &native,
        &emitted,
    );
    let other_title = deliver(
        &store,
        "build passed",
        Some("linker error"),
        &native,
        &emitted,
    );

    assert_ne!(first, other_body);
    assert_ne!(first, other_title);
    assert_eq!(native.lock().len(), 3);
    assert_eq!(emitted.lock().len(), 3);
    assert_eq!(store.lock().list(false).len(), 3);
}

#[test]
fn notify_dedupe_an_acknowledged_message_may_arrive_again() {
    let store = Arc::new(Mutex::new(NotificationStore::new(10)));
    let native = Mutex::new(Vec::new());
    let emitted = Mutex::new(Vec::new());

    let first = deliver(&store, "codex available", None, &native, &emitted);
    assert!(store.lock().mark_read(&first));
    let again = deliver(&store, "codex available", None, &native, &emitted);

    assert_ne!(
        again, first,
        "a recurrence after the user acknowledged it is a new event"
    );
    assert_eq!(native.lock().len(), 2);
    assert_eq!(store.lock().unread_count(), 1);
}

#[test]
fn notify_dedupe_standing_condition_survives_a_cleared_inbox() {
    let store = Arc::new(Mutex::new(NotificationStore::new(10)));
    let scope = push::tab_scope("tab_a");
    let condition = crate::notification::dedupe::provider_update_condition("codex");

    assert_eq!(
        store.lock().observe_condition_present(&scope, &condition),
        crate::notification::dedupe::ConditionTransition::Announce
    );
    store.lock().clear(None);
    assert_eq!(
        store.lock().observe_condition_present(&scope, &condition),
        crate::notification::dedupe::ConditionTransition::AlreadyAnnounced,
        "wiping history does not make a still-true condition news again"
    );

    store.lock().observe_condition_absent(&scope, &condition);
    assert_eq!(
        store.lock().observe_condition_present(&scope, &condition),
        crate::notification::dedupe::ConditionTransition::Announce,
        "only the condition resolving re-arms the announcement"
    );
}

/// `level` and `severity` are independent axes on the wire — these
/// exercise the payload, which is what the surfaces actually read.
mod notification_level_field {
    use super::*;

    #[test]
    fn notification_level_field_reaches_stored_record_and_emitted_payload() {
        let store = Arc::new(Mutex::new(NotificationStore::new(10)));
        let emitted = Mutex::new(Vec::new());

        let id = create_notification_with_delivery(
            NotificationAdmission::Explicit,
            &store,
            |_, _| {},
            "Which branch?".into(),
            None,
            Some("info"),
            crate::notification::store::Ask::default(),
            None,
            NotificationOrigin::default(),
            |_| None,
            PushDispatch::inert(),
            declared(NotifyLevel::ActionNeeded),
            |payload| emitted.lock().push(payload),
        );

        assert_eq!(
            store.lock().level(&id),
            Some(NotifyLevel::ActionNeeded),
            "the declared level is written onto the stored record"
        );
        let events = emitted.lock();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0]["level"], "action_needed",
            "the surfaces read the payload, so the level must be on it"
        );
    }

    #[test]
    fn notification_level_field_absent_stays_absent_and_still_notifies() {
        let store = Arc::new(Mutex::new(NotificationStore::new(10)));
        let emitted = Mutex::new(Vec::new());

        let id = create_notification_with_delivery(
            NotificationAdmission::Explicit,
            &store,
            |_, _| {},
            "Still working".into(),
            Some("step 4 of 9".into()),
            Some("info"),
            crate::notification::store::Ask::default(),
            None,
            NotificationOrigin::default(),
            |_| None,
            PushDispatch::inert(),
            undeclared(),
            |payload| emitted.lock().push(payload),
        );

        assert_eq!(
            store.lock().level(&id),
            None,
            "naming no level is a legal, distinct state, not a lost one"
        );
        let guard = store.lock();
        let listed = guard.list(false);
        assert_eq!(listed.len(), 1, "the record is still kept");
        assert!(!listed[0].read, "it still counts as unread");
        drop(guard);

        let events = emitted.lock();
        assert_eq!(events.len(), 1, "the bell still rings");
        assert!(
            events[0].get("level").is_none(),
            "an absent level must not appear on the payload at all"
        );
    }

    #[test]
    fn notification_level_field_independent_of_severity_survives_no_push_audience() {
        // `PushDispatch::inert()` mirrors `plan_push` clearing
        // intent with no audience — `surfaces.declared` must still land.
        let store = Arc::new(Mutex::new(NotificationStore::new(10)));
        let emitted = Mutex::new(Vec::new());

        create_notification_with_delivery(
            NotificationAdmission::Explicit,
            &store,
            |_, _| {},
            "Which branch?".into(),
            None,
            Some("info"),
            crate::notification::store::Ask::default(),
            None,
            NotificationOrigin::default(),
            |_| None,
            PushDispatch::inert(),
            declared(NotifyLevel::ActionNeeded),
            |payload| emitted.lock().push(payload),
        );

        let events = emitted.lock();
        assert_eq!(events[0]["severity"], "Info");
        assert_eq!(
            events[0]["level"], "action_needed",
            "severity being the info default must not drag the level down with it"
        );
    }
}
