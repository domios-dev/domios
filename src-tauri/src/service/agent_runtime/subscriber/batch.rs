use super::super::engine::{self, Session};
use super::super::model::RuntimeFailure;
use super::super::process::Process;
use super::bookkeeping::{self, Step};
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::*;
use std::sync::Arc;

pub enum Poll<T> {
    Frame(T),
    Empty,
    Lagged,
    Closed,
}

pub enum Drained<T> {
    Ready { frames: Vec<T>, lagged: bool },
    Closed { frames: Vec<T> },
}

pub fn drain_ready<T>(first: Poll<T>, mut poll: impl FnMut() -> Poll<T>, cap: usize) -> Drained<T> {
    let mut frames = Vec::new();
    let mut lagged = false;
    let mut closed = false;
    let mut stop = false;
    let mut take = |item: Poll<T>| match item {
        Poll::Frame(frame) => frames.push(frame),
        Poll::Empty => stop = true,
        Poll::Lagged => {
            lagged = true;
            stop = true;
        }
        Poll::Closed => {
            closed = true;
            stop = true;
        }
    };
    take(first);
    while !stop && frames.len() < cap {
        take(poll());
    }
    if closed {
        Drained::Closed { frames }
    } else {
        Drained::Ready { frames, lagged }
    }
}

pub(super) async fn handle(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    epoch: u64,
    frames: Vec<RuntimeFrame>,
    lagged: bool,
) -> Step {
    if session.context.lock().epoch != epoch || engine::validate_binding(svc, session).is_err() {
        return Step::Break;
    }
    let mut reducible = Vec::new();
    let mut accepted = Vec::new();
    for frame in frames {
        match frame {
            RuntimeFrame::Notification { method, params } => {
                if notification_allowed(session, &method) {
                    reducible.push(RuntimeFrame::Notification { method, params });
                }
            }
            RuntimeFrame::Request {
                request_id,
                method,
                params,
            } => {
                let refused = {
                    let mut pending = session.prompts.lock();
                    if super::outstanding_bound_exceeded(pending.len())
                        || pending.contains_key(&request_id)
                    {
                        true
                    } else {
                        pending.insert(
                            request_id.clone(),
                            engine::OutstandingRequest {
                                request: PendingRuntimeRequest {
                                    request_id: request_id.clone(),
                                    method: method.clone(),
                                    params: params.clone(),
                                },
                                prompt: None,
                            },
                        );
                        false
                    }
                };
                if refused {
                    if let Err(reason) =
                        super::reply_unhandled(&process.client, &request_id, -32601).await
                    {
                        return Step::Close(reason);
                    }
                    continue;
                }
                accepted.push(request_id.clone());
                reducible.push(RuntimeFrame::Request {
                    request_id,
                    method,
                    params,
                });
            }
        }
    }
    if reducible.is_empty() {
        return if lagged {
            bookkeeping::account(svc, session, process, epoch, "lag").await
        } else {
            Step::Continue
        };
    }
    let adapter = session.adapter.lock().clone();
    let Some(adapter) = adapter else {
        for id in &accepted {
            session.prompts.lock().remove(id);
            if let Err(reason) = super::reply_unhandled(&process.client, id, -32601).await {
                return Step::Close(reason);
            }
        }
        return Step::Break;
    };
    let (context, state) = (
        session.context.lock().clone(),
        session.hook_state.lock().clone(),
    );
    let result = tokio::time::timeout(
        RUNTIME_HOOK_DEADLINE,
        adapter.runtime_reduce(&context, &state, &reducible),
    )
    .await;
    let reduced = match result {
        Ok(Ok(reduced)) => reduced,
        Ok(Err(_)) | Err(_) => {
            return fail_requests(svc, session, process, epoch, &accepted, "hook").await;
        }
    };
    let received_at = chrono::Utc::now().timestamp_millis().max(0) as u64;
    if session.context.lock().epoch != epoch {
        return Step::Break;
    }
    let valid_events = reduced
        .events
        .iter()
        .cloned()
        .map(|event| HostAgentRuntimeEvent {
            at: received_at,
            event,
        })
        .collect::<Vec<_>>();
    if super::super::model::validate_state_size(&reduced.state).is_err()
        || reduced.events.len() > MAX_RUNTIME_BATCH
        || validate_host_runtime_events(&context, &valid_events).is_err()
    {
        return fail_requests(svc, session, process, epoch, &accepted, "limit").await;
    }
    for id in &accepted {
        if let Some(error) = super::unmapped_request_error(&context, id, &reduced.events) {
            session.prompts.lock().remove(id);
            if let Err(reason) = process.client.reply(id, Err(error)).await {
                return Step::Close(reason);
            }
        }
    }
    *session.hook_state.lock() = reduced.state;
    if let Err(reason) =
        super::apply::publish_events(svc, session, epoch, reduced.events, received_at).await
    {
        return match reason {
            RuntimeFailure::Superseded => Step::Break,
            reason if bookkeeping_protocol(&reason) => Step::Close(reason),
            _ => bookkeeping::account(svc, session, process, epoch, "publish").await,
        };
    }
    bookkeeping::reset(session);
    if lagged {
        bookkeeping::account(svc, session, process, epoch, "lag").await
    } else {
        Step::Continue
    }
}

fn bookkeeping_protocol(reason: &RuntimeFailure) -> bool {
    matches!(
        reason,
        RuntimeFailure::PipeEof
            | RuntimeFailure::PipeFailure { .. }
            | RuntimeFailure::ProcessExited { .. }
            | RuntimeFailure::InvalidFrame(_)
    )
}

async fn fail_requests(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    epoch: u64,
    accepted: &[RpcId],
    reason: &str,
) -> Step {
    for id in accepted {
        session.prompts.lock().remove(id);
        if let Err(reason) = super::reply_unhandled(&process.client, id, -32602).await {
            return Step::Close(reason);
        }
    }
    bookkeeping::account(svc, session, process, epoch, reason).await
}

fn notification_allowed(session: &Session, method: &str) -> bool {
    session
        .adapter
        .lock()
        .as_ref()
        .and_then(|adapter| adapter.agent_runtime())
        .is_some_and(|capability| {
            capability
                .notification_methods
                .iter()
                .any(|item| item == method)
        })
}
