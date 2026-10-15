use super::{
    engine::{self, Session},
    model::{RuntimeFailure, RuntimePhase},
    process::Process,
};
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::*;
use std::sync::Arc;

#[path = "subscriber/compaction.rs"]
mod compaction;
pub(super) use compaction::CompactionState;

#[path = "subscriber/apply.rs"]
mod apply;
pub(super) use apply::publish_events;

#[path = "subscriber/bookkeeping.rs"]
mod bookkeeping;

#[path = "subscriber/batch.rs"]
mod batch;

pub fn start(svc: ServiceContext, _provider: String, session: Arc<Session>, process: Arc<Process>) {
    let mut events = process.client.subscribe();
    let mut ended = process.client.ended();
    let mut exited = process.ended.clone();
    let epoch = session.context.lock().epoch;
    crate::async_runtime::spawn(async move {
        loop {
            if session.context.lock().epoch != epoch
                || engine::validate_binding(&svc, &session).is_err()
            {
                break;
            }
            let first = match events.try_recv() {
                Ok(frame) => batch::Poll::Frame(frame),
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                    if ended.borrow().is_some() || exited.borrow().is_some() {
                        break;
                    }
                    match tokio::select! {
                        biased;
                        event = events.recv() => event,
                        _ = ended.changed() => continue,
                        _ = exited.changed() => continue,
                    } {
                        Ok(frame) => batch::Poll::Frame(frame),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            batch::Poll::Lagged
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            batch::Poll::Closed
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => batch::Poll::Lagged,
                Err(tokio::sync::broadcast::error::TryRecvError::Closed) => batch::Poll::Closed,
            };
            let drained = batch::drain_ready(
                first,
                || match events.try_recv() {
                    Ok(frame) => batch::Poll::Frame(frame),
                    Err(tokio::sync::broadcast::error::TryRecvError::Empty) => batch::Poll::Empty,
                    Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {
                        batch::Poll::Lagged
                    }
                    Err(tokio::sync::broadcast::error::TryRecvError::Closed) => batch::Poll::Closed,
                },
                MAX_RUNTIME_BATCH,
            );
            let step = match drained {
                batch::Drained::Closed { frames } => {
                    if !frames.is_empty() {
                        let _ = batch::handle(&svc, &session, &process, epoch, frames, false).await;
                    }
                    process.client.close(RuntimeFailure::PipeFailure {
                        stream: "events".into(),
                        reason: "event broadcast closed".into(),
                    });
                    break;
                }
                batch::Drained::Ready { frames, lagged } => {
                    batch::handle(&svc, &session, &process, epoch, frames, lagged).await
                }
            };
            match step {
                bookkeeping::Step::Continue => {}
                bookkeeping::Step::Break => break,
                bookkeeping::Step::Close(reason) => {
                    process.client.close(reason);
                    break;
                }
            }
        }
        if session.context.lock().epoch == epoch {
            let reason = exited
                .borrow()
                .clone()
                .or_else(|| ended.borrow().clone())
                .unwrap_or(RuntimeFailure::PipeEof);
            let expired = session.context.lock().clone();
            super::login::forget(&session);
            session.login.lock().login = None;
            super::prompt_answers::clear(&svc, &expired);
            session.prompts.lock().clear();
            if unload_retaining_degradation(&mut session.state.lock(), epoch, &reason) {
                publish_runtime_ended(&svc, &session, &expired, epoch, &reason);
                session
                    .thread_loaded
                    .store(false, std::sync::atomic::Ordering::Release);
                {
                    let mut context = session.context.lock();
                    context.active_turns.clear();
                    context.thread_parents.clear();
                    context.active_turn_id = None;
                }
                let context = session.context.lock().clone();
                for status in svc
                    .chat_send_ledger
                    .rpc_uncertain_for_launch(&context.tab_id, Some(&context.binding_nonce))
                {
                    crate::events::emit_everywhere(&svc.app_handle, "chat-send-status", status);
                }
                engine::publish(&svc, &context.tab_id, &session);
                svc.emit_state_changed();
            }
            let owner = svc.clone();
            let ending = expired;
            let detail = format!("runtime process ended: {reason}");
            let _ = tokio::task::spawn_blocking(move || {
                owner
                    .run_watcher
                    .runtime_process_exited(&owner, &ending, detail);
            })
            .await;
            process.stop();
        }
    });
}

fn publish_runtime_ended(
    svc: &ServiceContext,
    session: &Session,
    expired: &RuntimeContext,
    epoch: u64,
    reason: &RuntimeFailure,
) {
    let now = chrono::Utc::now();
    let snapshot = session.cache.lock().snapshot();
    let cause = super::abort_provenance::runtime_ended_cause(reason);
    let rows = super::abort_provenance::runtime_ended_rows(
        &snapshot,
        expired.active_turn_id.as_deref(),
        epoch,
        &cause,
        Some(now.to_rfc3339()),
    );
    if rows.is_empty() {
        return;
    }
    *session.abort_note.lock() = Some(cause);
    log::warn!(
        "[agent-runtime] tab={} thread={:?} turn={:?} turn aborted: runtime process ended: {reason}",
        expired.tab_id,
        expired.thread_id,
        expired.active_turn_id
    );
    let at = now.timestamp_millis().max(0) as u64;
    let rows = {
        let mut cache = session.cache.lock();
        rows.into_iter()
            .map(|row| cache.reconcile_live(row, at))
            .collect::<Vec<_>>()
    };
    crate::service::tab_chat::structured_sink::emit_chat_msgs(
        &svc.app_handle,
        &expired.tab_id,
        &rows,
        None,
        expired.thread_id.as_deref(),
        Some(&expired.binding_nonce),
    );
}

const MAX_OUTSTANDING_REQUESTS: usize = 64;

fn outstanding_bound_exceeded(outstanding: usize) -> bool {
    outstanding >= MAX_OUTSTANDING_REQUESTS
}

fn request_error(code: i64) -> RpcFailure {
    RpcFailure {
        code,
        message: "Request not handled by host".into(),
        data: None,
    }
}

fn unmapped_request_error(
    context: &RuntimeContext,
    id: &RpcId,
    events: &[AgentRuntimeEvent],
) -> Option<RpcFailure> {
    let surfaced = events.iter().any(|event| {
        matches!(event,
        AgentRuntimeEvent::Prompt { thread_id, request_id, .. }
        if Some(thread_id) == context.thread_id.as_ref() && request_id == id)
    });
    (!surfaced).then(|| request_error(-32601))
}

async fn reply_unhandled(
    client: &super::rpc::Client,
    id: &RpcId,
    code: i64,
) -> Result<(), RuntimeFailure> {
    client.reply(id, Err(request_error(code))).await
}

fn unload_retaining_degradation(
    state: &mut super::model::RuntimeState,
    epoch: u64,
    reason: &RuntimeFailure,
) -> bool {
    let degraded =
        matches!(state.phase, RuntimePhase::Degraded { .. }).then(|| state.phase.clone());
    if !state.unload(epoch, reason) {
        return false;
    }
    if let Some(phase) = degraded {
        state.phase = phase;
    }
    true
}

#[cfg(test)]
#[path = "subscriber/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "subscriber/batch_tests.rs"]
mod batch_tests;

#[cfg(test)]
#[path = "subscriber/fixtures.rs"]
mod fixtures;
