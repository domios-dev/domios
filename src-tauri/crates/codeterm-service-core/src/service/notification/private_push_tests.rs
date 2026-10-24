use super::*;
use crate::notification::{push::NotifyLevel, store::NotificationStore};
use parking_lot::Mutex;
use std::sync::Arc;

#[test]
fn login_push_contains_opaque_values_only_in_transport_payload() {
    let store = Arc::new(Mutex::new(NotificationStore::new(10)));
    let sent = Mutex::new(Vec::new());
    let id = create_notification_with_delivery(
        NotificationAdmission::Explicit,
        &store,
        |_, _| {},
        "Provider sign-in".into(),
        Some("Continue provider sign-in on your phone.".into()),
        Some("info"),
        Ask::default(),
        Some(false),
        NotificationOrigin::default(),
        |_| None,
        PushDispatch {
            intent: PushIntent {
                level: Some(NotifyLevel::ActionNeeded),
                event_key: Some("opaque-event".into()),
            },
            per_scope_limit: 10,
            deliver: |envelope| {
                deliver_private_push(
                    envelope,
                    "https://example.invalid/opaque-path\nDevice code: opaque-token".into(),
                    |envelope| {
                        sent.lock()
                            .push((envelope.content.level, envelope.content.body));
                        Ok(())
                    },
                )
            },
        },
        SurfaceDispatch::silent(),
        |_| {},
    );
    assert_eq!(
        *sent.lock(),
        vec![(
            NotifyLevel::ActionNeeded,
            Some("https://example.invalid/opaque-path\nDevice code: opaque-token".into())
        )]
    );
    let guard = store.lock();
    let record = guard.notification(&id).unwrap();
    assert_eq!(
        record.body.as_deref(),
        Some("Continue provider sign-in on your phone.")
    );
    assert!(!crate::notification::persist::outlives_restart(record));
}

#[test]
fn private_push_failure_cannot_echo_transport_values() {
    let envelope = PushEnvelope {
        content: crate::notification::push::PushContent {
            level: NotifyLevel::ActionNeeded,
            title: "Provider sign-in".into(),
            body: None,
            provenance: Default::default(),
        },
        choices: vec![],
        record: "opaque-record".into(),
    };
    assert_eq!(
        deliver_private_push(envelope, "opaque-token".into(), |_| Err(
            "opaque-token".into()
        ))
        .unwrap_err(),
        "Private notification delivery failed"
    );
}
