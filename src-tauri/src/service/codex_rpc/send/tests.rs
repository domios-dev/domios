use super::super::protocol::RpcError;
use super::*;
use std::collections::VecDeque;

async fn drive(
    origin: Origin,
    active: Option<&str>,
    replies: Vec<Result<Value, ClientError>>,
) -> (Result<Value, ClientError>, Vec<(&'static str, Value)>) {
    let mut replies = VecDeque::from(replies);
    let mut calls = Vec::new();
    let result = submit(
        origin,
        active,
        start_params(
            "thread-a",
            "send-a",
            "opaque payload",
            Some("/workspace"),
            json!({}),
        ),
        |method, params| {
            calls.push((method, params));
            std::future::ready(replies.pop_front().expect("unexpected payload replay"))
        },
    )
    .await;
    assert!(replies.is_empty());
    (result, calls)
}

#[tokio::test]
async fn codex_rpc_send_idle_user_starts_exactly_once() {
    let (result, calls) = drive(
        Origin::User,
        None,
        vec![Ok(json!({"turn":{"id":"turn-a"}}))],
    )
    .await;
    assert!(result.is_ok());
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "turn/start");
    assert_eq!(calls[0].1["clientUserMessageId"], "send-a");
    assert_eq!(
        calls[0].1["input"],
        json!([{"type":"text","text":"opaque payload"}])
    );
}

#[tokio::test]
async fn codex_rpc_send_active_user_steers_with_expected_turn_and_client_id() {
    let (_, calls) = drive(Origin::User, Some("active-a"), vec![Ok(json!({}))]).await;
    assert_eq!(
        calls,
        vec![(
            "turn/steer",
            json!({"threadId":"thread-a","expectedTurnId":"active-a",
        "clientUserMessageId":"send-a","input":[{"type":"text","text":"opaque payload"}]})
        )]
    );
}

#[tokio::test]
async fn codex_rpc_send_active_agent_queues_one_new_turn() {
    let queued = json!({"turn":{"id":"queued-a","startedAt":null}});
    let (result, calls) = drive(Origin::Agent, Some("active-a"), vec![Ok(queued.clone())]).await;
    assert_eq!(result, Ok(queued));
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "turn/start");
    assert!(calls[0].1.get("expectedTurnId").is_none());
}

#[tokio::test]
async fn codex_rpc_send_nonsteerable_rejection_queues_once_with_same_client_id() {
    let error = ClientError::Rpc(RpcError {
        code: -32000,
        message: "opaque error".into(),
        data: Some(json!({"codexErrorInfo":{"activeTurnNotSteerable":{"turnKind":"review"}}})),
    });
    let (result, calls) = drive(
        Origin::User,
        Some("active-a"),
        vec![Err(error), Ok(json!({"turn":{"startedAt":null}}))],
    )
    .await;
    assert!(result.is_ok());
    assert_eq!(
        calls.iter().map(|call| call.0).collect::<Vec<_>>(),
        ["turn/steer", "turn/start"]
    );
    assert_eq!(
        calls[0].1["clientUserMessageId"],
        calls[1].1["clientUserMessageId"]
    );
    assert_eq!(calls[0].1["input"], calls[1].1["input"]);
}

#[tokio::test]
async fn codex_rpc_send_uncertain_steer_never_starts_or_replays() {
    for error in [
        ClientError::Deadline,
        ClientError::Closed("writer died".into()),
        ClientError::Rpc(RpcError {
            code: -32600,
            message: "activeTurnNotSteerable".into(),
            data: None,
        }),
    ] {
        let (result, calls) = drive(Origin::User, Some("active-a"), vec![Err(error.clone())]).await;
        assert_eq!(result, Err(error));
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "turn/steer");
    }
}

#[tokio::test]
async fn codex_rpc_send_uncertain_start_never_replays() {
    let (result, calls) = drive(Origin::Agent, None, vec![Err(ClientError::Deadline)]).await;
    assert_eq!(result, Err(ClientError::Deadline));
    assert_eq!(calls.len(), 1);
}

#[test]
fn codex_rpc_send_overrides_keep_reviewer_sandbox_cwd_model_and_effort() {
    let params = start_params(
        "thread-a",
        "id-a",
        "payload",
        Some("/cwd"),
        json!({
            "model":"model-a", "effort":"high", "approvalPolicy":"on-request",
            "approvalsReviewer":"auto_review", "sandboxPolicy":{"type":"workspaceWrite"}
        }),
    );
    assert_eq!(params["approvalsReviewer"], "auto_review");
    assert_eq!(params["sandboxPolicy"], json!({"type":"workspaceWrite"}));
    assert_eq!(params["approvalPolicy"], "on-request");
    assert_eq!(params["model"], "model-a");
    assert_eq!(params["effort"], "high");
    assert_eq!(params["cwd"], "/cwd");
    assert_eq!(
        start_params("t", "i", "p", None, json!({}))["approvalsReviewer"],
        "user"
    );
    assert_eq!(
        start_params("t", "i", "p", None, json!({"approvalsReviewer":null}))["approvalsReviewer"],
        "user"
    );
}

#[test]
fn codex_rpc_send_evidence_requires_user_message_client_id_notification() {
    let params =
        json!({"threadId":"thread-a","item":{"type":"userMessage","clientId":"id-a","content":[]}});
    assert_eq!(
        user_client_id("item/started", &params),
        Some(("thread-a", "id-a"))
    );
    assert_eq!(
        user_client_id("item/completed", &params),
        Some(("thread-a", "id-a"))
    );
    assert_eq!(user_client_id("thread/resume", &params), None);
    assert_eq!(
        user_client_id(
            "item/completed",
            &json!({"threadId":"t","item":{"type":"userMessage","id":"id-a"}})
        ),
        None
    );
    assert_eq!(
        user_client_id(
            "item/completed",
            &json!({"threadId":"t","item":{"type":"agentMessage","clientId":"id-a"}})
        ),
        None
    );
}

#[tokio::test]
async fn codex_rpc_send_no_paste_after_rpc_handoff_or_uncertainty() {
    for reply in [
        Ok(json!({"turn":{"startedAt":null}})),
        Err(ClientError::Deadline),
        Err(ClientError::Closed("writer failed".into())),
    ] {
        let (result, calls) = drive(Origin::Agent, None, vec![reply]).await;
        let mut pastes = 0;
        let _ = route_or_pty(Some(result.map(|_| ())), || {
            pastes += 1;
            Ok(())
        });
        assert_eq!(pastes, 0);
        assert_eq!(calls.len(), 1);
    }
}

#[test]
fn codex_rpc_send_prewrite_unavailability_uses_pty_exactly_once() {
    let mut pastes = Vec::new();
    let result: Result<(), ClientError> = route_or_pty(None, || {
        pastes.push("opaque payload");
        Ok(())
    });
    assert_eq!(result, Ok(()));
    assert_eq!(pastes, ["opaque payload"]);
}

#[test]
fn codex_rpc_send_unbound_sidecar_holds_without_paste() {
    let held = DeliveryError::TabBusyRefused {
        tab: "tab-a".into(),
        detail: "binding pending".into(),
    };
    let result = route_or_pty(Some(Err(held)), || panic!("unbound sidecar must hold"));
    assert!(!result.unwrap_err().bytes_written());
}
