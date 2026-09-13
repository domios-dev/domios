use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::origin_from_params;
use crate::notification::store::{Ask, NotificationProvenance, NotificationStore};
use crate::service::notification::{
    create_notification_with_delivery, NotificationAdmission, PushDispatch, SurfaceDispatch,
};

const LIVE: [(&str, &str, &str); 2] = [
    ("8717c497", "Hudson", "ws_team"),
    ("04482ebb", "Sonnet snake demo", "ws_team"),
];

fn live(candidate: &str) -> Option<NotificationProvenance> {
    LIVE.iter()
        .find(|(id, _, _)| *id == candidate)
        .map(|(id, title, workspace)| NotificationProvenance {
            tab_id: Some((*id).to_string()),
            tab_title: Some((*title).to_string()),
            workspace_id: Some((*workspace).to_string()),
            peer_name: None,
        })
}

/// Admits a `notification.create` request exactly as the socket handler reads it.
fn admit(params: Value) -> (Arc<Mutex<NotificationStore>>, String) {
    let store = Arc::new(Mutex::new(NotificationStore::new(10)));
    let id = create_notification_with_delivery(
        NotificationAdmission::Explicit,
        &store,
        |_, _| {},
        "Approve?".into(),
        None,
        Some("info"),
        Ask::default(),
        None,
        origin_from_params(&Some(params)),
        live,
        PushDispatch::inert(),
        SurfaceDispatch::silent(),
        |_| {},
    );
    (store, id)
}

#[test]
fn notification_deep_link_keeps_the_asking_tab() {
    let requests = [
        json!({ "title": "Approve?", "severity": "info", "level": "action_needed",
                "caller_tab_id": "8717c497",
                "target_tab_id": "8717c497", "chat_message_id": "0-601-0" }),
        json!({ "title": "Approve?", "severity": "info", "level": "action_needed",
                "caller_tab_id": "8717c497", "target_tab_id": "deadbeef" }),
        json!({ "title": "Approve?", "severity": "info", "level": "action_needed",
                "caller_tab_id": "04482ebb" }),
    ];
    for request in requests {
        let asker = request["caller_tab_id"].as_str().unwrap().to_string();
        let (store, id) = admit(request.clone());
        let guard = store.lock();
        let record = guard.notification(&id).expect("the record is stored");
        assert_eq!(record.tab_id.as_deref(), Some(asker.as_str()), "{request}");
        assert_eq!(record.workspace_id.as_deref(), Some("ws_team"), "{request}");
    }
}

#[test]
fn notification_deep_link_target_never_stands_in_for_the_asking_tab() {
    let (store, id) = admit(json!({
        "title": "Approve?",
        "target_tab_id": "04482ebb",
        "chat_message_id": "0-601-0",
    }));
    let guard = store.lock();
    let record = guard.notification(&id).unwrap();
    assert_eq!(record.tab_id, None, "a live target tab is not provenance");
    assert_eq!(record.workspace_id, None);
    assert_eq!(record.target_tab_id.as_deref(), Some("04482ebb"));
}
