use super::super::protocol::RpcError;
use super::*;
use crate::service::{
    local_recording::leases::{Capture, FrozenTarget},
    parked_user_delivery::tests::{mark_ready, queue_fixture},
    pending_delivery_payloads::user_sends::UserSendAuthority,
    test_fixture::ServiceFixture,
};
use codeterm_local_recording::LocalRecordingTrigger;
use codeterm_plugin::local_recording::SelectedDevices;
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

fn active_capture(fixture: &ServiceFixture, tab: &str) -> (Capture, u64) {
    fixture.service.local_recording_registry.register(
        "trigger",
        LocalRecordingTrigger {
            vendor_id: 1,
            product_id: 2,
            usage_page: 3,
            usage: 4,
            button: None,
        },
        true,
    );
    let devices = SelectedDevices {
        input_id: "input".into(),
        collection_id: "collection".into(),
        microphone_id: "microphone".into(),
    };
    fixture
        .service
        .local_recording_registry
        .select_devices("trigger", devices.clone())
        .unwrap();
    fixture
        .service
        .local_recording_registry
        .set_enabled("trigger", true)
        .unwrap();
    let generation = fixture
        .service
        .local_recording_registry
        .selected_devices("trigger")
        .unwrap()
        .1;
    assert_eq!(
        crate::service::local_recording::arm_registry_fixture(
            &fixture.service.local_recording_registry,
            "trigger"
        ),
        generation
    );
    let target = fixture.service.freeze_recording_target(tab).unwrap();
    let lease = fixture
        .service
        .local_recordings
        .start(
            "trigger",
            &devices,
            generation,
            target,
            &fixture.service.local_recording_registry,
        )
        .unwrap();
    fixture
        .service
        .local_recordings
        .stopped(&lease.lease)
        .unwrap();
    let capture = fixture
        .service
        .local_recordings
        .transcribing(&lease.lease)
        .unwrap();
    (capture, generation)
}

fn admitted_capture_rpc_fixture() -> (
    ServiceFixture,
    String,
    Capture,
    u64,
    UserSendAuthority,
    String,
) {
    let (fixture, tab) = queue_fixture(true, None);
    mark_ready(&fixture, &tab);
    let (capture, generation) = active_capture(&fixture, &tab);
    let authority = fixture.service.user_send_authority(&tab).unwrap();
    assert_eq!(capture.target, FrozenTarget::Pty(authority.clone()));
    let reservation = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .reserve_with_replay_only(
            authority.clone(),
            Some("capture-client".into()),
            false,
            || fixture.service.chat_send_ledger.reserve_identity(),
        )
        .unwrap();
    let send_id = reservation.send_id.clone();
    fixture
        .service
        .local_recordings
        .register_write_ticket(&send_id, &capture.lease_id, capture.clone())
        .unwrap();
    let record = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .admit_with_follow_up(
            &reservation,
            "voice marker",
            "voice marker",
            None,
            None,
            false,
        )
        .unwrap();
    let status = fixture.service.chat_send_ledger.admit_user(&record);
    assert_eq!(status.correlation_id.as_deref(), Some("capture-client"));
    drop(reservation);
    assert!(fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .claim(&record, authority.session_id.as_deref())
        .unwrap());
    let evidence = fixture.service.chat_send_anchor(&tab);
    assert!(fixture
        .service
        .chat_send_ledger
        .begin_user_handoff(&record, "voice marker", evidence));
    assert!(fixture
        .service
        .local_recordings
        .write_ticket_matches_authority(&send_id, &authority));
    (fixture, tab, capture, generation, authority, send_id)
}

