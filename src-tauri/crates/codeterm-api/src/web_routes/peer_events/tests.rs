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
fn peer_plan_topics_follow_read_plan_grant() {
    for event_type in ["plan-changed", "plans-loaded"] {
        let event = SystemEvent::Generic {
            event_type: event_type.into(),
            data: serde_json::json!({"plan_id":"plan-B"}),
        };
        assert!(topic_allowed(&AccessLevel::Full, &event, |_| false));
        assert!(topic_allowed(&AccessLevel::ReadOnly, &event, |_| false));
        assert!(!topic_allowed(&AccessLevel::AgentIsolated, &event, |_| {
            true
        }));
        assert!(!topic_allowed(
            &AccessLevel::Custom(vec![
                codeterm_service_core::mesh::types::Permission::ReadTabs
            ]),
            &event,
            |_| true
        ));
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

#[tokio::test]
async fn peer_appearance_and_config_topics_follow_read_host_and_strip_bodies() {
    use codeterm_service_core::mesh::types::Permission;
    let commit = SystemEvent::Generic {
        event_type: "appearance-committed".into(),
        data: serde_json::json!({
            "event": "evt-1",
            "machine": "peer-a",
            "proposal": "prop-1",
            "undo": true,
            "look": { "theme": { "name": "Nord" }, "huge": "x".repeat(100_000) }
        }),
    };
    let config = SystemEvent::Generic {
        event_type: "config-changed".into(),
        data: serde_json::json!({ "theme": { "name": "Nord" }, "huge": "y".repeat(100_000) }),
    };
    for event in [&commit, &config] {
        assert!(topic_allowed(&AccessLevel::Full, event, |_| false));
        assert!(topic_allowed(&AccessLevel::ReadOnly, event, |_| false));
        assert!(!topic_allowed(&AccessLevel::AgentIsolated, event, |_| true));
        assert!(!topic_allowed(
            &AccessLevel::Custom(vec![Permission::ReadTabs]),
            event,
            |_| true,
        ));
    }

    let events = Arc::new(crate::events::EventBroadcaster::new());
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
    events.publish(commit);
    events.publish(config);

    let mut frames = Vec::new();
    for _ in 0..3 {
        let next = tokio::time::timeout(std::time::Duration::from_secs(2), body.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let text = std::str::from_utf8(&next).unwrap().trim();
        let frame: serde_json::Value =
            serde_json::from_str(text.strip_prefix("data: ").unwrap()).unwrap();
        frames.push(frame);
    }
    assert_eq!(frames[0]["event_type"], "peer-replay-complete");
    assert_eq!(frames[1]["event_type"], "appearance-committed");
    assert_eq!(
        frames[1]["data"],
        serde_json::json!({
            "event": "evt-1",
            "machine": "peer-a",
            "proposal": "prop-1",
            "undo": true,
        })
    );
    assert_eq!(frames[2]["event_type"], "config-changed");
    assert_eq!(frames[2]["data"], serde_json::json!({}));
}

#[tokio::test]
async fn peer_provider_cli_topics_follow_read_host_and_strip_status_bodies() {
    use codeterm_service_core::mesh::types::Permission;
    let started = SystemEvent::Generic {
        event_type: "provider-cli-started".into(),
        data: serde_json::json!({ "provider": "fixture", "tab_id": "cli-tab" }),
    };
    let completed = SystemEvent::Generic {
        event_type: "provider-cli-completed".into(),
        data: serde_json::json!({
            "provider": "fixture",
            "operation": "install",
            "tab_id": "installer-tab",
            "success": true,
            "summary": "fixture CLI updated to 2.0.0",
            "status": { "searched": vec!["x".repeat(50_000)] },
        }),
    };
    let available = SystemEvent::Generic {
        event_type: "provider-cli-update-available".into(),
        data: serde_json::json!({ "provider": "fixture" }),
    };
    for event in [&started, &completed, &available] {
        assert!(topic_allowed(&AccessLevel::Full, event, |_| false));
        assert!(topic_allowed(&AccessLevel::ReadOnly, event, |_| false));
        assert!(!topic_allowed(&AccessLevel::AgentIsolated, event, |_| true));
        assert!(!topic_allowed(
            &AccessLevel::Custom(vec![Permission::ReadTabs]),
            event,
            |_| true,
        ));
    }

    let events = Arc::new(crate::events::EventBroadcaster::new());
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
    events.publish(started);
    events.publish(completed);
    events.publish(available);

    let mut frames = Vec::new();
    for _ in 0..4 {
        let next = tokio::time::timeout(std::time::Duration::from_secs(2), body.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let text = std::str::from_utf8(&next).unwrap().trim();
        let frame: serde_json::Value =
            serde_json::from_str(text.strip_prefix("data: ").unwrap()).unwrap();
        frames.push(frame);
    }
    assert_eq!(frames[0]["event_type"], "peer-replay-complete");
    assert_eq!(frames[1]["event_type"], "provider-cli-started");
    assert_eq!(
        frames[1]["data"],
        serde_json::json!({ "provider": "fixture" })
    );
    assert_eq!(frames[2]["event_type"], "provider-cli-completed");
    assert_eq!(
        frames[2]["data"],
        serde_json::json!({
            "provider": "fixture",
            "operation": "install",
            "tab_id": "installer-tab",
            "summary": "fixture CLI updated to 2.0.0",
            "success": true,
        })
    );
    assert_eq!(frames[3]["event_type"], "provider-cli-update-available");
    assert_eq!(
        frames[3]["data"],
        serde_json::json!({ "provider": "fixture" })
    );
}

#[test]
fn secure_input_changed_event_is_visible_only_to_local_owner() {
    let (mux, tab) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    let event = SystemEvent::Generic {
        event_type: codeterm_service_core::service::secure_input::SECURE_INPUT_CHANGED_EVENT.into(),
        data: serde_json::json!({}),
    };
    let tab_caller =
        CallerIdentity::Tab(codeterm_service_core::service::tab_credentials::TabCaller {
            tab_id: tab,
            binding_nonce: "nonce".into(),
        });
    assert!(visible(
        &state,
        Some(&CallerIdentity::Local),
        None,
        None,
        &event,
    ));
    assert!(!visible(&state, Some(&tab_caller), None, None, &event));
    assert!(!visible(&state, None, None, None, &event));
    assert!(!visible(
        &state,
        Some(&CallerIdentity::Peer {
            instance_id: "peer-1".into(),
            access_level: AccessLevel::Full,
            relay_protocol_version: 2,
        }),
        Some(PeerEventScope::Peer),
        None,
        &event,
    ));
}
