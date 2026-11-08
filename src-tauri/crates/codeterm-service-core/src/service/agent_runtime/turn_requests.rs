use super::{
    engine::{self, Session},
    model::RuntimeFailure,
    process::Process,
    rpc, subscriber,
};
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::*;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use tokio::sync::watch;

mod refusal;

static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);

enum ActivityError {
    Deferred,
    Failed(RuntimeFailure),
}

pub(super) struct PendingTurn {
    context: RuntimeContext,
    send_id: String,
    settled: AtomicBool,
    refused: AtomicBool,
    observed: AtomicBool,
    had_predecessor: bool,
    started_at: u64,
    order: u64,
    activity: tokio::sync::OnceCell<()>,
    written_input: tokio::sync::OnceCell<()>,
    turn: String,
    witness: Arc<rpc::WriteWitness>,
    outcome: watch::Sender<Option<Result<RuntimeResult, RuntimeFailure>>>,
    cancelled: watch::Sender<bool>,
}

pub(super) async fn stream_window(session: &Session, sequences: Vec<u64>) -> RuntimeStreamWindow {
    let context = session.context.lock().clone();
    let pending = session
        .turn_requests
        .lock()
        .values()
        .filter(|request| {
            request.context.epoch == context.epoch
                && request.context.binding_nonce == context.binding_nonce
                && request.context.thread_id == context.thread_id
                && !request.settled.load(Ordering::Acquire)
        })
        .cloned()
        .collect::<Vec<_>>();
    let through = sequences.last().copied().unwrap_or_default();
    for request in &pending {
        if request
            .witness
            .after_sequence()
            .is_some_and(|boundary| through > boundary)
        {
            let _ = request.witness.wait().await;
        }
    }
    RuntimeStreamWindow {
        frame_sequences: sequences,
        turn_requests: pending
            .iter()
            .map(|request| RuntimeTurnRequestWitness {
                send_id: request.send_id.clone(),
                turn_id: request.turn.clone(),
                after_sequence: request.witness.after_sequence(),
                written: request.witness.written(),
            })
            .collect(),
    }
}

pub(super) fn written_pending(session: &Session, context: &RuntimeContext) -> bool {
    session.turn_requests.lock().values().any(|pending| {
        pending.context.epoch == context.epoch
            && pending.context.binding_nonce == context.binding_nonce
            && pending.context.thread_id == context.thread_id
            && pending.witness.written()
            && !pending.settled.load(Ordering::Acquire)
    })
}

pub(super) fn evidence(session: &Session, send: &str, thread: &str, turn: &str) -> bool {
    let pending = session.turn_requests.lock().get(send).cloned();
    let Some(pending) = pending else {
        return true;
    };
    let current = session.context.lock().clone();
    if pending.context.thread_id.as_deref() != Some(thread)
        || pending.turn != turn
        || current.epoch != pending.context.epoch
        || current.binding_nonce != pending.context.binding_nonce
        || pending.settled.load(Ordering::Acquire) && !pending.refused.load(Ordering::Acquire)
        || !pending.witness.written()
    {
        return false;
    }
    pending.observed.store(true, Ordering::Release);
    if pending.outcome.borrow().is_none() {
        pending
            .outcome
            .send_replace(Some(Ok(RuntimeResult::TurnAccepted {
                thread_id: thread.into(),
                send_id: send.into(),
                turn_id: Some(turn.into()),
            })));
    }
    true
}

pub(super) fn observe_item(
    session: &Session,
    context: &RuntimeContext,
    item: &codeterm_types::agent::chat_types::ChatMsg,
) {
    if !matches!(
        item.kind.as_str(),
        "user" | "assistant" | "thinking" | "tool" | "tool_call" | "tool_result"
    ) {
        return;
    }
    for pending in session.turn_requests.lock().values() {
        if pending.context.epoch == context.epoch
            && pending.context.binding_nonce == context.binding_nonce
            && pending.context.thread_id == context.thread_id
            && context.active_turn_id.as_deref() == Some(pending.turn.as_str())
        {
            pending.observed.store(true, Ordering::Release);
        }
    }
}