async fn submit_capture_rpc<B>(
    fixture: &ServiceFixture,
    authority: &UserSendAuthority,
    send_id: &str,
    active: Option<&str>,
    before_validation: B,
    replies: Vec<Result<Value, ClientError>>,
) -> (GuardedSubmit, Vec<(&'static str, Value)>)
where
    B: FnMut(&'static str, &Value),
{
    let mut replies = VecDeque::from(replies);
    let mut calls = Vec::new();
    let thread = authority.session_id.as_deref().unwrap();
    let outcome = submit_owned_rpc(
        &fixture.service,
        Origin::User,
        active,
        start_params(thread, send_id, "voice marker", None, json!({})),
        send_id,
        Some(authority),
        thread,
        &authority.binding_nonce,
        false,
        before_validation,
        |method, params| {
            calls.push((method, params));
            std::future::ready(replies.pop_front().expect("unexpected RPC handoff"))
        },
    )
    .await;
    (outcome, calls)
}

#[derive(Clone, Copy)]
enum CaptureRpcInvalidation {
    Cancel,
    Revoke,
    Unload,
    DeviceLoss,
    Deadline,
    Retire,
    ReboundSession,
    ReboundNonce,
}

fn invalidate_capture_rpc(
    fixture: &ServiceFixture,
    tab: &str,
    capture: &Capture,
    generation: u64,
    send_id: &str,
    invalidation: CaptureRpcInvalidation,
) {
    match invalidation {
        CaptureRpcInvalidation::Cancel => {
            fixture.service.local_recordings.cancel(&capture.lease_id)
        }
        CaptureRpcInvalidation::Revoke => {
            fixture.service.local_recording_registry.revoke("trigger")
        }
        CaptureRpcInvalidation::Unload => fixture
            .service
            .local_recording_registry
            .unregister("trigger"),
        CaptureRpcInvalidation::DeviceLoss => assert!(fixture
            .service
            .local_recording_registry
            .mark_unavailable("trigger", generation)),
        CaptureRpcInvalidation::Deadline => fixture
            .service
            .local_recordings
            .expire_write_ticket(send_id),
        CaptureRpcInvalidation::Retire => fixture
            .service
            .local_recordings
            .retire_write_ticket(send_id),
        CaptureRpcInvalidation::ReboundSession => {
            fixture
                .service
                .mux
                .write()
                .get_tab_mut(tab)
                .unwrap()
                .agent_session_id = Some("replacement-session".into());
        }
        CaptureRpcInvalidation::ReboundNonce => {
            fixture.service.rotate_session_binding_nonce(tab);
        }
    }
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
    for state in [
        CaptureRpcInvalidation::Cancel,
        CaptureRpcInvalidation::Revoke,
        CaptureRpcInvalidation::Unload,
        CaptureRpcInvalidation::DeviceLoss,
        CaptureRpcInvalidation::Deadline,
        CaptureRpcInvalidation::Retire,
        CaptureRpcInvalidation::ReboundSession,
        CaptureRpcInvalidation::ReboundNonce,
    ] {
        let (fixture, tab, capture, generation, authority, send_id) =
            admitted_capture_rpc_fixture();
        let mut boundary = Some(state);
        let (outcome, calls) = submit_capture_rpc(
            &fixture,
            &authority,
            &send_id,
            None,
            |_, _| {
                if let Some(state) = boundary.take() {
                    invalidate_capture_rpc(&fixture, &tab, &capture, generation, &send_id, state);
                }
            },
            vec![Ok(json!({"turn":{"id":"unexpected"}}))],
        )
        .await;
        assert_eq!(
            outcome,
            GuardedSubmit::RefusedBeforeRequest {
                after_known_steer_refusal: false
            }
        );
        assert!(calls.is_empty());
        assert!(!fixture.service.chat_send_ledger.has_rpc_handoff(&send_id));
        assert_eq!(
            fixture
                .service
                .chat_send_ledger
                .status(&send_id)
                .unwrap()
                .state,
            crate::service::chat_send_transaction::ChatSendState::Pending
        );
    }
}

#[tokio::test]
async fn lark_recording_contract_rpc_capture_valid_ticket_tracks_one_request() {
    let (fixture, tab, _, _, authority, send_id) = admitted_capture_rpc_fixture();
    let (outcome, calls) = submit_capture_rpc(
        &fixture,
        &authority,
        &send_id,
        None,
        |_, _| {},
        vec![Ok(json!({"turn":{"id":"turn-a"}}))],
    )
    .await;
    assert!(matches!(outcome, GuardedSubmit::Attempted(Ok(_))));
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "turn/start");
    assert_eq!(calls[0].1["clientUserMessageId"], send_id);
    assert_eq!(
        calls[0].1["threadId"],
        authority.session_id.as_deref().unwrap()
    );
    assert!(fixture.service.chat_send_ledger.has_rpc_handoff(&send_id));
    let result = match outcome {
        GuardedSubmit::Attempted(result) => result,
        GuardedSubmit::RefusedBeforeRequest { .. } => unreachable!(),
    };
    assert!(finish_handoff(
        &fixture.service,
        &tab,
        authority.session_id.as_deref().unwrap(),
        &send_id,
        result,
    )
    .is_ok());
    let status = fixture.service.chat_send_ledger.status(&send_id).unwrap();
    assert_eq!(
        status.state,
        crate::service::chat_send_transaction::ChatSendState::Written
    );
    assert_eq!(status.correlation_id.as_deref(), Some("capture-client"));
}

#[tokio::test]
async fn lark_recording_contract_rpc_capture_uncertainty_never_pastes_or_replays() {
    let (fixture, tab, _, _, authority, send_id) = admitted_capture_rpc_fixture();
    let (outcome, calls) = submit_capture_rpc(
        &fixture,
        &authority,
        &send_id,
        None,
        |_, _| {},
        vec![Err(ClientError::Deadline)],
    )
    .await;
    assert_eq!(
        outcome,
        GuardedSubmit::Attempted(Err(ClientError::Deadline))
    );
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1["clientUserMessageId"], send_id);
    assert!(fixture.service.chat_send_ledger.has_rpc_handoff(&send_id));
    let result = match outcome {
        GuardedSubmit::Attempted(result) => result,
        GuardedSubmit::RefusedBeforeRequest { .. } => unreachable!(),
    };
    assert!(matches!(
        finish_handoff(
            &fixture.service,
            &tab,
            authority.session_id.as_deref().unwrap(),
            &send_id,
            result,
        ),
        Err(DeliveryError::SubmitUnconfirmed { .. })
    ));
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&send_id)
            .unwrap()
            .state,
        crate::service::chat_send_transaction::ChatSendState::Unknown
    );
}

