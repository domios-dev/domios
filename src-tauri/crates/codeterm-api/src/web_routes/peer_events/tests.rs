use super::*;
use axum::response::IntoResponse;
use futures_util::StreamExt;
use std::sync::Arc;

fn topic_allowed(
    access: &AccessLevel,
    event: &SystemEvent,
    owns_tab: impl FnOnce(&str) -> bool,
) -> bool {
    topic_scope(event).is_some_and(|scope| scope.allowed_with_owner(access, owns_tab))
}

fn status(tab: &str) -> SystemEvent {
    SystemEvent::Generic {
        event_type: "tab-status-changed".into(),
        data: serde_json::json!({"tab_id": tab, "status": "Working"}),
    }
}

#[test]
fn peer_event_topics_follow_live_tab_and_mux_authority() {
    let owns = |tab: &str| tab == "owned";
    let own = status("owned");
    let foreign = status("foreign");
    let mux = SystemEvent::MuxStateChanged;
    let unrelated = SystemEvent::Generic {
        event_type: "notification-created".into(),
        data: serde_json::json!({"tab_id": "owned"}),
    };
    for access in [AccessLevel::Full, AccessLevel::ReadOnly] {
        assert!(topic_allowed(&access, &own, owns));
        assert!(topic_allowed(&access, &foreign, owns));
        assert!(topic_allowed(&access, &mux, owns));
        assert!(!topic_allowed(&access, &unrelated, owns));
    }
    let isolated = AccessLevel::AgentIsolated;
    assert!(topic_allowed(&isolated, &own, owns));
    assert!(!topic_allowed(&isolated, &foreign, owns));
    assert!(!topic_allowed(&isolated, &mux, owns));
    assert!(!topic_allowed(&isolated, &unrelated, owns));
    assert!(!topic_allowed(&AccessLevel::Custom(vec![]), &own, owns));
    let chat = SystemEvent::Generic {
        event_type: "chat-send-status".into(),
        data: serde_json::json!({"tab_id": "owned"}),
    };
    use codeterm_service_core::mesh::types::Permission;
    assert!(topic_allowed(&isolated, &chat, owns));
    assert!(!topic_allowed(&isolated, &chat, |_| false));
    assert!(topic_allowed(&AccessLevel::ReadOnly, &chat, owns));
    assert!(!topic_allowed(
        &AccessLevel::Custom(vec![Permission::ReadTabs]),
        &chat,
        owns,
    ));
}

#[tokio::test]
async fn peer_event_stream_replays_every_tab_then_marks_live_boundary() {
    let events = Arc::new(crate::events::EventBroadcaster::new());
    events.publish(status("first"));
    events.publish(status("second"));
    events.publish(SystemEvent::MuxStateChanged);
    let census = crate::stream_census::StreamCensus::default();
    let stream = super::super::events_sse_filtered(
        events.clone(),
        EventsStreamQuery {
            since: Some(format!("{}:0", events.boot_id())),
            tab_id: None,
            scope: Some(PeerEventScope::Peer),
        },
        &census,
        |event| topic_allowed(&AccessLevel::Full, event, |_| false),
    );
    let mut body = stream.into_response().into_body().into_data_stream();
    events.publish(status("live"));
    events.publish(SystemEvent::Generic {
        event_type: "chat-send-status".into(),
        data: serde_json::json!({"tab_id": "live", "system_send": { "content": "x".repeat(100_000) }}),
    });
    for (kind, tab) in [
        ("tab-status-changed", Some("first")),
        ("tab-status-changed", Some("second")),
        ("mux-state-changed", None),
        ("peer-replay-complete", None),
        ("tab-status-changed", Some("live")),
        ("chat-send-status", Some("live")),
    ] {
        let next = tokio::time::timeout(std::time::Duration::from_secs(2), body.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let text = std::str::from_utf8(&next).unwrap().trim();
        let frame: serde_json::Value =
            serde_json::from_str(text.strip_prefix("data: ").unwrap()).unwrap();
        assert_eq!(frame["event_type"], kind);
        assert_eq!(frame["data"]["tab_id"].as_str(), tab);
        if kind == "peer-replay-complete" {
            assert_eq!(frame["seq"], 3);
            assert_eq!(frame["boot"], events.boot_id());
        }
        if kind == "chat-send-status" {
            assert_eq!(frame["prev_seq"], 4);
            assert_eq!(frame["seq"], 5);
            assert_eq!(frame["data"], serde_json::json!({ "tab_id": "live" }));
        }
    }
    assert!(super::super::event_matches_tab(
        &status("first"),
        Some("first")
    ));
    assert!(!super::super::event_matches_tab(
        &status("second"),
        Some("first")
    ));
    assert!(!super::super::event_matches_tab(
        &SystemEvent::MuxStateChanged,
        Some("first")
    ));
}

#[test]
fn scoped_tab_topics_bind_topic_class_and_exact_tab() {
    use codeterm_service_core::mesh::types::Permission;
    let event = |name: &str| SystemEvent::Generic {
        event_type: name.into(),
        data: serde_json::json!({"tab_id": "owned"}),
    };
    let tabs = AccessLevel::Custom(vec![Permission::ReadTabs]);
    let output = AccessLevel::Custom(vec![Permission::ReadOutput]);
    let metadata = event("provider-switch-status");
    let chat = event("tab-chat-msg-owned");
    assert!(tab_topic(&metadata, "owned").unwrap().allowed(&tabs));
    assert!(!tab_topic(&metadata, "owned").unwrap().allowed(&output));
    assert!(tab_topic(&chat, "owned").unwrap().allowed(&output));
    assert!(!tab_topic(&chat, "owned").unwrap().allowed(&tabs));
    assert!(tab_topic(&chat, "other").is_none());
    assert!(tab_topic(&event("notification-created"), "owned").is_none());
}
