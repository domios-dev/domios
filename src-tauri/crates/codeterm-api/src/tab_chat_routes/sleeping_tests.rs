use super::*;
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use std::sync::Arc;

fn register_file_provider(fixture: &ServiceFixture, runtime: bool) {
    use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};

    let path = fixture.temp.path().join("owned.jsonl");
    std::fs::write(&path, concat!(
        "{\"uuid\":\"u1\",\"role\":\"user\",\"blocks\":[{\"kind\":\"text\",\"data\":{\"text\":\"stored question\"}}]}\n",
        "{\"uuid\":\"a1\",\"role\":\"assistant\",\"blocks\":[{\"kind\":\"text\",\"data\":{\"text\":\"stored answer\"}}]}\n",
    )).unwrap();
    let mut manifest = serde_json::json!({
        "id": "snapshot-file", "version": "1.0.0", "displayName": "Snapshot fixture",
        "capabilities": {"agentProvider": true},
    });
    if runtime {
        manifest["capabilities"]["chatBackend"] = serde_json::json!({
            "spawnable": true, "sessionConfig": true, "agentTasks": true,
        });
        manifest["capabilities"]["agentRuntime"] = serde_json::json!({
            "runtime": "stdio-jsonrpc", "command": {"binary": "must-not-launch", "args": []},
            "requestMethods": {"initialize": ["handshake"]}, "notificationMethods": [],
        });
        manifest["permissions"] = serde_json::json!({
            "subprocess": {"allow": ["must-not-launch"]},
        });
    }
    let manifest = PluginManifest::from_json_str(&manifest.to_string()).unwrap();
    manifest.validate_agent_runtime().unwrap();
    let source = format!(
        r#"module.exports = {{
            sessionFilePath: (_, session) => session === 'owned-session' ? {} : null,
            parseSessionDelta: (text) => ({{messages: text.trim().split('\n').map(JSON.parse)}}),
        }};"#,
        serde_json::to_string(&path.to_string_lossy()).unwrap(),
    );
    let vm = Arc::new(PluginVm::from_source("snapshot-file", source).unwrap());
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(manifest, vm)));
}

async fn response_body(response: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn frozen_chat_and_pty_snapshots_serve_stored_messages_without_runtime() {
    for (runtime, chat_backend) in [(true, true), (true, false), (false, false)] {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.frozen = true;
        tab.restored_from_snapshot = true;
        tab.agent_type = Some("snapshot-file".into());
        tab.agent_session_id = Some("owned-session".into());
        tab.session_binding_nonce = Some("owned-nonce".into());
        if chat_backend {
            tab.backing = TabBacking::ChatBackend {
                plugin_id: "snapshot-file".into(),
                session_id: "owned-session".into(),
            };
        }
        let fixture = ServiceFixture::new(mux);
        register_file_provider(&fixture, runtime);
        for (since, expected) in [
            (0, vec!["stored question", "stored answer"]),
            (1, vec!["stored answer"]),
        ] {
            let response = snapshot(
                AxumState(crate::transport_tabs::fixture::app_state(&fixture)),
                Path(tab_id.clone()),
                Query(ChatQuery { since }),
                HeaderMap::new(),
            )
            .await
            .unwrap()
            .into_response();
            assert_eq!(response.status(), StatusCode::OK);
            let body = response_body(response).await;
            assert_eq!(body["pending"], false);
            assert_eq!(body["count"], 2);
            let content: Vec<_> = body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .map(|message| message["content"].as_str().unwrap())
                .collect();
            assert_eq!(content, expected);
            let mux = fixture.service.mux.read();
            let tab = mux.get_tab(&tab_id).unwrap();
            assert!(tab.frozen);
            assert!(tab.restored_from_snapshot);
            assert!(tab.pty_id.is_none());
            assert_eq!(tab.agent_session_id.as_deref(), Some("owned-session"));
            assert_eq!(tab.session_binding_nonce.as_deref(), Some("owned-nonce"));
            assert!(
                codeterm_service_core::service::agent_runtime::engine::for_tab(&tab_id).is_none()
            );
        }
    }
}

#[tokio::test]
async fn unknown_snapshot_failure_returns_its_reason_in_typed_json() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.frozen = true;
    tab.agent_type = Some("missing-provider".into());
    tab.agent_session_id = Some("owned-session".into());
    let fixture = ServiceFixture::new(mux);
    let response = snapshot(
        AxumState(crate::transport_tabs::fixture::app_state(&fixture)),
        Path(tab_id),
        Query(ChatQuery { since: 0 }),
        HeaderMap::new(),
    )
    .await
    .unwrap_err()
    .into_response();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        response_body(response).await,
        serde_json::json!({
            "code": "chat_snapshot_failed", "state": "failed",
            "error": "provider has no JSONL and no structured chat",
        })
    );
}

#[tokio::test]
async fn runtime_without_a_session_returns_pending_waking_without_launching() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("snapshot-file".into());
    tab.backing = TabBacking::ChatBackend {
        plugin_id: "snapshot-file".into(),
        session_id: String::new(),
    };
    let fixture = ServiceFixture::new(mux);
    register_file_provider(&fixture, true);
    let response = snapshot(
        AxumState(crate::transport_tabs::fixture::app_state(&fixture)),
        Path(tab_id.clone()),
        Query(ChatQuery { since: 0 }),
        HeaderMap::new(),
    )
    .await
    .unwrap()
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response_body(response).await,
        serde_json::json!({
            "messages": [], "count": 0, "pending": true, "state": "waking",
        })
    );
    assert!(codeterm_service_core::service::agent_runtime::engine::for_tab(&tab_id).is_none());
}
