use super::super::{capture, owned_context, rpc, subscriber};
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

pub(in crate::service::agent_runtime) async fn plan(
    session: &Session,
    context: &RuntimeContext,
    operation: &RuntimeOperation,
) -> Result<RuntimePlan, RuntimeFailure> {
    let adapter = session
        .adapter
        .lock()
        .clone()
        .ok_or(RuntimeFailure::Superseded)?;
    tokio::time::timeout(
        RUNTIME_HOOK_DEADLINE,
        adapter.runtime_plan(context, operation),
    )
    .await
    .map_err(|_| RuntimeFailure::Deadline)?
    .map_err(|reason| RuntimeFailure::HookFailed(reason.to_string()))
}

pub async fn run(
    svc: &ServiceContext,
    _provider: &str,
    session: &Arc<Session>,
    process: &Arc<Process>,
    operation: RuntimeOperation,
) -> Result<RuntimeResult, RuntimeFailure> {
    run_tracked(svc, session, process, operation, &AtomicBool::new(false)).await
}

/// Also reports whether any frame reached the runtime writer, so a failure before it is a proven zero-write.
pub(crate) async fn run_submit(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    operation: RuntimeOperation,
) -> (Result<RuntimeResult, RuntimeFailure>, bool) {
    let handed_off = AtomicBool::new(false);
    let result = run_tracked(svc, session, process, operation, &handed_off).await;
    (result, handed_off.load(Ordering::Acquire))
}

async fn run_tracked(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    operation: RuntimeOperation,
    handed_off: &AtomicBool,
) -> Result<RuntimeResult, RuntimeFailure> {
    tokio::time::timeout(
        std::time::Duration::from_secs(60),
        run_inner(svc, session, process, operation, handed_off),
    )
    .await
    .map_err(|_| RuntimeFailure::Deadline)?
}

