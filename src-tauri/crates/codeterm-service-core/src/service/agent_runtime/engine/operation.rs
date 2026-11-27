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
            "trusted" => "trusted",
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
        "projectTrust":policy_label(value.get("config").and_then(|config| config.get("projects"))
            .and_then(|projects| value.get("cwd").and_then(Value::as_str).and_then(|cwd| projects.get(cwd)))
            .and_then(|project| project.get("trust_level"))),
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
    .map_err(|reason| {
        if reason.code == codeterm_plugin::agent_runtime::RuntimeFailureCode::Deadline {
            RuntimeFailure::Deadline
        } else if reason.code == codeterm_plugin::agent_runtime::RuntimeFailureCode::ProviderRefused
        {
            RuntimeFailure::Refused(reason.message)
        } else {
            RuntimeFailure::HookFailed(reason.to_string())
        }
    })
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

struct OperationZeroizeGuard(RuntimeOperation);

impl Drop for OperationZeroizeGuard {
    fn drop(&mut self) {
        if let RuntimeOperation::Control { value, .. } = &mut self.0 {
            crate::service::secure_input::zeroize_secure_input_json_value(value);
        }
    }
}

async fn run_tracked(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    operation: RuntimeOperation,
    handed_off: &AtomicBool,
) -> Result<RuntimeResult, RuntimeFailure> {
    let mut guard = OperationZeroizeGuard(operation);
    let invocation = session.invocation.lock().clone();
    let result = codeterm_plugin::provider_account::scope(invocation, async {
        tokio::time::timeout(
            std::time::Duration::from_secs(60),
            run_inner(svc, session, process, &mut guard.0, handed_off),
        )
        .await
        .map_err(|_| RuntimeFailure::Deadline)?
    })
    .await;
    crate::service::provider_accounts::runtime_boundary(svc, session);
    result
}

