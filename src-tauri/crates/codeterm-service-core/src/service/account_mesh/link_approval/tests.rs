use super::*;

#[test]
fn account_mesh_link_completion_refreshes_transport_and_account_views() {
    let mut success = Vec::new();
    publish_link_completion(true, |event, _| success.push(event.to_string()));
    assert_eq!(success, ["mesh-state-changed", "account-mesh-changed"]);
    let mut refused = Vec::new();
    publish_link_completion(false, |event, _| refused.push(event.to_string()));
    assert_eq!(refused, ["account-mesh-changed"]);
}

#[test]
fn account_mesh_approval_notification_offers_exact_request_choices() {
    let request = LinkRequest {
        request_id: "request-a".into(),
        installation_id: "installation-a".into(),
        name: "A".into(),
        incoming: true,
        expires_at: 600,
        decision: LinkDecision::Waiting,
        notification_id: None,
        failure: None,
    };
    let ask = approval_ask(&request);
    assert_eq!(ask.daemon_request.as_deref(), Some("mesh-link:request-a"));
    let labels = ask
        .actions
        .as_ref()
        .unwrap()
        .iter()
        .filter_map(NotificationAction::answer_label)
        .collect::<Vec<_>>();
    assert_eq!(labels, ["Approve", "Decline"]);
}

#[test]
fn account_mesh_approval_notification_reaches_bell_and_push_with_one_choice_set() {
    use crate::notification::store::NotificationStore;
    use crate::service::notification::{
        create_notification_with_delivery, NotificationAdmission, PushDispatch, SurfaceDispatch,
    };
    use parking_lot::Mutex;
    use std::sync::Arc;

    let store = Arc::new(Mutex::new(NotificationStore::new(10)));
    let envelopes = Mutex::new(Vec::new());
    let events = Mutex::new(Vec::new());
    let request = LinkRequest {
        request_id: "proof-a".into(),
        installation_id: "a".into(),
        name: "A".into(),
        incoming: true,
        expires_at: 600,
        decision: LinkDecision::Waiting,
        notification_id: None,
        failure: None,
    };
    let id = create_notification_with_delivery(
        NotificationAdmission::UserConsent,
        &store,
        |_, _| {},
        "A wants to connect".into(),
        None,
        Some("info"),
        approval_ask(&request),
        Some(true),
        NotificationOrigin::default(),
        |_| None,
        PushDispatch {
            intent: PushIntent {
                level: Some(NotifyLevel::ActionNeeded),
                event_key: Some(request.request_id),
            },
            per_scope_limit: 10,
            deliver: |envelope| {
                envelopes.lock().push(envelope);
                Ok(())
            },
        },
        SurfaceDispatch::silent(),
        |event| events.lock().push(event),
    );
    assert_eq!(store.lock().list(false).len(), 1);
    assert_eq!(store.lock().level(&id), Some(NotifyLevel::ActionNeeded));
    assert!(!store.lock().answer_slot(&id).unwrap().is_undeliverable());
    assert_eq!(envelopes.lock().len(), 1);
    assert_eq!(
        envelopes.lock()[0]
            .choices
            .iter()
            .map(|choice| choice.label.as_str())
            .collect::<Vec<_>>(),
        ["Approve", "Decline"]
    );
    assert_eq!(events.lock()[0]["actions"].as_array().unwrap().len(), 2);
    assert!(!codeterm_notification_core::persist::outlives_restart(
        store.lock().notification(&id).unwrap()
    ));
}
