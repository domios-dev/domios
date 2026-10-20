use super::*;
use serde_json::{json, Value};

#[test]
fn codex_rpc_instant_start_fresh_reply_identity_needs_no_history_or_mcp_events() {
    let reply = json!({"thread":{"id":"own", "threadSource":"user"}});
    let (thread, history) = fresh_history(&reply).unwrap();
    assert_eq!(thread, "own");
    assert!(history.messages.is_empty());
    assert!(history.active_turn.is_none());
    assert!(history.completed_turns.is_empty());
    assert!(fresh_history(&json!({"thread":{"id":"own"}})).is_ok());
    assert!(fresh_history(&json!({"thread":{"id":"own", "threadSource":"appServer"}})).is_ok());
    assert!(fresh_history(
        &json!({"thread":{"id":"own","threadSource":"user","parentThreadId":"foreign"}})
    )
    .is_err());
}

#[test]
fn codex_rpc_instant_start_accepts_optional_analytics_source_only_on_own_reply() {
    assert!(fresh_history(
        &json!({"thread":{"id":"own", "threadSource":null, "source":"appServer"}})
    )
    .is_ok());
    for reply in [
        json!({"thread":{"id":"own", "threadSource":"subagent"}}),
        json!({"thread":{"id":"own", "source":{"subAgent":{"thread_spawn":{}}}}}),
        json!({"thread":{"id":"", "threadSource":"user"}}),
        json!({"thread":{"threadSource":"user"}}),
    ] {
        assert!(fresh_history(&reply).is_err());
    }
}

#[tokio::test]
async fn codex_rpc_instant_start_remote_resume_waits_for_materialization_reply() {
    use futures_util::FutureExt;
    let reply = json!({"thread":{"id":"own"}});
    let (ack, received) = tokio::sync::oneshot::channel();
    let prepared = materialized_fresh_history(&reply, |params| {
        assert_eq!(params["threadId"], "own");
        assert_eq!(params["items"].as_array().unwrap().len(), 1);
        assert_eq!(params["items"][0]["role"], "developer");
        assert_eq!(params["items"][0]["content"][0]["text"], BINDING_MARKER);
        async move { received.await.unwrap() }
    });
    tokio::pin!(prepared);
    assert!(prepared.as_mut().now_or_never().is_none());
    ack.send(Ok(json!({}))).unwrap();
    assert_eq!(prepared.await.unwrap().0, "own");
    let failed =
        materialized_fresh_history(&reply, |_| async { Err("materialization rejected".into()) })
            .await;
    assert_eq!(failed.err().as_deref(), Some("materialization rejected"));
}

#[test]
fn codex_rpc_instant_start_carries_resolved_model_effort_and_reviewer_explicitly() {
    let spawn = Spawn {
        provider: "opaque-provider".into(),
        cwd: "/cwd".into(),
        instructions: None,
        resume: None,
        model: Some("selected-model".into()),
        effort: Some("chosen-effort".into()),
        full_access: false,
        host_owned: crate::agent::DOMIOS_HOST_OWNED_CAPABILITIES,
    };
    let params = parameters(&spawn, RuntimeMode::Auto);
    assert_eq!(params["model"], "selected-model");
    assert_eq!(params["threadSource"], "user");
    assert_eq!(params["config"]["model_reasoning_effort"], "chosen-effort");
    assert_eq!(params["approvalsReviewer"], "auto_review");
    assert_eq!(params["approvalPolicy"], "on-request");
    assert_eq!(params["sandbox"], "workspace-write");
}

#[test]
fn native_thread_start_host_owned_config_matches_runtime_fixture() {
    const FIXTURE: &str =
        include_str!("../../../../../../../tests/fixtures/app-server-host-owned-config.json");
    let expected: Value = serde_json::from_str(FIXTURE).unwrap();
    let spawn = Spawn {
        provider: "opaque-provider".into(),
        cwd: "/cwd".into(),
        instructions: None,
        resume: None,
        model: None,
        effort: None,
        full_access: false,
        host_owned: crate::agent::DOMIOS_HOST_OWNED_CAPABILITIES,
    };
    let params = parameters(&spawn, RuntimeMode::Auto);
    let actual = params["config"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(key, _)| key.starts_with("features."))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<serde_json::Map<_, _>>();
    assert_eq!(actual, expected.as_object().unwrap().clone());
}