pub(super) fn retire_refused_claims(
    session: &Session,
    expected: &RuntimeContext,
    turn: Option<&str>,
) {
    let expired = session
        .maps
        .lock()
        .settle_refused_claims(expected.epoch, turn);
    if expired.is_empty() {
        return;
    }
    let mut current = session.context.lock();
    if current.epoch == expected.epoch
        && current.binding_nonce == expected.binding_nonce
        && current.thread_id == expected.thread_id
    {
        // Keep late refusal evidence eligible only until the next other turn settles.
        current
            .claimed_sends
            .retain(|claim| !expired.contains(&claim.send_id));
    }
}

pub(super) fn started_at(
    session: &Session,
    context: &RuntimeContext,
    turn: Option<&str>,
) -> Option<u64> {
    session
        .turn_requests
        .lock()
        .values()
        .find(|pending| {
            pending.context.epoch == context.epoch
                && pending.context.binding_nonce == context.binding_nonce
                && pending.context.thread_id == context.thread_id
                && Some(pending.turn.as_str()) == turn
                && pending.witness.written()
        })
        .map(|pending| pending.started_at)
}

pub(super) async fn publish_written_inputs(
    svc: &ServiceContext,
    session: &Arc<Session>,
    epoch: u64,
) -> Result<(), RuntimeFailure> {
    let mut pending = session
        .turn_requests
        .lock()
        .values()
        .filter(|pending| {
            pending.context.epoch == epoch
                && pending.witness.written()
                && !pending.settled.load(Ordering::Acquire)
        })
        .cloned()
        .collect::<Vec<_>>();
    pending.sort_by_key(|pending| pending.order);
    for pending in pending {
        pending
            .written_input
            .get_or_try_init(|| async {
                validate(svc, session, &pending.context)?;
                let thread = pending
                    .context
                    .thread_id
                    .as_deref()
                    .ok_or(RuntimeFailure::Superseded)?;
                let Some(claim) =
                    pending.context.claimed_sends.iter().find(|claim| {
                        claim.send_id == pending.send_id && claim.thread_id == thread
                    })
                else {
                    return Ok(());
                };
                let mut item = session
                    .maps
                    .lock()
                    .routed_input(&pending.send_id, thread)
                    .unwrap_or_else(|| codeterm_types::agent::chat_types::ChatMsg {
                        kind: "user".into(),
                        content: claim.input.text.clone(),
                        ..Default::default()
                    });
                item.id = super::events::RuntimeMaps::written_input_id(thread, &pending.send_id);
                item.attachments =
                    (!claim.input.attachments.is_empty()).then(|| claim.input.attachments.clone());
                super::model::omit_user_image_data(&mut item);
                let item = session
                    .cache
                    .lock()
                    .reconcile_live(item, pending.started_at);
                super::events::fold(
                    &mut session.maps.lock(),
                    epoch,
                    &super::events::RuntimeEvent::ItemUpdated {
                        thread_id: thread.into(),
                        item: item.clone(),
                    },
                );
                validate(svc, session, &pending.context)?;
                crate::service::tab_chat::structured_sink::emit_chat_msgs(
                    svc.ui.as_ref(),
                    &pending.context.tab_id,
                    &[item],
                    None,
                    Some(thread),
                    Some(&pending.context.binding_nonce),
                );
                super::transcript::persist(session).await;
                Ok::<(), RuntimeFailure>(())
            })
            .await?;
    }
    Ok(())
}

