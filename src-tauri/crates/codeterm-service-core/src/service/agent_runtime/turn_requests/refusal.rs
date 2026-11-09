use super::*;

pub(super) async fn apply(
    svc: &ServiceContext,
    session: &Arc<Session>,
    pending: &PendingTurn,
    reply: &RpcReply,
    reason: &codeterm_plugin::agent_runtime::RuntimeFailure,
    before_acceptance: bool,
) -> Result<(), RuntimeFailure> {
    // A busy agent may still record the written prompt, so the refusal settles it as uncertain with
    // a visible reason: no Retry, and late own-session evidence still delivers it.
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
    let Some(status) = svc
        .chat_send_ledger
        .mark_rpc_uncertain(&pending.send_id)
        .or_else(|| svc.chat_send_ledger.status(&pending.send_id))
        .filter(|status| {
            status.tab_id == pending.context.tab_id
                && status.state == crate::service::chat_send_transaction::ChatSendState::Unknown
        })
    else {
        return Ok(());
    };
    session.maps.lock().remember_refused_claim(
        pending.context.epoch,
        &pending.send_id,
        &pending.turn,
    );
    pending.refused.store(true, Ordering::Release);
    pending.settled.store(true, Ordering::Release);
    let status = svc
        .chat_send_ledger
        .note_rpc_refused_while_busy(&pending.send_id)
        .unwrap_or(status);
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