async fn run_inner(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    operation: &mut RuntimeOperation,
    handed_off: &AtomicBool,
) -> Result<RuntimeResult, RuntimeFailure> {
    let tab_id = session.context.lock().tab_id.clone();
    let transcript_handoff = match &*operation {
        RuntimeOperation::SubmitTurn { send_id, .. } => {
            svc.require_transcript_handoff(&tab_id, send_id)?
        }
        _ => None,
    };
    let external_transcript_handoff = match &*operation {
        RuntimeOperation::SubmitTurn { send_id, .. } => {
            crate::service::live_transcript::external_handoff(svc, &tab_id, send_id)?
        }
        _ => None,
    };
    let switch_authority = match &*operation {
        RuntimeOperation::SubmitTurn { send_id, .. } => {
            crate::service::messenger::switch_handoff::for_send(svc, send_id)
        }
        _ => None,
    };
    let launch_authority =
        crate::service::messenger::switch_handoff::launch_authority(svc, &tab_id);
    let switch_guard: Option<rpc::PrewriteGuard> = (switch_authority.is_some()
        || launch_authority.is_some()
        || transcript_handoff.is_some()
        || external_transcript_handoff.is_some())
    .then(|| {
        let authority = switch_authority.clone();
        let launch = launch_authority.clone();
        let owner = svc.clone();
        let runtime = session.clone();
        let epoch = runtime.context.lock().epoch;
        Arc::new(move || {
            if runtime.context.lock().epoch != epoch {
                return Err(
                    if authority
                        .as_ref()
                        .is_some_and(|authority| authority.was_claimed())
                    {
                        RuntimeFailure::WrittenAwaitingEvidence
                    } else {
                        RuntimeFailure::Superseded
                    },
                );
            }
            if let Some(launch) = &launch {
                launch
                    .validate(&owner)
                    .map_err(|_| RuntimeFailure::Superseded)?;
            }
            if let Some(authority) = &authority {
                authority.claim(&owner)?;
            }
            if let Some(handoff) = &transcript_handoff {
                handoff.claim(&owner)?;
            }
            if let Some(handoff) = &external_transcript_handoff {
                handoff.claim(&owner)?;
            }
            Ok(())
        }) as rpc::PrewriteGuard
    });
    let mut context = session.context.lock().clone();
    let turn_starts = matches!(operation, RuntimeOperation::SubmitTurn { .. })
        .then(|| svc.runtime_turn_starts(&context.tab_id, &context.binding_nonce, context.epoch));
    let before_turn_start = turn_starts.as_ref().map(|starts| starts.sequence());
    let before_completion = turn_starts
        .as_ref()
        .map(|_| session.completions.lock().sequence());
    let mut fallback_settings = None;
    #[cfg(debug_assertions)]
    let submit_stages = {
        let mut started = std::time::Instant::now();
        let tab = context.tab_id.clone();
        let send_id = match &*operation {
            RuntimeOperation::SubmitTurn { send_id, .. } => Some(send_id.clone()),
            _ => None,
        };
        move |stage: &str| {
            let now = std::time::Instant::now();
            if let Some(send_id) = &send_id {
                log::debug!(
                    "[delivery-stage] tab={tab} send_id={send_id} stage={stage} elapsed_us={}",
                    now.duration_since(started).as_micros()
                );
            }
            started = now;
        }
    };
    #[cfg(debug_assertions)]
    let mut submit_stages = submit_stages;
    let cli_validation =
        super::super::cli_exception::validate_before_operation(svc, &context).await;
    #[cfg(debug_assertions)]
    submit_stages("cli_validation");
    cli_validation?;
    let declined = super::super::permission_policy::declined(session, operation);
    crate::service::execution_sandbox::validate_runtime_operation(
        svc,
        &context,
        declined.as_ref().unwrap_or(&*operation),
    )
    .map_err(RuntimeFailure::Refused)?;
    super::model::validate_operation_thread(&context, operation)?;
    if matches!(operation, RuntimeOperation::SubmitTurn { .. }) {
        crate::service::provider_accounts::invalidate_boundary(svc, &context.tab_id);
    }
    if matches!(operation, RuntimeOperation::SubmitTurn { .. })
        && session.compaction.lock().held(context.epoch)
    {
        return Err(RuntimeFailure::Refused(
            "Context restoration pending or failed; sends held".into(),
        ));
    }
    if let RuntimeOperation::Answer { request_id, answer } = &*operation {
        let requests = session.prompts.lock();
        let pending = requests.get(request_id).ok_or(RuntimeFailure::Superseded)?;
        let prompt = pending.prompt.as_ref().ok_or(RuntimeFailure::Superseded)?;
        super::model::validate_prompt_answer(prompt, answer)?;
        context.pending_request = Some(pending.request.clone());
    }
    if matches!(operation, RuntimeOperation::SubmitTurn { .. })
        && !super::super::turn_requests::submit_joins_turn(session, operation)
    {
        session.prompts_cancelled.store(false, Ordering::Release);
        super::super::client_bridge::begin_turn(&context);
        let snapshot = process.begin_turn().await;
        #[cfg(debug_assertions)]
        submit_stages("process_identity_snapshot");
        snapshot?;
    }
    let adapter = session
        .adapter
        .lock()
        .clone()
        .ok_or(RuntimeFailure::Superseded)?;
    let capability = adapter.agent_runtime().ok_or(RuntimeFailure::Superseded)?;
    validate_runtime_operation(&context, operation)
        .map_err(|reason| RuntimeFailure::Refused(reason.to_string()))?;
    let next = plan(session, &context, operation).await;
    #[cfg(debug_assertions)]
    submit_stages("runtime_plan");
    let mut next = next?;
    if matches!(operation, RuntimeOperation::SubmitTurn { .. })
        || matches!(operation, RuntimeOperation::Control { action, .. } if matches!(action.as_str(), "restoreContext" | "compact"))
    {
        let fenced = super::resume::before_turn(svc, session).await;
        #[cfg(debug_assertions)]
        submit_stages("resume_fence");
        fenced?;
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
        if matches!(operation, RuntimeOperation::Answer { .. })
            && !matches!(&next.action, RuntimeAction::Reply { .. })
        {
            return Err(RuntimeFailure::Refused(
                "Answer requires an own request reply".into(),
            ));
        }
        validate_runtime_plan(&capability, &context, operation, &next)
            .map_err(|reason| RuntimeFailure::Refused(reason.to_string()))?;
        let reply = match &next.action {
            RuntimeAction::Local => Ok(Value::Null),
            RuntimeAction::Refused { reason } => {
                return Err(RuntimeFailure::Refused(reason.clone()))
            }
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
                if !super::model::permits(&capability, operation, &call.method) {
                    return Err(RuntimeFailure::Refused(
                        "RPC method outside declared contract".into(),
                    ));
                }
                let owner = svc.clone();
                let runtime = session.clone();
                let epoch = context.epoch;
                let captured_send = match &*operation {
                    RuntimeOperation::SubmitTurn { send_id, .. } => Some(send_id.clone()),
                    _ => None,
                };
                let submit_binding = captured_send.as_ref().map(|_| {
                    (
                        context.binding_nonce.clone(),
                        context.thread_id.clone(),
                        process.clone(),
                        svc.pending_delivery_payloads
                            .user_sends
                            .tab(&context.tab_id),
                    )
                });
                let captured_read = matches!(operation, RuntimeOperation::ReadHistory { .. })
                    .then(|| operation.clone());
                let captured_login = super::super::login::declared_operation_witness(operation);
                let captured_switch = switch_guard.clone();
                let captured_claim = switch_authority.clone();
                let fallback_witness = fallback_settings.as_ref().and(turn_starts.clone());
                let guard: rpc::PrewriteGuard = Arc::new(move || {
                    let result = (|| {
                        if runtime.context.lock().epoch != epoch {
                            return Err(RuntimeFailure::Superseded);
                        }
                        validate_binding(&owner, &runtime)?;
                        if fallback_witness.as_ref().is_some_and(|starts| {
                            Some(starts.sequence()) != before_turn_start
                                || Some(runtime.completions.lock().sequence()) != before_completion
                        }) {
                            return Err(RuntimeFailure::WrittenAwaitingEvidence);
                        }
                        if let Some(witness) = &captured_login {
                            super::super::login::validate_declared_witness(&runtime, witness)?;
                        }
                        let cli_context = runtime.context.lock().clone();
                        super::super::cli_exception::validate_binding(&owner, &cli_context)
                            .map_err(RuntimeFailure::Refused)?;
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
                        if let Some((nonce, thread, process, send_owner)) = &submit_binding {
                            if cli_context.binding_nonce != *nonce
                                || cli_context.thread_id != *thread
                                || send_owner.revoked.load(Ordering::Acquire)
                                || send_owner.failed.load(Ordering::Acquire)
                                || !super::super::authority::verifies(
                                    &owner,
                                    &cli_context.tab_id,
                                    &runtime.provider,
                                    nonce,
                                )
                                || !for_tab(&cli_context.tab_id)
                                    .is_some_and(|current| Arc::ptr_eq(&current, &runtime))
                                || !runtime
                                    .process
                                    .lock()
                                    .as_ref()
                                    .is_some_and(|current| Arc::ptr_eq(current, process))
                            {
                                return Err(RuntimeFailure::Superseded);
                            }
                        }
                        if let Some(guard) = &captured_switch {
                            guard()?;
                        }
                        Ok(())
                    })();
                    result.map_err(|reason| {
                        if captured_claim
                            .as_ref()
                            .is_some_and(|authority| authority.was_claimed())
                        {
                            RuntimeFailure::WrittenAwaitingEvidence
                        } else {
                            reason
                        }
                    })
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
                #[cfg(debug_assertions)]
                submit_stages(&format!("rpc_dispatch:{}", call.method));
                if !super::super::turn_requests::call_joins_turn(operation, call)
                    && capability
                        .turn_scoped_requests
                        .iter()
                        .any(|kind| kind == operation.kind())
                {
                    return super::super::turn_requests::run(
                        svc,
                        session,
                        process,
                        context.clone(),
                        operation.clone(),
                        next.clone(),
                        Some(guard),
                    )
                    .await;
                }
                let reply = process.client.request_guarded(call, Some(guard)).await;
                #[cfg(debug_assertions)]
                submit_stages(&format!("rpc_reply:{}", call.method));
                reply?
            }
            RuntimeAction::Notification { notification } => {
                if !super::model::permits(&capability, operation, &notification.method) {
                    return Err(RuntimeFailure::Refused(
                        "Notification outside declared contract".into(),
                    ));
                }
                handed_off.store(true, Ordering::Release);
                process
                    .client
                    .notify_guarded(notification, switch_guard.clone())
                    .await?;
                Ok(Value::Null)
            }
            RuntimeAction::Reply { request_id, result } => {
                if !matches!(&*operation,RuntimeOperation::Answer{request_id:expected,..} if expected==request_id)
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
                let approval = super::super::client_bridge::approvals::approved_reply(
                    &context,
                    operation,
                    reply.as_ref().ok(),
                )
                .await;
                handed_off.store(true, Ordering::Release);
                if let Err(reason) = process.client.reply(request_id, reply).await {
                    super::super::client_bridge::approvals::revoke(&context, approval);
                    if matches!(&reason, RuntimeFailure::ReplyRefused { .. }) {
                        session.prompts.lock().remove(request_id);
                        super::super::prompt_answers::resolve(svc, session, request_id);
                    }
                    return Err(reason);
                }
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
            adapter.runtime_response(&context, operation, &next.state, &reply),
        )
        .await
        .map_err(|_| RuntimeFailure::Deadline)?
        .map_err(|reason| {
            if reason.code == RuntimeFailureCode::Deadline {
                RuntimeFailure::Deadline
            } else {
                RuntimeFailure::HookFailed(reason.to_string())
            }
        })?;
        validate_runtime_step(&capability, &context, operation, &reply, &step)
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
                if switch_authority
                    .as_ref()
                    .is_some_and(|authority| authority.was_claimed())
                {
                    return Err(RuntimeFailure::WrittenAwaitingEvidence);
                }
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
            RuntimeStep::Refused {
                reason,
                before_acceptance,
            } => {
                if reason.code == RuntimeFailureCode::ModelUnavailable {
                    use super::super::model_unavailable as unavailable;
                    if let Some(model) =
                        unavailable::settings(operation).and_then(|settings| settings.model.clone())
                    {
                        unavailable::remember(svc, session, process, &model).await;
                        let no_turn_observed = || {
                            turn_starts.as_ref().is_none_or(|starts| {
                                Some(starts.sequence()) == before_turn_start
                                    && Some(session.completions.lock().sequence())
                                        == before_completion
                            })
                        };
                        let replay_safe = before_acceptance
                            && refused
                            && no_turn_observed()
                            && switch_guard.is_none();
                        if let Some(candidate) = unavailable::choose(
                            svc,
                            session,
                            &context,
                            operation,
                            &model,
                            replay_safe,
                        )
                        .await
                        {
                            if no_turn_observed() {
                                let settings = unavailable::settings(operation)
                                    .ok_or(RuntimeFailure::Superseded)?;
                                let candidate_id = candidate.id.clone();
                                unavailable::replace_model(settings, candidate);
                                fallback_settings = Some(settings.clone());
                                unavailable::publish(svc, session, &context, "model_rerouted",
                                    format!("Model {model} is unavailable for this account. Trying {candidate_id}."), &candidate_id);
                                next = plan(session, &context, operation).await?;
                                continue;
                            }
                        }
                        unavailable::publish(
                            svc,
                            session,
                            &context,
                            unavailable::SUBTYPE,
                            reason.message.clone(),
                            &model,
                        );
                        super::super::capacity_retry::notice::model_unavailable(
                            svc,
                            &context.tab_id,
                            &context.binding_nonce,
                            &reason.message,
                        );
                        let failure = RuntimeFailure::ModelUnavailable {
                            model,
                            message: reason.message,
                        };
                        unavailable::record_launch_failure(&failure);
                        return Err(failure);
                    }
                }
                // Declined input never entered the turn, so it stays held for the next prompt.
                if before_acceptance
                    && super::super::turn_requests::submit_joins_turn(session, operation)
                {
                    if reason.code == RuntimeFailureCode::AuthenticationRequired {
                        super::super::declared_controls::require_authentication(svc, session);
                    }
                    return Err(RuntimeFailure::PrewriteRefused(reason.to_string()));
                }
                if reason.code == RuntimeFailureCode::AuthenticationRequired {
                    super::super::declared_controls::require_authentication(svc, session);
                    return Err(RuntimeFailure::SignInRequired);
                }
                if reason.code == RuntimeFailureCode::SessionLoadRefused
                    && matches!(operation, RuntimeOperation::ResumeThread { .. })
                {
                    let detail = match &reply {
                        RpcReply::Error { error } => super::super::restore::rpc_reason(error),
                        _ => reason.message.clone(),
                    };
                    return Err(RuntimeFailure::SessionLoadRefused(detail));
                }
                if reason.code == RuntimeFailureCode::ThreadNotFound
                    && matches!(operation, RuntimeOperation::ResumeThread { .. })
                {
                    return Err(RuntimeFailure::ConversationMissing);
                }
                let cause = if matches!(
                    operation,
                    RuntimeOperation::Initialize | RuntimeOperation::ResumeThread { .. }
                ) {
                    match &reply {
                        RpcReply::Error { error } => {
                            format!("{} ({})", reason, super::super::restore::rpc_reason(error))
                        }
                        _ => reason.to_string(),
                    }
                } else {
                    reason.to_string()
                };
                return Err(RuntimeFailure::Refused(cause));
            }
            RuntimeStep::Complete {
                result,
                notification,
                events,
            } => {
                if let Some(notification) = notification {
                    if !super::model::permits(&capability, operation, &notification.method) {
                        return Err(RuntimeFailure::Refused(
                            "Notification outside declared contract".into(),
                        ));
                    }
                    handed_off.store(true, Ordering::Release);
                    process
                        .client
                        .notify_guarded(&notification, switch_guard.clone())
                        .await?;
                }
                if let Some(events) = events {
                    let current = session.context.lock().clone();
                    let events = events
                        .into_iter()
                        .filter(|event| {
                            if let (
                                RuntimeOperation::OpenThread { .. }
                                | RuntimeOperation::ResumeThread { .. },
                                RuntimeResult::ThreadLoaded { thread_id, .. },
                                AgentRuntimeEvent::Notice {
                                    thread_id: notice_thread,
                                    message,
                                },
                            ) = (&*operation, &result, event)
                            {
                                if thread_id == notice_thread {
                                    session.state.lock().pending_load_notice =
                                        Some(message.clone());
                                    return false;
                                }
                            }
                            model::reply_may_report_turn(operation, event)
                                && model::accepts_response_event(&context, &current, event)
                        })
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
                owned_context::record(svc, session, &context, operation, &result)?;
                if let (RuntimeResult::TurnAccepted { .. }, Some(settings)) =
                    (&result, fallback_settings.as_ref())
                {
                    if let Err(error) = super::super::model_unavailable::persist_fallback(
                        svc, session, &context, settings,
                    ) {
                        log::warn!("[model-catalogue] could not save fallback selection: {error}");
                    }
                }
                if let (
                    RuntimeOperation::OpenThread { .. },
                    RuntimeResult::ThreadLoaded { .. },
                    Some(settings),
                ) = (&*operation, &result, fallback_settings)
                {
                    session.context.lock().settings = settings;
                }
                return Ok(result);
            }
        }
    }
}