#[tokio::test]
async fn lark_recording_contract_rpc_capture_cancel_after_steer_refusal_blocks_start() {
    let (fixture, _, capture, _, authority, send_id) = admitted_capture_rpc_fixture();
    let mut invalidated = false;
    let (outcome, calls) = submit_capture_rpc(
        &fixture,
        &authority,
        &send_id,
        Some("active-turn"),
        |method, _| {
            if method == "turn/start" && !invalidated {
                fixture.service.local_recordings.cancel(&capture.lease_id);
                invalidated = true;
            }
        },
        vec![
            Err(refused("no active turn to steer")),
            Ok(json!({"turn":{"id":"must-not-start"}})),
        ],
    )
    .await;
    assert!(matches!(
        outcome,
        GuardedSubmit::RefusedBeforeRequest {
            after_known_steer_refusal: true
        }
    ));
    assert!(invalidated);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "turn/steer");
    assert!(!calls.iter().any(|(method, _)| *method == "turn/start"));
    assert!(!fixture.service.chat_send_ledger.has_rpc_handoff(&send_id));
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&send_id)
            .unwrap()
            .state,
        crate::service::chat_send_transaction::ChatSendState::Pending
    );
}

#[tokio::test]
async fn lark_recording_contract_rpc_capture_hold_does_not_fall_back_to_paste() {
    for gate in [
        SendGate::classify(
            Some(true),
            super::super::lifecycle::DeliveryState::Pending,
            true,
            true,
            false,
        ),
        SendGate::classify(
            Some(true),
            super::super::lifecycle::DeliveryState::Ready,
            false,
            true,
            true,
        ),
    ] {
        assert!(matches!(gate, SendGate::Refuse(_)));
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
