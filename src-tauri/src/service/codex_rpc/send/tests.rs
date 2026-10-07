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
async fn codex_rpc_send_active_user_holds_without_any_rpc_write() {
    let (result, calls) = drive(Origin::User, Some("active-a"), vec![]).await;
    assert!(result.is_err());
    assert!(calls.is_empty());
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

#[test]
fn codex_rpc_send_user_admission_holds_during_notification_gap() {
    assert!(user_turn_pending(false, true));
    assert!(user_turn_pending(true, false));
    assert!(user_turn_pending(true, true));
    assert!(!user_turn_pending(false, false));
    assert!(!busy::hold(Origin::Agent, true));
}

#[tokio::test]
async fn codex_rpc_send_uncertain_user_start_never_replays() {
    for error in [
        ClientError::Deadline,
        ClientError::Closed("writer died".into()),
        ClientError::Rpc(RpcError {
            code: -32600,
            message: "opaque error".into(),
            data: None,
        }),
    ] {
        let (result, calls) = drive(Origin::User, None, vec![Err(error.clone())]).await;
        assert_eq!(result, Err(error));
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "turn/start");
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

#[tokio::test]
async fn codex_rpc_send_busy_messages_survive_interrupt_as_one_exact_next_turn() {
    use crate::service::{
        chat_send_transaction::{ChatSendLedger, ChatSendState, SendLiveBinding},
        codex_rpc::control::ControlState,
    };
    for interrupted in [false, true] {
        let mut control = ControlState::default();
        control.started("thread-a", "old-turn");
        let ledger = ChatSendLedger::default();
        let messages = ["one", "two", "three", "four"];
        let ids = messages
            .iter()
            .map(|text| ledger.try_start("tab-a", text, None).unwrap().send_id)
            .collect::<Vec<_>>();
        assert!(user_turn_pending(
            control.active.contains_key("thread-a"),
            false
        ));
        let (held, writes) = drive(Origin::User, Some("old-turn"), vec![]).await;
        assert!(held.is_err());
        assert!(writes.is_empty());
        for id in &ids {
            assert_eq!(ledger.status(id).unwrap().state, ChatSendState::Pending);
        }
        // Interrupt acknowledgement alone is not settlement; only the root completion opens admission.
        if interrupted {
            assert!(control.active.contains_key("thread-a"));
        }
        assert!(control.completed("thread-a", "old-turn"));
        assert!(!user_turn_pending(
            control.active.contains_key("thread-a"),
            false
        ));
        let ids_ref = ids.iter().map(String::as_str).collect::<Vec<_>>();
        assert!(ledger.bind_rpc_group(
            &ids_ref,
            SendLiveBinding {
                session_id: Some("thread-a".into()),
                binding_nonce: Some("nonce-a".into())
            }
        ));
        let text = joined_text(messages.iter().map(|text| text.to_string()));
        let params = start_params("thread-a", &ids[0], &text, None, json!({}));
        let mut calls = Vec::new();
        let result = submit(Origin::User, None, params, |method, params| {
            calls.push((method, params));
            std::future::ready(Ok(json!({"turn":{"id":"next-turn"}})))
        })
        .await;
        assert!(result.is_ok());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "turn/start");
        assert_eq!(calls[0].1["input"][0]["text"], messages.join("\n\n"));
        for id in &ids {
            ledger.mark_rpc_accepted(id);
        }
        assert!(ledger
            .confirm_rpc_group("tab-a", "foreign-thread", "nonce-a", &ids[0])
            .is_empty());
        assert!(ledger
            .confirm_rpc_group("tab-a", "thread-a", "stale-nonce", &ids[0])
            .is_empty());
        assert_eq!(
            ledger
                .confirm_rpc_group("tab-a", "thread-a", "nonce-a", &ids[0])
                .len(),
            messages.len()
        );
        assert!(ledger
            .confirm_rpc_group("tab-a", "thread-a", "nonce-a", &ids[0])
            .is_empty());
        assert!(!ledger.bind_rpc_group(&ids_ref, SendLiveBinding::default()));
        for id in &ids {
            assert_eq!(ledger.status(id).unwrap().state, ChatSendState::Sent);
        }
    }
}