async fn publish_written_activity(
    svc: &ServiceContext,
    session: &Arc<Session>,
    pending: &PendingTurn,
) -> Result<(), RuntimeFailure> {
    publish_written_inputs(svc, session, pending.context.epoch).await?;
    let result = pending
        .activity
        .get_or_try_init(|| async {
            validate(svc, session, &pending.context).map_err(ActivityError::Failed)?;
            let current = session.context.lock().clone();
            let thread = current
                .thread_id
                .clone()
                .ok_or(ActivityError::Failed(RuntimeFailure::Superseded))?;
            if pending.settled.load(Ordering::Acquire)
                || session.completions.lock().contains(&thread, &pending.turn)
            {
                return Ok(());
            }
            let earlier = session
                .turn_requests
                .lock()
                .values()
                .filter(|other| {
                    other.order < pending.order
                        && other.context.epoch == current.epoch
                        && other.context.binding_nonce == current.binding_nonce
                        && other.context.thread_id == current.thread_id
                        && other.witness.written()
                        && !other.settled.load(Ordering::Acquire)
                })
                .map(|other| other.turn.clone())
                .collect::<Vec<_>>();
            if current
                .active_turn_id
                .as_ref()
                .is_some_and(|turn| turn != &pending.turn)
                || earlier
                    .iter()
                    .any(|turn| !session.completions.lock().contains(&thread, turn))
            {
                return Err(ActivityError::Deferred);
            }
            // A written request awaiting its reply is already an outstanding turn.
            subscriber::publish_events(
                svc,
                session,
                current.epoch,
                vec![AgentRuntimeEvent::TurnState {
                    thread_id: thread,
                    turn_id: Some(pending.turn.clone()),
                    parent_thread_id: None,
                    state: RuntimeTurnState::Working,
                    detail: None,
                    status_line: None,
                    started_at: Some(pending.started_at),
                    completed_at: None,
                }],
                pending.started_at,
            )
            .await
            .map_err(ActivityError::Failed)
        })
        .await;
    match result {
        Err(ActivityError::Failed(reason)) => Err(reason),
        _ => Ok(()),
    }
}

pub(super) async fn run(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    context: RuntimeContext,
    operation: RuntimeOperation,
    plan: RuntimePlan,
    guard: Option<rpc::PrewriteGuard>,
) -> Result<RuntimeResult, RuntimeFailure> {
    let RuntimeOperation::SubmitTurn { send_id, .. } = &operation else {
        return Err(RuntimeFailure::Superseded);
    };
    let RuntimeAction::Request { call } = &plan.action else {
        return Err(RuntimeFailure::Superseded);
    };
    let turn = call
        .target
        .as_ref()
        .and_then(|target| target.turn_id.clone())
        .ok_or_else(|| {
            RuntimeFailure::PrewriteRefused("Turn-scoped request lacks turn identity".into())
        })?;
    let pending = Arc::new(PendingTurn {
        context: context.clone(),
        send_id: send_id.clone(),
        settled: AtomicBool::new(false),
        refused: AtomicBool::new(false),
        observed: AtomicBool::new(false),
        had_predecessor: written_pending(session, &context),
        started_at: now(),
        order: NEXT_REQUEST.fetch_add(1, Ordering::Relaxed),
        activity: tokio::sync::OnceCell::new(),
        written_input: tokio::sync::OnceCell::new(),
        turn,
        witness: Arc::new(rpc::WriteWitness::default()),
        outcome: watch::channel(None).0,
        cancelled: watch::channel(false).0,
    });
    {
        let mut requests = session.turn_requests.lock();
        if requests.len() >= 32 || requests.contains_key(send_id) {
            return Err(RuntimeFailure::PrewriteRefused(
                "Pending turn request capacity exceeded".into(),
            ));
        }
        requests.insert(send_id.clone(), pending.clone());
    }
    let mut outcome = pending.outcome.subscribe();
    let witness = pending.witness.clone();
    let activity = pending.clone();
    let owner = svc.clone();
    let runtime = session.clone();
    let process = process.clone();
    let send = send_id.clone();
    tokio::spawn(async move {
        let result = await_reply(
            &owner, &runtime, &process, &pending, &operation, &plan, guard,
        )
        .await;
        if let Err(reason) = &result {
            if !*pending.cancelled.borrow() {
                let _ = settle_failure(
                    &owner,
                    &runtime,
                    &pending,
                    reply_failure_detail(reason),
                    format!("Turn completion reply failed: {reason}"),
                )
                .await;
            }
        }
        if pending.outcome.borrow().is_none() {
            pending.outcome.send_replace(Some(result));
        }
        runtime.turn_requests.lock().remove(&send);
        runtime.control_changes.send_replace(());
        owner.wake_user_owner_on_state_change(&pending.context.tab_id);
    });
    loop {
        if let Some(result) = outcome.borrow_and_update().clone() {
            return result;
        }
        tokio::select! {
            changed = outcome.changed() => { changed.map_err(|_| RuntimeFailure::PipeEof)?; },
            written = witness.wait() => {
                written?;
                publish_written_activity(svc, session, &activity).await?;
                if let Some(result) = outcome.borrow().clone() { return result; }
                return Err(RuntimeFailure::WrittenAwaitingEvidence);
            }
        }
    }
}

