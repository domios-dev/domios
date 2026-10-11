use super::super::protocol::RpcError;
use super::*;
use std::collections::VecDeque;

fn capture_authority() -> UserSendAuthority {
    UserSendAuthority {
        tab_id: "tab-a".into(),
        pty_id: Some("pty-a".into()),
        binding_nonce: "nonce-a".into(),
        pty_session_generation: Some(7),
        provider_id: "provider-a".into(),
        session_id: Some("thread-a".into()),
        observed_session_id: Some("thread-a".into()),
        prelaunch: false,
    }
}

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
async fn lark_recording_contract_rpc_capture_ticket_denies_stale_identity_before_request() {
    #[derive(Clone, Copy)]
    enum State {
        Cancelled,
        Revoked,
        Unloaded,
        DeviceLost,
        Expired,
        ReboundSession,
        ReboundNonce,
    }

    let expected = capture_authority();
    for state in [
        State::Cancelled,
        State::Revoked,
        State::Unloaded,
        State::DeviceLost,
        State::Expired,
        State::ReboundSession,
        State::ReboundNonce,
    ] {
        let mut current = expected.clone();
        let ticket_matches = match state {
            State::Cancelled
            | State::Revoked
            | State::Unloaded
            | State::DeviceLost
            | State::Expired => false,
            State::ReboundSession => {
                current.session_id = Some("replacement-thread".into());
                true
            }
            State::ReboundNonce => {
                current.binding_nonce = "replacement-nonce".into();
                true
            }
        };
        let mut calls = Vec::new();
        let outcome = submit_guarded(
            Origin::User,
            None,
            start_params(
                "thread-a",
                "capture-client",
                "voice marker",
                None,
                json!({}),
            ),
            |_, _| {
                capture_rpc_identity_matches(
                    ticket_matches,
                    Some(&current),
                    &expected,
                    "thread-a",
                    "nonce-a",
                )
            },
            |method, params| {
                calls.push((method, params));
                std::future::ready(Ok(json!({"turn":{"id":"unexpected"}})))
            },
        )
        .await;
        assert_eq!(outcome, GuardedSubmit::RefusedBeforeRequest);
        assert!(calls.is_empty());
        let mut pastes = 0;
        let refused = DeliveryError::TabBusyRefused {
            tab: "tab-a".into(),
            detail: "capture ticket stale".into(),
        };
        let _ = route_or_pty(Some(Err(refused)), || {
            pastes += 1;
            Ok(())
        });
        assert_eq!(pastes, 0);
    }
}

#[tokio::test]
async fn lark_recording_contract_rpc_capture_valid_ticket_tracks_one_request() {
    let expected = capture_authority();
    let mut calls = Vec::new();
    let outcome = submit_guarded(
        Origin::User,
        None,
        start_params(
            "thread-a",
            "capture-client",
            "voice marker",
            None,
            json!({}),
        ),
        |_, _| {
            capture_rpc_identity_matches(true, Some(&expected), &expected, "thread-a", "nonce-a")
        },
        |method, params| {
            calls.push((method, params));
            std::future::ready(Ok(json!({"turn":{"id":"turn-a"}})))
        },
    )
    .await;
    assert!(matches!(outcome, GuardedSubmit::Attempted(Ok(_))));
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "turn/start");
    assert_eq!(calls[0].1["clientUserMessageId"], "capture-client");
    assert_eq!(calls[0].1["threadId"], "thread-a");
}

#[tokio::test]
async fn lark_recording_contract_rpc_capture_uncertainty_never_pastes_or_replays() {
    let expected = capture_authority();
    let mut calls = Vec::new();
    let outcome = submit_guarded(
        Origin::User,
        None,
        start_params(
            "thread-a",
            "capture-client",
            "voice marker",
            None,
            json!({}),
        ),
        |_, _| {
            capture_rpc_identity_matches(true, Some(&expected), &expected, "thread-a", "nonce-a")
        },
        |method, params| {
            calls.push((method, params));
            std::future::ready(Err(ClientError::Deadline))
        },
    )
    .await;
    assert_eq!(
        outcome,
        GuardedSubmit::Attempted(Err(ClientError::Deadline))
    );
    assert_eq!(calls.len(), 1);
    let mut pastes = 0;
    let uncertain = DeliveryError::SubmitUnconfirmed {
        tab: "tab-a".into(),
        detail: "RPC handoff uncertain".into(),
    };
    let _ = route_or_pty(Some(Err(uncertain)), || {
        pastes += 1;
        Ok(())
    });
    assert_eq!(pastes, 0);
}

