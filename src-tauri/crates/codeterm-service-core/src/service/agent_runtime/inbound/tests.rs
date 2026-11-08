use super::super::engine::test_support::runtime_fixture_turn_scoped;
use super::*;
use crate::service::parked_user_delivery::UserDrain;
use serde_json::{json, Value};
use std::{
    pin::Pin,
    task::{Context, Poll, Waker},
    time::Duration,
};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

const SOURCE: &str = r#"
module.exports = {
  runtimePlan(ctx, op) {
    const turn = op.activeTurnId || op.sendId;
    return {action:{kind:'request',call:{method:'fixture/submit',params:{threadId:op.threadId,sendId:op.sendId,text:op.input.text},target:{threadId:op.threadId,turnId:turn}}},state:null};
  },
  runtimeResponse(ctx, op, state, reply) {
    if (reply.kind === 'error') return {kind:'refused',reason:{code:'provider_refused',message:reply.error.message},beforeAcceptance:false};
    if (reply.value.outcome === 'promptRequired') return {kind:'refused',reason:{code:'provider_refused',message:'opaque'},beforeAcceptance:true};
    const turn = op.activeTurnId || op.sendId;
    const events = reply.value.outcome ? [] : [{kind:'turnState',threadId:op.threadId,turnId:turn,state:'idle'}];
    return {kind:'complete',result:{kind:'turnAccepted',threadId:op.threadId,sendId:op.sendId,turnId:turn},events};
  },
  runtimeReduce(ctx, state, frames) { return {state,events:[]}; }
};
"#;

fn turn_state(thread: &str, turn: &str, state: RuntimeTurnState) -> AgentRuntimeEvent {
    AgentRuntimeEvent::TurnState {
        thread_id: thread.into(),
        turn_id: Some(turn.into()),
        parent_thread_id: None,
        state,
        detail: None,
        status_line: None,
        started_at: None,
        completed_at: None,
    }
}

fn publish_turn(svc: &ServiceContext, session: &Arc<Session>, turn: &str, state: RuntimeTurnState) {
    let (thread, epoch) = {
        let context = session.context.lock();
        (context.thread_id.clone().unwrap(), context.epoch)
    };
    crate::async_runtime::block_on(subscriber::publish_events(
        svc,
        session,
        epoch,
        vec![turn_state(&thread, turn, state)],
        now(),
    ))
    .unwrap();
}

fn running_turn_with_host_tools(svc: &ServiceContext, tab: &str) -> (Arc<Session>, String) {
    svc.bind_user_send_owner();
    let session = engine::for_tab(tab).unwrap();
    publish_turn(svc, &session, "running", RuntimeTurnState::Working);
    let mut context = session.context.lock();
    context.host_mcp = Some(RuntimeHostMcp {
        name: "domios".into(),
        command: "opaque".into(),
        args: Vec::new(),
        env: Default::default(),
    });
    let nonce = context.binding_nonce.clone();
    drop(context);
    (session, nonce)
}

fn admit(svc: &ServiceContext, tab: &str, text: &str) -> UserSendRecord {
    let store = &svc.pending_delivery_payloads.user_sends;
    let reservation = store
        .reserve(svc.user_send_authority(tab).unwrap(), None, || {
            svc.chat_send_ledger.reserve_identity()
        })
        .unwrap();
    let record = store.admit(&reservation, text, text, None).unwrap();
    drop(reservation);
    svc.chat_send_ledger.admit_user(&record);
    record
}

fn state(svc: &ServiceContext, record: &UserSendRecord) -> ChatSendState {
    svc.chat_send_ledger.status(&record.send_id).unwrap().state
}

fn queued(svc: &ServiceContext, tab: &str) -> Vec<String> {
    svc.pending_delivery_payloads
        .user_sends
        .queued(tab)
        .unwrap()
        .into_iter()
        .map(|record| record.send_id)
        .collect()
}

