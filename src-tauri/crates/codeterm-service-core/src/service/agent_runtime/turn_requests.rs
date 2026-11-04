use super::{
    engine::{self, Session},
    model::RuntimeFailure,
    process::Process,
    rpc, subscriber,
};
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::watch;

pub(super) struct PendingTurn {
    context: RuntimeContext,
    send_id: String,
    settled: AtomicBool,
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
        || pending.settled.load(Ordering::Acquire)
        || !pending.witness.written()
    {
        return false;
    }
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
                    RuntimeTurnDetail::Failed,
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
    });
    loop {
        if let Some(result) = outcome.borrow_and_update().clone() {
            return result;
        }
        tokio::select! {
            changed = outcome.changed() => { changed.map_err(|_| RuntimeFailure::PipeEof)?; },
            written = witness.wait() => {
                written?;
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
    if pending.settled.swap(true, Ordering::AcqRel) {
        return Ok(result);
    }
    let current = session.context.lock().clone();
    let events = events
        .unwrap_or_default()
        .into_iter()
        .filter(|event| super::model::accepts_response_event(&pending.context, &current, event))
        .collect();
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
    subscriber::publish_events(
        svc,
        session,
        pending.context.epoch,
        vec![
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
        ],
        now(),
    )
    .await
}

pub(super) fn waiting(session: &Session, epoch: u64) -> bool {
    session
        .turn_requests
        .lock()
        .values()
        .any(|pending| pending.context.epoch == epoch && !pending.settled.load(Ordering::Acquire))
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
            RuntimeTurnDetail::Failed,
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
