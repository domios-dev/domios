use super::super::{capture, owned_context, rpc, subscriber};
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(any(debug_assertions, test))]
fn policy_label(value: Option<&Value>) -> &'static str {
    match value {
        None => "missing",
        Some(Value::Null) => "null",
        Some(Value::String(label)) => match label.as_str() {
            "read-only" => "read-only",
            "read-only-with-writable-roots" => "read-only-with-writable-roots",
            "workspace-write" => "workspace-write",
            "danger-full-access" => "danger-full-access",
            "full-access" => "full-access",
            "approval-required" => "approval-required",
            "auto-accept-edits" => "auto-accept-edits",
            "auto" => "auto",
            "never" => "never",
            "on-request" => "on-request",
            "untrusted" => "untrusted",
            "user" => "user",
            "auto_review" => "auto_review",
            "readOnly" => "readOnly",
            "workspaceWrite" => "workspaceWrite",
            "dangerFullAccess" => "dangerFullAccess",
            "externalSandbox" => "externalSandbox",
            ":workspace" => ":workspace",
            ":read-only" => ":read-only",
            _ => "other",
        },
        Some(_) => "other",
    }
}

#[cfg(any(debug_assertions, test))]
fn policy_fields(value: &Value) -> Value {
    let sandbox = value.get("sandbox");
    let sandbox = sandbox
        .filter(|sandbox| !sandbox.is_object())
        .or_else(|| sandbox.and_then(|sandbox| sandbox.get("type")));
    serde_json::json!({
        "mode":policy_label(value.get("mode")),
        "approval":policy_label(value.get("approvalPolicy")),
        "reviewer":policy_label(value.get("approvalsReviewer")),
        "sandbox":policy_label(sandbox),
        "sandboxPolicy":policy_label(value.get("sandboxPolicy").and_then(|policy| policy.get("type"))),
        "profile":policy_label(value.get("activePermissionProfile").and_then(|profile| profile.get("id"))),
        "permissions":policy_label(value.get("permissions")),
        "configSandbox":policy_label(value.get("config").and_then(|config| config.get("sandbox_mode"))),
    })
}

#[cfg(any(debug_assertions, test))]
pub(super) fn policy_projection(value: &Value) -> Value {
    let nested = value.get("threadSettings");
    let selected_nested = match nested {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(value)) => value.as_f64().is_some_and(|value| value != 0.0),
        Some(Value::String(value)) => !value.is_empty(),
        Some(_) => true,
    };
    let selected = if selected_nested {
        &value["threadSettings"]
    } else {
        value
    };
    serde_json::json!({
        "threadSettingsSelected":selected_nested,
        "top":policy_fields(value),
        "topThread":policy_fields(&value["thread"]),
        "selected":policy_fields(selected),
        "selectedThread":policy_fields(&selected["thread"]),
    })
}

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
    if matches!(operation, RuntimeOperation::SubmitTurn { .. })
        || matches!(&operation, RuntimeOperation::Control { action, .. } if matches!(action.as_str(), "restoreContext" | "compact"))
    {
        super::resume::before_turn(svc, session).await?;
    }
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
                #[cfg(debug_assertions)]
                if matches!(call.method.as_str(), "thread/start" | "thread/resume") {
                    log::debug!(
                        "[runtime-policy] phase=rpc_request tab={} epoch={} method={} policy={}",
                        context.tab_id,
                        context.epoch,
                        call.method,
                        policy_projection(&call.params)
                    );
                }
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
        #[cfg(debug_assertions)]
        if let RuntimeAction::Request { call } = &next.action {
            if matches!(call.method.as_str(), "thread/start" | "thread/resume") {
                if let RpcReply::Success { value } = &reply {
                    log::debug!(
                        "[runtime-policy] phase=rpc_reply tab={} epoch={} method={} policy={}",
                        context.tab_id,
                        context.epoch,
                        call.method,
                        policy_projection(value)
                    );
                }
            }
        }
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
                if reason.code == RuntimeFailureCode::ThreadNotFound
                    && matches!(operation, RuntimeOperation::ResumeThread { .. })
                {
                    return Err(RuntimeFailure::ConversationMissing);
                }
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
                    let result = history.finish(thread, cursor);
                    if matches!(&result, RuntimeResult::HistoryPage { items, .. } if !items.is_empty())
                    {
                        super::resume::before_turn(svc, session).await?;
                    }
                    return Ok(result);
                }
                owned_context::record(svc, session, &context, &operation, &result)?;
                return Ok(result);
            }
        }
    }
}