async fn await_reply(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    pending: &PendingTurn,
    operation: &RuntimeOperation,
    plan: &RuntimePlan,
    guard: Option<rpc::PrewriteGuard>,
) -> Result<RuntimeResult, RuntimeFailure> {
    let RuntimeAction::Request { call } = &plan.action else {
        return Err(RuntimeFailure::Superseded);
    };
    let mut changes = session.control_changes.subscribe();
    let mut ended = process.ended.clone();
    let mut cancelled = pending.cancelled.subscribe();
    let reply = process
        .client
        .request_turn(call, guard, pending.witness.clone());
    tokio::pin!(reply);
    let reply = loop {
        if *cancelled.borrow_and_update() {
            return Err(RuntimeFailure::Refused(
                "Turn completion request cancelled by lifecycle".into(),
            ));
        }
        validate(svc, session, &pending.context)?;
        if pending.witness.written() && pending.activity.get().is_none() {
            publish_written_activity(svc, session, pending).await?;
        }
        tokio::select! {
            biased;
            reply = &mut reply => break reply?,
            _ = changes.changed() => {},
            _ = ended.changed() => {
                if let Some(reason) = ended.borrow().clone() { return Err(reason); }
            },
            _ = cancelled.changed() => return Err(RuntimeFailure::Refused("Turn interrupted after cancel grace".into())),
        }
    };
    pending.witness.wait().await?;
    publish_written_activity(svc, session, pending).await?;
    wait_for_updates(svc, session, process, pending, reply.through).await?;
    validate(svc, session, &pending.context)?;
    let adapter = session
        .adapter
        .lock()
        .clone()
        .ok_or(RuntimeFailure::Superseded)?;
    let capability = adapter.agent_runtime().ok_or(RuntimeFailure::Superseded)?;
    let reply = match reply.result {
        Ok(value) => RpcReply::Success { value },
        Err(error) => RpcReply::Error { error },
    };
    let step = tokio::time::timeout(
        RUNTIME_HOOK_DEADLINE,
        adapter.runtime_response(&pending.context, operation, &plan.state, &reply),
    )
    .await
    .map_err(|_| RuntimeFailure::Deadline)?
    .map_err(|reason| RuntimeFailure::HookFailed(reason.to_string()))?;
    validate_runtime_step(&capability, &pending.context, operation, &reply, &step)
        .map_err(|reason| RuntimeFailure::Refused(reason.to_string()))?;
    if let RuntimeStep::Refused {
        reason,
        before_acceptance,
    } = &step
    {
        refusal::apply(svc, session, pending, &reply, reason, *before_acceptance).await?;
        return Err(RuntimeFailure::Refused(reason.message.clone()));
    }
    let RuntimeStep::Complete {
        result,
        events,
        notification: None,
    } = step
    else {
        return Err(RuntimeFailure::Refused(
            "Turn completion must finish the original request".into(),
        ));
    };
    let current = session.context.lock().clone();
    let mut events: Vec<_> = events
        .unwrap_or_default()
        .into_iter()
        .filter(|event| super::model::accepts_response_event(&pending.context, &current, event))
        .collect();
    if let Some(claim) = current.claimed_sends.iter().find(|claim| {
        claim.send_id == pending.send_id
            && Some(&claim.thread_id) == pending.context.thread_id.as_ref()
    }) {
        let mut maps = session.maps.lock();
        for event in &mut events {
            if let AgentRuntimeEvent::ChatItem { thread_id, item } = event {
                if thread_id == &claim.thread_id
                    && item.kind == "user"
                    && item.content == claim.input.text
                    && item.attachments.as_deref().unwrap_or_default()
                        == claim.input.attachments.as_slice()
                {
                    maps.project_routed_response_item(&pending.send_id, thread_id, item);
                }
            }
        }
    }
    if pending.settled.swap(true, Ordering::AcqRel) {
        return Ok(result);
    }
    subscriber::publish_events(svc, session, pending.context.epoch, events, now()).await?;
    Ok(result)
}

