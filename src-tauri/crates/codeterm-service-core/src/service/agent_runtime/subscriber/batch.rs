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
    let mut item = first;
    loop {
        match item {
            Poll::Frame(frame) => frames.push(frame),
            Poll::Empty => break,
            Poll::Lagged => {
                lagged = true;
                break;
            }
            Poll::Closed => {
                closed = true;
                break;
            }
        }
        if frames.len() >= cap {
            break;
        }
        item = poll();
    }
    if closed {
        Drained::Closed { frames }
    } else {
        Drained::Ready { frames, lagged }
    }
}

pub(super) async fn handle_stamped(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    epoch: u64,
    frames: Vec<super::super::rpc::Ingress>,
    lagged: bool,
) -> Step {
    let through = frames.last().map_or(0, |frame| frame.sequence);
    let lagged = lagged || frames.iter().any(|frame| frame.frame.is_none());
    let (frames, sequences) = frames
        .into_iter()
        .filter_map(|entry| entry.frame.map(|frame| (frame, entry.sequence)))
        .unzip();
    let result = handle(svc, session, process, epoch, frames, sequences, lagged).await;
    if session.context.lock().epoch == epoch {
        let previous = *session.stream_progress.borrow();
        session.stream_progress.send_replace(previous.max(through));
    }
    crate::service::provider_accounts::runtime_boundary(svc, session);
    result
}