async fn run_inner(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    operation: RuntimeOperation,
    handed_off: &AtomicBool,
) -> Result<RuntimeResult, RuntimeFailure> {
    let mut context = session.context.lock().clone();
    crate::service::execution_sandbox::validate_runtime_operation(svc, &context, &operation)
        .map_err(RuntimeFailure::Refused)?;
    super::model::validate_operation_thread(&context, &operation)?;
    if matches!(operation, RuntimeOperation::SubmitTurn { .. })
        && session.compaction.lock().held(context.epoch)
    {
        return Err(RuntimeFailure::Refused(
            "Context restoration pending or failed; sends held".into(),
        ));
    }
    if let RuntimeOperation::Answer { request_id, answer } = &operation {
        let requests = session.prompts.lock();
        let pending = requests.get(request_id).ok_or(RuntimeFailure::Superseded)?;
        let prompt = pending.prompt.as_ref().ok_or(RuntimeFailure::Superseded)?;
        super::model::validate_prompt_answer(prompt, answer)?;
        context.pending_request = Some(pending.request.clone());
    }
    if matches!(operation, RuntimeOperation::SubmitTurn { .. }) {
        process.begin_turn().await?;
    }
    let adapter = session
        .adapter
        .lock()
        .clone()
        .ok_or(RuntimeFailure::Superseded)?;
    let capability = adapter.agent_runtime().ok_or(RuntimeFailure::Superseded)?;
    validate_runtime_operation(&context, &operation)
        .map_err(|reason| RuntimeFailure::Refused(reason.to_string()))?;
    let mut next = plan(session, &context, &operation).await?;
    let mut history = RuntimeHistory::default();
    let mut steps = 0;
    loop {
        steps += 1;
        if steps > MAX_RUNTIME_STEPS {
            return Err(RuntimeFailure::HookFailed("Operation step limit".into()));
        }
        validate_binding(svc, session)?;
        if session.context.lock().epoch != context.epoch {
            return Err(RuntimeFailure::Superseded);
        }
        if matches!(&operation, RuntimeOperation::Answer { .. })
            && !matches!(&next.action, RuntimeAction::Reply { .. })
        {
            return Err(RuntimeFailure::Refused(
                "Answer requires an own request reply".into(),
            ));
        }
        validate_runtime_plan(&capability, &context, &operation, &next)
            .map_err(|reason| RuntimeFailure::Refused(reason.to_string()))?;
        let reply = match &next.action {
            RuntimeAction::QuestionAnswer { .. } => {
                return Err(RuntimeFailure::Refused(
                    "Question formatting must enter the durable user submit door".into(),
                ));
            }
            RuntimeAction::Request { call } => {
                log::debug!(
                    "[runtime-rpc] tab={} operation={} method={}",
                    context.tab_id,
                    operation.kind(),
                    call.method
                );
                if matches!(operation, RuntimeOperation::ReadHistory { .. })
                    && !history.admit_request(&call.method, &call.params.to_string())
                {
                    return Err(RuntimeFailure::HookFailed(
                        "Repeated history request".into(),
                    ));
                }
                if !super::model::permits(&capability, &operation, &call.method) {
                    return Err(RuntimeFailure::Refused(
                        "RPC method outside declared contract".into(),
                    ));
                }
                let owner = svc.clone();
                let runtime = session.clone();
                let epoch = context.epoch;
                let captured_send = match &operation {
                    RuntimeOperation::SubmitTurn { send_id, .. } => Some(send_id.clone()),
                    _ => None,
                };
                let captured_read = matches!(operation, RuntimeOperation::ReadHistory { .. })
                    .then(|| operation.clone());
                let guard: rpc::PrewriteGuard = Arc::new(move || {
                    if runtime.context.lock().epoch != epoch {
                        return Err(RuntimeFailure::Superseded);
                    }
                    validate_binding(&owner, &runtime)?;
                    if let Some(read) = &captured_read {
                        super::model::validate_operation_thread(&runtime.context.lock(), read)?;
                    }
                    if let Some(send) = &captured_send {
                        if !runtime.capacity_retry.lock().permits_send(send) {
                            return Err(RuntimeFailure::Refused(
                                "Automatic retry cancelled before write".into(),
                            ));
                        }
                        if runtime.compaction.lock().held(epoch) {
                            return Err(RuntimeFailure::Refused(
                                "Context restoration pending or failed; sends held".into(),
                            ));
                        }
                        let tab = runtime.context.lock().tab_id.clone();
                        capture::validate_if_present(&owner, &tab, send)?;
                    }
                    Ok(())
                });
                handed_off.store(true, Ordering::Release);
                process.client.request_guarded(call, Some(guard)).await?
            }
            RuntimeAction::Notification { notification } => {
                if !super::model::permits(&capability, &operation, &notification.method) {
                    return Err(RuntimeFailure::Refused(
                        "Notification outside declared contract".into(),
                    ));
                }
                handed_off.store(true, Ordering::Release);
                process.client.notify(notification).await?;
                Ok(Value::Null)
            }
            RuntimeAction::Reply { request_id, result } => {
                if !matches!(&operation,RuntimeOperation::Answer{request_id:expected,..} if expected==request_id)
                    || !session.prompts.lock().contains_key(request_id)
                {
                    return Err(RuntimeFailure::Refused(
                        "Reply does not belong to a pending own request".into(),
                    ));
                }
                let reply = match result {
                    RpcReply::Success { value } => Ok(value.clone()),
                    RpcReply::Error { error } => Err(error.clone()),
                };
                handed_off.store(true, Ordering::Release);
                process.client.reply(request_id, reply).await?;
                session.prompts.lock().remove(request_id);
                Ok(Value::Null)
            }
        };
        let refused = reply.is_err();
        let reply = match reply {
            Ok(value) => RpcReply::Success { value },
            Err(error) => RpcReply::Error { error },
        };
        let step = tokio::time::timeout(
            RUNTIME_HOOK_DEADLINE,
            adapter.runtime_response(&context, &operation, &next.state, &reply),
        )
        .await
        .map_err(|_| RuntimeFailure::Deadline)?
        .map_err(|reason| RuntimeFailure::HookFailed(reason.to_string()))?;
        validate_runtime_step(&capability, &context, &operation, &reply, &step)
            .map_err(|reason| RuntimeFailure::Refused(reason.to_string()))?;
        validate_binding(svc, session)?;
        if session.context.lock().epoch != context.epoch {
            return Err(RuntimeFailure::Superseded);
        }
        match step {
            RuntimeStep::Continue {
                history: chunk,
                plan,
                before_acceptance,
            } => {
                if matches!(operation, RuntimeOperation::SubmitTurn { .. })
                    && (!refused || !before_acceptance)
                {
                    return Err(RuntimeFailure::Refused(
                        "Turn handoff cannot be replayed".into(),
                    ));
                }
                if let Some(chunk) = chunk {
                    history.append(chunk);
                    steps = 0;
                }
                next = plan;
            }
            RuntimeStep::Refused { reason, .. } => {
                return Err(RuntimeFailure::Refused(reason.to_string()));
            }
            RuntimeStep::Complete {
                result,
                notification,
                events,
            } => {
                if let Some(notification) = notification {
                    if !super::model::permits(&capability, &operation, &notification.method) {
                        return Err(RuntimeFailure::Refused(
                            "Notification outside declared contract".into(),
                        ));
                    }
                    handed_off.store(true, Ordering::Release);
                    process.client.notify(&notification).await?;
                }
                if let Some(events) = events {
                    let current = session.context.lock().clone();
                    let events = events
                        .into_iter()
                        .filter(|event| model::accepts_response_event(&context, &current, event))
                        .collect();
                    let at = chrono::Utc::now().timestamp_millis().max(0) as u64;
                    subscriber::publish_events(svc, session, context.epoch, events, at).await?;
                }
                if let RuntimeResult::HistoryPage {
                    ref thread_id,
                    ref next_cursor,
                    ..
                } = result
                {
                    let thread = thread_id.clone();
                    let cursor = next_cursor.clone();
                    history.append(result);
                    return Ok(history.finish(thread, cursor));
                }
                owned_context::record(svc, session, &context, &operation, &result)?;
                return Ok(result);
            }
        }
    }
}