#[tokio::test]
async fn lark_recording_contract_rpc_capture_hold_does_not_fall_back_to_paste() {
    for gate in [
        SendGate::classify(
            Some(true),
            super::super::lifecycle::DeliveryState::Pending,
            true,
            true,
        ),
        SendGate::classify(
            Some(true),
            super::super::lifecycle::DeliveryState::Ready,
            false,
            true,
        ),
    ] {
        assert!(matches!(gate, SendGate::Hold(_)));
        let mut pastes = 0;
        let held = DeliveryError::TabBusyRefused {
            tab: "tab-a".into(),
            detail: "structured attachment pending".into(),
        };
        let _ = route_or_pty(Some(Err(held)), || {
            pastes += 1;
            Ok(())
        });
        assert_eq!(pastes, 0);
    }
}

#[tokio::test]
async fn codex_rpc_send_active_user_steers_exactly_once() {
    let (result, calls) = drive(
        Origin::User,
        Some("active-a"),
        vec![Ok(json!({"turnId":"active-a"}))],
    )
    .await;
    assert!(result.is_ok());
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "turn/steer");
    assert_eq!(
        calls[0].1,
        json!({"threadId":"thread-a", "expectedTurnId":"active-a",
        "clientUserMessageId":"send-a", "input":[{"type":"text","text":"opaque payload"}]})
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

fn refused(message: &str) -> ClientError {
    ClientError::Rpc(RpcError {
        code: -32600,
        message: message.into(),
        data: None,
    })
}

#[tokio::test]
async fn codex_rpc_send_refused_ended_turn_starts_same_message_once() {
    for error in [
        refused("no active turn to steer"),
        refused("expected active turn id `active-a` but found `active-b`"),
    ] {
        let (result, calls) = drive(
            Origin::User,
            Some("active-a"),
            vec![Err(error), Ok(json!({"turn":{"id":"new"}}))],
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "turn/steer");
        assert_eq!(calls[1].0, "turn/start");
        assert_eq!(
            calls[0].1["clientUserMessageId"],
            calls[1].1["clientUserMessageId"]
        );
        assert_eq!(calls[0].1["input"], calls[1].1["input"]);
        assert_eq!(calls[1].1["cwd"], "/workspace");
        assert!(calls[1].1.get("expectedTurnId").is_none());
    }
}

#[tokio::test]
async fn codex_rpc_send_uncertain_or_unrelated_steer_error_never_replays() {
    for error in [
        ClientError::Deadline,
        ClientError::Closed("writer died".into()),
        refused("cannot steer a compact turn"),
        refused("opaque refusal"),
        ClientError::Rpc(RpcError {
            code: -32603,
            message: "no active turn to steer".into(),
            data: None,
        }),
        ClientError::Rpc(RpcError {
            code: -32600,
            message: "no active turn to steer".into(),
            data: Some(json!({"opaque":"data"})),
        }),
    ] {
        let (result, calls) = drive(Origin::User, Some("active-a"), vec![Err(error.clone())]).await;
        assert_eq!(result, Err(error));
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "turn/steer");
    }
}

#[tokio::test]
async fn codex_rpc_send_two_busy_messages_are_two_independently_confirmed_steers() {
    use crate::service::chat_send_transaction::{ChatSendLedger, ChatSendState};
    let ledger = ChatSendLedger::default();
    let mut calls = Vec::new();
    let mut ids = Vec::new();
    for text in ["one\nwith whitespace  ", "two"] {
        let id = ledger.try_start("tab-a", text, None).unwrap().send_id;
        assert!(ledger.bind_rpc_send(
            &id,
            SendLiveBinding {
                session_id: Some("thread-a".into()),
                binding_nonce: Some("nonce-a".into())
            }
        ));
        let reply = submit(
            Origin::User,
            Some("active-a"),
            start_params("thread-a", &id, text, None, json!({})),
            |method, params| {
                calls.push((method, params));
                std::future::ready(Ok(json!({"turnId":"active-a"})))
            },
        )
        .await;
        assert!(reply.is_ok());
        ledger.mark_rpc_accepted(&id);
        ids.push(id);
    }
    assert_eq!(calls.len(), 2);
    for (index, text) in ["one\nwith whitespace  ", "two"].iter().enumerate() {
        assert_eq!(calls[index].0, "turn/steer");
        assert_eq!(
            calls[index].1["input"],
            json!([{"type":"text","text":text}])
        );
        assert_eq!(calls[index].1["clientUserMessageId"], ids[index]);
    }
    assert!(ledger
        .confirm_rpc_send("tab-a", "foreign", "nonce-a", &ids[0])
        .is_none());
    assert!(ledger
        .confirm_rpc_send("tab-a", "thread-a", "stale", &ids[0])
        .is_none());
    assert!(ledger
        .confirm_rpc_send("tab-a", "thread-a", "nonce-a", &ids[0])
        .is_some());
    assert_eq!(
        ledger.status(&ids[1]).unwrap().state,
        ChatSendState::Written
    );
    assert!(ledger
        .confirm_rpc_send("tab-a", "thread-a", "nonce-a", &ids[0])
        .is_none());
    assert!(ledger
        .confirm_rpc_send("tab-a", "thread-a", "nonce-a", &ids[1])
        .is_some());
}