async fn wait_for_updates(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Process,
    pending: &PendingTurn,
    through: u64,
) -> Result<(), RuntimeFailure> {
    let mut progress = session.stream_progress.subscribe();
    let mut lifecycle = session.control_changes.subscribe();
    let mut ended = process.ended.clone();
    let mut cancelled = pending.cancelled.subscribe();
    loop {
        validate(svc, session, &pending.context)?;
        if *cancelled.borrow_and_update() {
            return Err(RuntimeFailure::Superseded);
        }
        if *progress.borrow_and_update() >= through {
            return Ok(());
        }
        if let Some(reason) = ended.borrow().clone() {
            return Err(reason);
        }
        tokio::select! {
            _ = progress.changed() => {},
            _ = lifecycle.changed() => {},
            _ = ended.changed() => {},
            _ = cancelled.changed() => {},
        }
    }
}

fn validate(
    svc: &ServiceContext,
    session: &Session,
    expected: &RuntimeContext,
) -> Result<(), RuntimeFailure> {
    engine::validate_binding(svc, session)?;
    let current = session.context.lock();
    if current.epoch != expected.epoch
        || current.binding_nonce != expected.binding_nonce
        || current.thread_id != expected.thread_id
    {
        return Err(RuntimeFailure::Superseded);
    }
    Ok(())
}

async fn settle_failure(
    svc: &ServiceContext,
    session: &Arc<Session>,
    pending: &PendingTurn,
    detail: RuntimeTurnDetail,
    message: String,
) -> Result<(), RuntimeFailure> {
    if pending.witness.after_sequence().is_none() {
        return Ok(());
    }
    validate(svc, session, &pending.context)?;
    if pending.settled.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    let thread_id = pending
        .context
        .thread_id
        .clone()
        .ok_or(RuntimeFailure::Superseded)?;
    let turn_id = Some(pending.turn.clone());
    let mut events = Vec::new();
    if pending.witness.written() && !pending.observed.load(Ordering::Acquire) {
        let claim = session
            .context
            .lock()
            .claimed_sends
            .iter()
            .find(|claim| claim.send_id == pending.send_id && claim.thread_id == thread_id)
            .cloned();
        if let Some(claim) = claim {
            let mut item = session
                .maps
                .lock()
                .routed_input(&pending.send_id, &thread_id)
                .unwrap_or_else(|| codeterm_types::agent::chat_types::ChatMsg {
                    kind: "user".into(),
                    content: claim.input.text,
                    attachments: (!claim.input.attachments.is_empty())
                        .then_some(claim.input.attachments),
                    ..Default::default()
                });
            item.id = super::events::RuntimeMaps::written_input_id(&thread_id, &pending.send_id);
            events.push(AgentRuntimeEvent::ChatItem {
                thread_id: thread_id.clone(),
                item,
            });
        }
    }
    events.extend([
        AgentRuntimeEvent::ChatItem {
            thread_id: thread_id.clone(),
            item: codeterm_types::agent::chat_types::ChatMsg {
                id: format!("{thread_id}:turn-request-failure:{}", pending.send_id),
                kind: "status".into(),
                content: message,
                ..Default::default()
            },
        },
        AgentRuntimeEvent::TurnState {
            thread_id,
            turn_id,
            parent_thread_id: None,
            state: RuntimeTurnState::Idle,
            detail: Some(detail),
            status_line: None,
            started_at: None,
            completed_at: Some(now()),
        },
    ]);
    subscriber::publish_events(svc, session, pending.context.epoch, events, now()).await
}

pub(super) fn waiting(session: &Session, epoch: u64) -> bool {
    session
        .turn_requests
        .lock()
        .values()
        .any(|pending| pending.context.epoch == epoch && !pending.settled.load(Ordering::Acquire))
}

pub(crate) const ACTIVE_TURN_HOLD: &str = "held_active_turn";

/// A declared turn-scoped submit owns its turn until its reply, so another submit while a turn
/// is outstanding would be a concurrent prompt the runtime never declared it accepts.
pub(crate) fn holds_new_submit(session: &Session) -> bool {
    if !submit_owns_turn(session) {
        return false;
    }
    let (active, epoch) = {
        let context = session.context.lock();
        (context.active_turn_id.is_some(), context.epoch)
    };
    active || waiting(session, epoch)
}