fn assert_nothing_written(peer: &mut BufReader<DuplexStream>) {
    assert!(matches!(
        Pin::new(peer).poll_fill_buf(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
}

#[test]
fn a_tool_result_carries_held_messages_in_order_once_and_only_its_ack_delivers_them() {
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let svc = &fixture.service;
    let (_session, nonce) = running_turn_with_host_tools(svc, &tab);
    let first = admit(svc, &tab, "opaque first");
    let second = admit(svc, &tab, "opaque second");

    let text = svc
        .claim_inbound_tool_messages(&tab, &nonce, "claim")
        .unwrap()
        .expect("held input rides the tool result");
    assert!(text.find("opaque first").unwrap() < text.find("opaque second").unwrap());
    assert!(text.contains(&first.send_id) && text.contains(&second.send_id));

    assert_eq!(
        svc.claim_inbound_tool_messages(&tab, &nonce, "other")
            .unwrap(),
        None,
        "claimed input is never offered to a second tool result"
    );
    assert!(matches!(
        user_delivery::drain(svc, &tab).unwrap(),
        UserDrain::Idle
    ));
    assert!(queued(svc, &tab).is_empty());
    for record in [&first, &second] {
        assert_ne!(
            state(svc, record),
            ChatSendState::Sent,
            "a claim alone proves nothing reached the agent"
        );
    }

    svc.settle_inbound_tool_messages(&tab, &nonce, "claim", true)
        .unwrap();
    for record in [&first, &second] {
        assert_eq!(state(svc, record), ChatSendState::Sent);
    }
    svc.settle_inbound_tool_messages(&tab, &nonce, "claim", true)
        .unwrap();
    assert!(matches!(
        user_delivery::drain(svc, &tab).unwrap(),
        UserDrain::Idle
    ));
    let mut peer = BufReader::new(peer);
    assert_nothing_written(&mut peer);
}

#[test]
fn tool_claims_belong_to_this_launchs_running_turn_with_host_tools() {
    let (fixture, tab, _peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let svc = &fixture.service;
    let (session, nonce) = running_turn_with_host_tools(svc, &tab);
    let record = admit(svc, &tab, "opaque");

    assert_eq!(
        svc.claim_inbound_tool_messages(&tab, "foreign", "foreign")
            .unwrap(),
        None
    );
    let descriptor = session.context.lock().host_mcp.take();
    assert_eq!(
        svc.claim_inbound_tool_messages(&tab, &nonce, "no-tools")
            .unwrap(),
        None
    );
    session.context.lock().host_mcp = descriptor;
    session.prompts_cancelled.store(true, Ordering::Release);
    assert_eq!(
        svc.claim_inbound_tool_messages(&tab, &nonce, "cancelling")
            .unwrap(),
        None,
        "an interrupted agent may discard the tool result"
    );
    session.prompts_cancelled.store(false, Ordering::Release);
    publish_turn(svc, &session, "running", RuntimeTurnState::Idle);
    assert_eq!(
        svc.claim_inbound_tool_messages(&tab, &nonce, "idle")
            .unwrap(),
        None,
        "with no turn running the ordinary prompt owns the send"
    );
    assert_eq!(queued(svc, &tab), [record.send_id]);
}

#[test]
fn a_claim_stops_at_the_first_send_that_cannot_ride_a_tool_result() {
    let (fixture, tab, _peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let svc = &fixture.service;
    let (_session, nonce) = running_turn_with_host_tools(svc, &tab);
    admit(svc, &tab, "opaque text");
    let command = admit(svc, &tab, "/opaque-command");
    let after = admit(svc, &tab, "opaque after");

    let text = svc
        .claim_inbound_tool_messages(&tab, &nonce, "claim")
        .unwrap()
        .unwrap();
    assert!(text.contains("opaque text"));
    assert!(!text.contains("opaque-command") && !text.contains("opaque after"));
    assert_eq!(
        queued(svc, &tab),
        [command.send_id, after.send_id],
        "order is kept: nothing overtakes the command"
    );
}

#[test]
fn a_message_too_long_for_a_tool_result_stays_held_for_the_turn_end() {
    let (fixture, tab, _peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let svc = &fixture.service;
    let (_session, nonce) = running_turn_with_host_tools(svc, &tab);
    let long = admit(svc, &tab, &"x".repeat(TOOL_BUDGET_BYTES + 1));
    assert_eq!(
        svc.claim_inbound_tool_messages(&tab, &nonce, "claim")
            .unwrap(),
        None
    );
    assert_eq!(queued(svc, &tab), [long.send_id]);
}

#[test]
fn a_claim_whose_text_never_reached_the_tool_server_returns_to_the_queue_in_order() {
    let (fixture, tab, _peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let svc = &fixture.service;
    let (_session, nonce) = running_turn_with_host_tools(svc, &tab);
    let first = admit(svc, &tab, "opaque first");
    let second = admit(svc, &tab, "opaque second");
    svc.claim_inbound_tool_messages(&tab, &nonce, "lost-reply")
        .unwrap()
        .unwrap();
    svc.settle_inbound_tool_messages(&tab, &nonce, "lost-reply", false)
        .unwrap();
    assert_eq!(
        queued(svc, &tab),
        [first.send_id.clone(), second.send_id.clone()]
    );
    assert_eq!(state(svc, &first), ChatSendState::Pending);

    let again = svc
        .claim_inbound_tool_messages(&tab, &nonce, "next")
        .unwrap()
        .expect("a proven unwritten claim can be claimed once more");
    assert!(again.contains(&first.send_id) && again.contains(&second.send_id));
    svc.settle_inbound_tool_messages(&tab, &nonce, "next", true)
        .unwrap();
    assert_eq!(state(svc, &first), ChatSendState::Sent);
}

#[test]
fn an_unacknowledged_claim_is_uncertain_when_the_turn_ends_and_is_never_replayed() {
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let svc = &fixture.service;
    let (session, nonce) = running_turn_with_host_tools(svc, &tab);
    let record = admit(svc, &tab, "opaque");
    svc.claim_inbound_tool_messages(&tab, &nonce, "unacknowledged")
        .unwrap()
        .unwrap();

    publish_turn(svc, &session, "running", RuntimeTurnState::Idle);
    assert!(matches!(
        user_delivery::drain(svc, &tab).unwrap(),
        UserDrain::Idle
    ));
    assert_eq!(state(svc, &record), ChatSendState::Unknown);
    assert!(queued(svc, &tab).is_empty());
    let mut peer = BufReader::new(peer);
    assert_nothing_written(&mut peer);

    svc.settle_inbound_tool_messages(&tab, &nonce, "unacknowledged", true)
        .unwrap();
    assert_eq!(
        state(svc, &record),
        ChatSendState::Sent,
        "a late acknowledgement is still exact evidence"
    );
}

fn blocking_turn_server(
    peer: DuplexStream,
) -> (
    tokio::sync::oneshot::Sender<()>,
    tokio::sync::oneshot::Receiver<Value>,
    tokio::task::JoinHandle<BufReader<DuplexStream>>,
) {
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    let (next_written, next) = tokio::sync::oneshot::channel::<Value>();
    let server = crate::async_runtime::spawn(async move {
        let mut peer = BufReader::new(peer);
        let mut line = String::new();
        peer.read_line(&mut line).await.unwrap();
        let busy: Value = serde_json::from_str(&line).unwrap();
        released.await.unwrap();
        let reply = json!({"id":busy["id"],"result":{"stopReason":"end_turn"}});
        peer.get_mut()
            .write_all(format!("{reply}\n").as_bytes())
            .await
            .unwrap();
        line.clear();
        peer.read_line(&mut line).await.unwrap();
        next_written
            .send(serde_json::from_str(&line).unwrap())
            .unwrap();
        peer
    });
    (release, next, server)
}

fn submit_busy_turn(svc: &ServiceContext, tab: &str) -> Arc<Session> {
    svc.bind_user_send_owner();
    let session = engine::for_tab(tab).unwrap();
    let process = session.process.lock().clone().unwrap();
    let operation = {
        let context = session.context.lock();
        RuntimeOperation::SubmitTurn {
            thread_id: context.thread_id.clone().unwrap(),
            active_turn_id: None,
            send_id: "busy".into(),
            input: TurnInput {
                text: "opaque busy".into(),
                attachments: Vec::new(),
            },
            settings: RuntimeSettings::default(),
        }
    };
    let (_, written) =
        crate::async_runtime::block_on(engine::run_submit(svc, &session, &process, operation));
    assert!(written);
    session
}

fn hold_behind_turn(svc: &ServiceContext, tab: &str, text: &str) -> UserSendRecord {
    let record = admit(svc, tab, text);
    assert!(matches!(
        user_delivery::drain(svc, tab).unwrap(),
        UserDrain::Idle
    ));
    assert_eq!(state(svc, &record), ChatSendState::Pending);
    record
}

fn await_next(next: tokio::sync::oneshot::Receiver<Value>) -> Value {
    crate::async_runtime::block_on(async {
        tokio::time::timeout(Duration::from_secs(5), next).await
    })
    .expect("the next prompt is written once its turn settles")
    .unwrap()
}

#[test]
fn messages_held_at_turn_end_go_out_as_one_framed_digest_in_order_and_settle_together() {
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let svc = &fixture.service;
    let (release, next, server) = blocking_turn_server(peer);
    let session = submit_busy_turn(svc, &tab);
    let held = ["opaque first", "opaque second", "opaque third"]
        .map(|text| hold_behind_turn(svc, &tab, text));

    release.send(()).unwrap();
    let digest = await_next(next);
    assert_eq!(digest["params"]["sendId"], held[0].send_id.as_str());
    let wire = digest["params"]["text"].as_str().unwrap();
    let frame = codeterm_wire::domios::parse_message(wire).expect("one complete leading frame");
    assert_eq!(frame.attr("context"), Some(DIGEST_CONTEXT));
    let mut from = 0;
    for (record, text) in held
        .iter()
        .zip(["opaque first", "opaque second", "opaque third"])
    {
        let id = wire
            .find(&record.send_id)
            .expect("each message is identified");
        let body = wire.find(text).unwrap();
        assert!(from <= id && id < body, "arrival order is preserved");
        from = body;
    }
    let mut peer = crate::async_runtime::block_on(server).unwrap();
    assert_nothing_written(&mut peer);

    let (thread, epoch) = {
        let context = session.context.lock();
        (context.thread_id.clone().unwrap(), context.epoch)
    };
    crate::async_runtime::block_on(subscriber::publish_events(
        svc,
        &session,
        epoch,
        vec![AgentRuntimeEvent::SendEvidence {
            thread_id: thread,
            send_id: held[0].send_id.clone(),
            turn_id: held[0].send_id.clone(),
            item_id: None,
        }],
        now(),
    ))
    .unwrap();
    for record in &held {
        assert_eq!(state(svc, record), ChatSendState::Sent);
    }
    assert!(queued(svc, &tab).is_empty());
    assert_nothing_written(&mut peer);
}

#[test]
fn a_digest_never_folds_past_a_send_that_needs_its_own_prompt() {
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let svc = &fixture.service;
    let (release, next, _server) = blocking_turn_server(peer);
    submit_busy_turn(svc, &tab);
    hold_behind_turn(svc, &tab, "opaque first");
    let command = hold_behind_turn(svc, &tab, "/opaque-command");

    release.send(()).unwrap();
    assert_eq!(
        await_next(next)["params"]["text"],
        "opaque first",
        "a lone message keeps its own wire text"
    );
    assert_eq!(queued(svc, &tab), [command.send_id]);
}

fn claimed_head(
    svc: &ServiceContext,
    tab: &str,
    session: &Arc<Session>,
) -> (UserSendRecord, UserSendRecord, RuntimeContext) {
    svc.bind_user_send_owner();
    let head = admit(svc, tab, "opaque head");
    let member = admit(svc, tab, "opaque member");
    let context = session.context.lock().clone();
    let store = &svc.pending_delivery_payloads.user_sends;
    assert!(store.claim(&head, context.thread_id.as_deref()).unwrap());
    assert!(svc.chat_send_ledger.begin_user_handoff(
        &head,
        "opaque head",
        svc.chat_send_anchor(tab)
    ));
    (head, member, context)
}

#[test]
fn a_digest_that_never_reached_the_writer_returns_every_member_to_the_queue() {
    let (fixture, tab, _peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let svc = &fixture.service;
    let session = engine::for_tab(&tab).unwrap();
    let (head, member, context) = claimed_head(svc, &tab, &session);
    let digest = begin_digest(svc, &tab, &session, &head, "opaque head", &context).unwrap();
    assert!(queued(svc, &tab).is_empty());

    finish_digest(svc, &tab, &session, digest, true, Err("opaque".into()));
    assert_eq!(queued(svc, &tab), [member.send_id.clone()]);
    assert_eq!(state(svc, &member), ChatSendState::Pending);
    assert!(!svc.chat_send_ledger.has_rpc_handoff(&member.send_id));
}

#[test]
fn a_digest_with_an_uncertain_write_leaves_every_member_uncertain_and_never_replays() {
    let (fixture, tab, _peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let svc = &fixture.service;
    let session = engine::for_tab(&tab).unwrap();
    let (head, member, context) = claimed_head(svc, &tab, &session);
    let digest = begin_digest(svc, &tab, &session, &head, "opaque head", &context).unwrap();

    finish_digest(
        svc,
        &tab,
        &session,
        digest,
        false,
        Err("submit_unconfirmed: opaque".into()),
    );
    assert!(queued(svc, &tab).is_empty());
    assert_eq!(state(svc, &member), ChatSendState::Unknown);
}

fn steering_turn_server(
    peer: DuplexStream,
    outcome: Value,
) -> (
    tokio::sync::oneshot::Sender<()>,
    tokio::sync::oneshot::Receiver<Value>,
    tokio::sync::oneshot::Receiver<Value>,
    tokio::task::JoinHandle<BufReader<DuplexStream>>,
) {
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    let (joined_written, joined) = tokio::sync::oneshot::channel::<Value>();
    let (next_written, next) = tokio::sync::oneshot::channel::<Value>();
    let server = crate::async_runtime::spawn(async move {
        let mut peer = BufReader::new(peer);
        let mut line = String::new();
        peer.read_line(&mut line).await.unwrap();
        let busy: Value = serde_json::from_str(&line).unwrap();
        line.clear();
        peer.read_line(&mut line).await.unwrap();
        let steer: Value = serde_json::from_str(&line).unwrap();
        let reply = json!({"id":steer["id"],"result":{"outcome":outcome}});
        peer.get_mut()
            .write_all(format!("{reply}\n").as_bytes())
            .await
            .unwrap();
        joined_written.send(steer).unwrap();
        released.await.unwrap();
        let reply = json!({"id":busy["id"],"result":{"stopReason":"end_turn"}});
        peer.get_mut()
            .write_all(format!("{reply}\n").as_bytes())
            .await
            .unwrap();
        line.clear();
        if outcome == "promptRequired" {
            peer.read_line(&mut line).await.unwrap();
            next_written
                .send(serde_json::from_str(&line).unwrap())
                .unwrap();
        }
        peer
    });
    (release, joined, next, server)
}

#[test]
fn an_agent_that_declared_it_takes_a_send_into_its_running_turn_without_waiting_for_the_end() {
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let svc = &fixture.service;
    let (release, joined, _next, server) = steering_turn_server(peer, json!("injected"));
    let session = submit_busy_turn(svc, &tab);
    session.state.lock().mid_turn_input = true;

    let record = admit(svc, &tab, "opaque steer");
    assert!(matches!(
        user_delivery::drain(svc, &tab).unwrap(),
        UserDrain::Idle
    ));
    let steer = crate::async_runtime::block_on(async {
        tokio::time::timeout(Duration::from_secs(5), joined).await
    })
    .expect("the send is written while the turn is still running")
    .unwrap();
    assert_eq!(steer["params"]["sendId"], record.send_id.as_str());
    assert_eq!(state(svc, &record), ChatSendState::Sent);
    assert!(queued(svc, &tab).is_empty());
    assert_eq!(
        session.context.lock().active_turn_id.as_deref(),
        Some("busy"),
        "the running turn still owns its end"
    );
    assert!(!session.turn_requests.lock().contains_key(&record.send_id));

    release.send(()).unwrap();
    let mut peer = crate::async_runtime::block_on(server).unwrap();
    assert_nothing_written(&mut peer);
}

#[test]
fn input_the_agent_declined_stays_held_and_is_written_once_when_the_turn_ends() {
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let svc = &fixture.service;
    let (release, joined, next, server) = steering_turn_server(peer, json!("promptRequired"));
    let session = submit_busy_turn(svc, &tab);
    session.state.lock().mid_turn_input = true;

    let record = admit(svc, &tab, "opaque declined");
    assert!(matches!(
        user_delivery::drain(svc, &tab).unwrap(),
        UserDrain::ZeroWriteRefusal { .. }
    ));
    crate::async_runtime::block_on(async {
        tokio::time::timeout(Duration::from_secs(5), joined).await
    })
    .unwrap()
    .unwrap();
    assert_eq!(state(svc, &record), ChatSendState::Pending);
    assert_eq!(queued(svc, &tab), [record.send_id.clone()]);
    assert!(!svc.chat_send_ledger.has_rpc_handoff(&record.send_id));

    release.send(()).unwrap();
    let prompt = await_next(next);
    assert_eq!(prompt["params"]["sendId"], record.send_id.as_str());
    let mut peer = crate::async_runtime::block_on(server).unwrap();
    assert_nothing_written(&mut peer);
}

#[test]
fn a_command_waits_for_the_turn_even_when_the_agent_takes_input_into_it() {
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let svc = &fixture.service;
    let (_release, _next, _server) = blocking_turn_server(peer);
    let session = submit_busy_turn(svc, &tab);
    session.state.lock().mid_turn_input = true;

    let command = hold_behind_turn(svc, &tab, "/opaque-command");
    assert_eq!(
        svc.chat_send_ledger
            .status(&command.send_id)
            .unwrap()
            .pending_reason
            .as_deref(),
        Some(turn_requests::ACTIVE_TURN_HOLD)
    );
    assert_eq!(queued(svc, &tab), [command.send_id]);
}
