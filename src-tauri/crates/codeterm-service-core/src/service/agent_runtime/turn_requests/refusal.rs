use super::*;

pub(super) async fn apply(
    svc: &ServiceContext,
    session: &Arc<Session>,
    pending: &PendingTurn,
    reply: &RpcReply,
    reason: &codeterm_plugin::agent_runtime::RuntimeFailure,
    before_acceptance: bool,
) -> Result<(), RuntimeFailure> {
    // Invalid-params on this overlapping request proves rejection, never a transport timeout.
    if !eligible(
        reply,
        reason.code,
        before_acceptance,
        pending.had_predecessor,
        pending.observed.load(Ordering::Acquire),
        pending.outcome.borrow().is_some(),
    ) {
        return Ok(());
    }
    validate(svc, session, &pending.context)?;
    let thread = pending
        .context
        .thread_id
        .clone()
        .ok_or(RuntimeFailure::Superseded)?;
    let Some(status) = svc.chat_send_ledger.rpc_refusal_candidate(
        &pending.context.tab_id,
        &thread,
        &pending.context.binding_nonce,
        &pending.send_id,
    ) else {
        return Ok(());
    };
    let owner = svc.clone();
    let receipt = status.clone();
    let receipt_thread = thread.clone();
    let nonce = pending.context.binding_nonce.clone();
    let persisted = tokio::task::spawn_blocking(move || {
        owner
            .pending_delivery_payloads
            .user_sends
            .persist_rpc_refusal(&receipt, &receipt_thread, &nonce)
    })
    .await
    .map_err(|error| RuntimeFailure::HookFailed(error.to_string()))?
    .map_err(RuntimeFailure::HookFailed)?;
    if !persisted {
        return Ok(());
    }
    validate(svc, session, &pending.context)?;
    let Some(status) = svc.chat_send_ledger.refuse_rpc_send(
        &pending.context.tab_id,
        &thread,
        &pending.context.binding_nonce,
        &pending.send_id,
    ) else {
        return Ok(());
    };
    pending.refused.store(true, Ordering::Release);
    pending.settled.store(true, Ordering::Release);
    svc.ui.emit(
        "chat-send-status",
        serde_json::to_value(status).unwrap_or_default(),
    );
    subscriber::publish_events(
        svc,
        session,
        pending.context.epoch,
        vec![AgentRuntimeEvent::TurnState {
            thread_id: thread,
            turn_id: Some(pending.turn.clone()),
            parent_thread_id: None,
            state: RuntimeTurnState::Idle,
            detail: Some(RuntimeTurnDetail::Failed),
            status_line: None,
            started_at: None,
            completed_at: Some(now()),
        }],
        now(),
    )
    .await
}

fn eligible(
    reply: &RpcReply,
    code: RuntimeFailureCode,
    before_acceptance: bool,
    overlapping: bool,
    observed: bool,
    has_outcome: bool,
) -> bool {
    before_acceptance
        && code == RuntimeFailureCode::ProviderRefused
        && matches!(reply, RpcReply::Error { error } if error.code == -32602)
        && overlapping
        && !observed
        && !has_outcome
}

#[cfg(test)]
mod tests;