pub(super) async fn handle(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    epoch: u64,
    frames: Vec<RuntimeFrame>,
    sequences: Vec<u64>,
    lagged: bool,
) -> Step {
    if session.context.lock().epoch != epoch || engine::validate_binding(svc, session).is_err() {
        return Step::Break;
    }
    process.client.trace_binding(&session.context.lock());
    let mut reducible = Vec::new();
    let mut reducible_sequences = Vec::new();
    let mut accepted = Vec::new();
    let mut native = Vec::new();
    let adapter = session.adapter.lock().clone();
    let capability = adapter.as_ref().and_then(|adapter| adapter.agent_runtime());
    let bridges_declared = capability
        .as_ref()
        .is_some_and(|cap| cap.client_bridges.is_some());
    let keep_interactive = capability
        .as_ref()
        .is_some_and(|cap| cap.http_sse.is_some());
    for (frame, sequence) in frames.into_iter().zip(sequences) {
        match frame {
            RuntimeFrame::Notification { method, params } => {
                if notification_allowed(session, &method) {
                    reducible.push(RuntimeFrame::Notification { method, params });
                    reducible_sequences.push(sequence);
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
                                permission: None,
                            },
                        );
                        false
                    }
                };
                if refused {
                    if keep_interactive {
                        log::debug!("[runtime-prompt] request={request_id:?} left pending at host admission limit");
                        continue;
                    }
                    if let Err(reason) =
                        super::reply_unhandled(&process.client, &request_id, -32601).await
                    {
                        return Step::Close(reason);
                    }
                    continue;
                }
                accepted.push(request_id.clone());
                if bridges_declared && super::super::client_bridge::method_supported(&method) {
                    native.push((request_id.clone(), method.clone(), params.clone()));
                }
                reducible_sequences.push(sequence);
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
            bookkeeping::account(svc, session, process, epoch, bookkeeping::Reason::Lag).await
        } else {
            Step::Continue
        };
    }
    let Some(adapter) = adapter else {
        for id in &accepted {
            session.prompts.lock().remove(id);
            if let Err(reason) = super::reply_unhandled(&process.client, id, -32601).await {
                return Step::Close(reason);
            }
        }
        return Step::Break;
    };
    let mut context = session.context.lock().clone();
    if adapter
        .agent_runtime()
        .is_some_and(|cap| !cap.turn_scoped_requests.is_empty())
    {
        context.stream_window =
            Some(super::super::turn_requests::stream_window(session, reducible_sequences).await);
    }
    let trace = super::super::model::responsiveness::context(&context);
    if let Some(trace) = trace {
        trace.mark("reducer_prepare_begin", None, serde_json::json!({}));
    }
    let state = session.hook_state.lock().clone();
    if let Some(trace) = trace {
        trace.batch(&reducible, &state);
    }
    let invocation = session.invocation.lock().clone();
    let result = codeterm_plugin::provider_account::scope(
        invocation,
        tokio::time::timeout(
            RUNTIME_HOOK_DEADLINE,
            adapter.runtime_reduce(&context, &state, &reducible),
        ),
    )
    .await;
    if let Some(trace) = trace {
        trace.mark(
            "reducer_return",
            None,
            serde_json::json!({"ok": matches!(&result, Ok(Ok(_)))}),
        );
    }
    let reduced = match result {
        Ok(Ok(reduced)) => reduced,
        Ok(Err(reason)) => {
            log::debug!(
                "[agent-runtime] tab={} epoch={epoch} reducer failed: {reason}",
                context.tab_id
            );
            return fail_requests(
                svc,
                session,
                process,
                epoch,
                &accepted,
                bookkeeping::Reason::Hook,
                keep_interactive,
            )
            .await;
        }
        Err(reason) => {
            log::debug!(
                "[agent-runtime] tab={} epoch={epoch} reducer deadline: {reason}",
                context.tab_id
            );
            return fail_requests(
                svc,
                session,
                process,
                epoch,
                &accepted,
                bookkeeping::Reason::Hook,
                keep_interactive,
            )
            .await;
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
    let state_bytes = super::super::model::validated_state_bytes(&reduced.state);
    if let Some(trace) = trace {
        trace.mark("reducer_validated", None, serde_json::json!({"state_bytes":state_bytes.as_ref().ok(),"events":reduced.events.len()}));
    }
    let native_prompt = reduced.events.iter().any(|event| match event {
        AgentRuntimeEvent::Prompt { request_id, .. }
        | AgentRuntimeEvent::PromptResolved { request_id, .. } => {
            native.iter().any(|(id, _, _)| id == request_id)
        }
        _ => false,
    });
    if native_prompt
        || state_bytes.is_err()
        || reduced.events.len() > MAX_RUNTIME_BATCH
        || validate_host_runtime_events(&context, &valid_events).is_err()
    {
        return fail_requests(
            svc,
            session,
            process,
            epoch,
            &accepted,
            bookkeeping::Reason::Limit,
            keep_interactive,
        )
        .await;
    }
    let mut deferred = Vec::new();
    for id in &accepted {
        if native.iter().any(|(native_id, _, _)| native_id == id) {
            continue;
        }
        if let Some(error) = super::unmapped_request_error(&context, id, &reduced.events) {
            if keep_interactive {
                deferred.push(id.clone());
                continue;
            }
            session.prompts.lock().remove(id);
            if let Err(reason) = process.client.reply(id, Err(error)).await {
                return Step::Close(reason);
            }
        }
    }
    let refresh_background = reduced
        .events
        .iter()
        .any(super::super::background::changed_by)
        || lagged;
    *session.hook_state.lock() = reduced.state;
    if let Err(reason) =
        super::apply::publish_live_events(svc, session, epoch, reduced.events, received_at).await
    {
        for (id, _, _) in &native {
            session.prompts.lock().remove(id);
            if let Err(reason) = process
                .client
                .reply(
                    id,
                    Err(RpcFailure {
                        code: -32602,
                        message: "Native client turn publication failed".into(),
                        data: None,
                    }),
                )
                .await
            {
                return Step::Close(reason);
            }
        }
        return match reason {
            RuntimeFailure::Superseded => Step::Break,
            reason if bookkeeping_protocol(&reason) => Step::Close(reason),
            _ => {
                bookkeeping::account(svc, session, process, epoch, bookkeeping::Reason::Publish)
                    .await
            }
        };
    }
    if refresh_background {
        process.background_changed.notify_one();
    }
    for (id, method, params) in native {
        session.prompts.lock().remove(&id);
        let current = session.context.lock().clone();
        if current.epoch != epoch || current.active_turn_id.is_none() {
            if let Err(reason) = process
                .client
                .reply(
                    &id,
                    Err(RpcFailure {
                        code: -32602,
                        message: "Native client request requires a published active turn".into(),
                        data: None,
                    }),
                )
                .await
            {
                return Step::Close(reason);
            }
            continue;
        }
        if let Err(reason) =
            super::super::client_bridge::handle(svc, session, process, &id, &method, &params).await
        {
            return Step::Close(reason);
        }
    }
    recover_interactive(svc, session, epoch, &deferred).await;
    if session
        .prompts_cancelled
        .load(std::sync::atomic::Ordering::Acquire)
    {
        if let Err(reason) =
            super::super::prompt_answers::cancel_pending(svc, session, process).await
        {
            return Step::Close(reason);
        }
    }
    if lagged {
        bookkeeping::account(svc, session, process, epoch, bookkeeping::Reason::Lag).await
    } else {
        Step::Continue
    }
}

fn bookkeeping_protocol(reason: &RuntimeFailure) -> bool {
    matches!(
        reason,
        RuntimeFailure::PipeEof
            | RuntimeFailure::PipeFailure { .. }
            | RuntimeFailure::SseEventTooLarge { .. }
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
    reason: bookkeeping::Reason,
    keep_interactive: bool,
) -> Step {
    if keep_interactive {
        let step = bookkeeping::account(svc, session, process, epoch, reason).await;
        if matches!(step, Step::Continue) {
            recover_interactive(svc, session, epoch, accepted).await;
        }
        return step;
    }
    for id in accepted {
        session.prompts.lock().remove(id);
        if let Err(reason) = super::reply_unhandled(&process.client, id, -32602).await {
            return Step::Close(reason);
        }
    }
    bookkeeping::account(svc, session, process, epoch, reason).await
}

async fn recover_interactive(
    svc: &ServiceContext,
    session: &Arc<Session>,
    epoch: u64,
    ids: &[RpcId],
) {
    if ids.is_empty() {
        return;
    }
    let context = session.context.lock().clone();
    if context.epoch != epoch || engine::validate_binding(svc, session).is_err() {
        return;
    }
    let Some(adapter) = session.adapter.lock().clone() else {
        return;
    };
    let state = session.hook_state.lock().clone();
    let frames = {
        let pending = session.prompts.lock();
        ids.iter()
            .filter_map(|id| {
                let request = &pending.get(id)?.request;
                Some(RuntimeFrame::Request {
                    request_id: request.request_id.clone(),
                    method: request.method.clone(),
                    params: request.params.clone(),
                })
            })
            .collect::<Vec<_>>()
    };
    if frames.is_empty() {
        return;
    }
    let invocation = session.invocation.lock().clone();
    let reduction = codeterm_plugin::provider_account::scope(
        invocation,
        tokio::time::timeout(
            RUNTIME_HOOK_DEADLINE,
            adapter.runtime_reduce(&context, &state, &frames),
        ),
    )
    .await;
    if let Ok(Ok(reduced)) = reduction {
        let events: Vec<_> = reduced
            .events
            .into_iter()
            .filter(|event| {
                matches!(event,
                    AgentRuntimeEvent::Prompt { request_id, .. } if ids.contains(request_id)
                )
            })
            .collect();
        let all_surfaced = ids
            .iter()
            .all(|id| super::unmapped_request_error(&context, id, &events).is_none());
        let at = chrono::Utc::now().timestamp_millis().max(0) as u64;
        if !events.is_empty()
            && super::super::model::validated_state_bytes(&reduced.state).is_ok()
            && super::apply::publish_live_events(svc, session, epoch, events, at)
                .await
                .is_ok()
        {
            if session.context.lock().epoch == epoch {
                *session.hook_state.lock() = reduced.state;
            }
            if all_surfaced {
                return;
            }
        }
    }
    log::debug!(
        "[runtime-prompt] tab={} epoch={epoch} interactive requests left pending after recovery",
        context.tab_id
    );
    if session.context.lock().epoch == epoch && engine::validate_binding(svc, session).is_ok() {
        super::super::declared_controls::notice(svc, session,
            "The provider is waiting for input that could not be displayed. Stop the turn to continue.".into());
    }
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