/// Input joins a running turn only when this agent declared it and a prompt of ours owns that turn.
/// A command starts its own turn, so it waits like any other submit.
pub(crate) fn joins_running_turn(session: &Session, text: &str) -> bool {
    let declared = session.state.lock().mid_turn_input;
    if text.starts_with('/') || !declared || !submit_owns_turn(session) {
        return false;
    }
    let (active, epoch) = {
        let context = session.context.lock();
        (context.active_turn_id.is_some(), context.epoch)
    };
    active && waiting(session, epoch)
}

pub(crate) fn holds_submit(session: &Session, text: &str) -> bool {
    holds_new_submit(session) && !joins_running_turn(session, text)
}

/// This submit was admitted into a running turn: it opens no turn of its own.
pub(super) fn submit_joins_turn(session: &Session, operation: &RuntimeOperation) -> bool {
    matches!(
        operation,
        RuntimeOperation::SubmitTurn {
            active_turn_id: Some(_),
            ..
        }
    ) && session.state.lock().mid_turn_input
}

/// A request aimed at the turn already running reports its outcome in its reply; that turn's own
/// prompt keeps ownership of when the turn ends.
pub(super) fn call_joins_turn(operation: &RuntimeOperation, call: &RpcCall) -> bool {
    matches!(
        operation,
        RuntimeOperation::SubmitTurn { active_turn_id: Some(active), .. }
            if call.target.as_ref().and_then(|target| target.turn_id.as_ref()) == Some(active)
    )
}

pub(super) fn submit_owns_turn(session: &Session) -> bool {
    session
        .adapter
        .lock()
        .clone()
        .and_then(|adapter| adapter.agent_runtime())
        .is_some_and(|capability| {
            capability
                .turn_scoped_requests
                .iter()
                .any(|kind| kind == "submitTurn")
        })
}

pub(super) fn open_turns(session: &Session, context: &RuntimeContext) -> Vec<String> {
    session
        .turn_requests
        .lock()
        .values()
        .filter(|pending| {
            pending.context.epoch == context.epoch
                && pending.context.binding_nonce == context.binding_nonce
                && pending.context.thread_id == context.thread_id
                && !pending.settled.load(Ordering::Acquire)
        })
        .map(|pending| pending.turn.clone())
        .collect()
}

/// With no request awaiting a reply the provider has no turn to cancel, whatever the host recorded.
pub(super) fn no_provider_turn(session: &Session, epoch: u64) -> bool {
    submit_owns_turn(session) && !waiting(session, epoch)
}

pub(super) async fn cancel_timed_out(
    svc: &ServiceContext,
    session: &Arc<Session>,
    epoch: u64,
) -> Result<bool, RuntimeFailure> {
    let pending = session
        .turn_requests
        .lock()
        .values()
        .filter(|pending| pending.context.epoch == epoch)
        .cloned()
        .collect::<Vec<_>>();
    for turn in &pending {
        settle_failure(svc, session, turn, RuntimeTurnDetail::Interrupted,
            "The agent did not confirm cancellation before the cancel grace expired; the turn was interrupted.".into()).await?;
        turn.cancelled.send_replace(true);
    }
    Ok(!pending.is_empty())
}

/// A runtime that stopped has not failed the turn: restart and recovery own what happens next,
/// so this must not read as a provider failure that ends automatic recovery.
fn reply_failure_detail(reason: &RuntimeFailure) -> RuntimeTurnDetail {
    match reason {
        RuntimeFailure::ProcessExited { .. }
        | RuntimeFailure::PipeEof
        | RuntimeFailure::PipeFailure { .. }
        | RuntimeFailure::Superseded => RuntimeTurnDetail::Interrupted,
        _ => RuntimeTurnDetail::Failed,
    }
}

pub(super) async fn end_for_lifecycle(svc: &ServiceContext, session: &Arc<Session>, reason: &str) {
    let pending = session
        .turn_requests
        .lock()
        .values()
        .cloned()
        .collect::<Vec<_>>();
    for turn in pending {
        let _ = settle_failure(
            svc,
            session,
            &turn,
            RuntimeTurnDetail::Interrupted,
            reason.into(),
        )
        .await;
        turn.cancelled.send_replace(true);
        if turn.outcome.borrow().is_none() {
            turn.outcome
                .send_replace(Some(Err(RuntimeFailure::Refused(reason.into()))));
        }
    }
}

fn now() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

#[cfg(test)]
pub(super) mod tests;
