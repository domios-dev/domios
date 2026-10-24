use super::{engine, events::RuntimeEvent, model::RuntimeFailure};
use crate::delayed_send::{DelayedMessage, DelayedSendQueue, TurnRetry};
use crate::service::{
    messenger::{AgentInput, HumanInput},
    ServiceContext,
};
use codeterm_plugin::agent_runtime::{
    RuntimeErrorSignal, RuntimeRetryableErrorKind, RuntimeTurnDetail, RuntimeTurnState,
};

mod policy;
pub(super) use policy::Policy;

pub(crate) fn cancel(svc: &ServiceContext, tab: &str) -> Result<(), String> {
    if let Some(session) = engine::for_tab(tab) {
        let pending = session.capacity_retry.lock().cancel();
        if let Some(id) = pending {
            let queue = svc.ext::<DelayedSendQueue>();
            if queue.list(Some(tab)).iter().any(|msg| {
                msg.id == id && msg.state != crate::delayed_send::DelayedMessageState::Sending
            }) {
                queue.delete(&id)?;
                crate::delayed_send::emit_changed(svc, tab);
            }
        }
    }
    Ok(())
}

pub(super) async fn observe(
    svc: &ServiceContext,
    session: &std::sync::Arc<engine::Session>,
    event: &RuntimeEvent,
    at: u64,
) -> Result<(), RuntimeFailure> {
    let context = session.context.lock().clone();
    let (decision, source_send) = {
        let mut policy = session.capacity_retry.lock();
        let decision = match event {
            RuntimeEvent::EvidenceUpdated {
                thread_id,
                send_id,
                turn_id,
                ..
            } if context.thread_id.as_ref() == Some(thread_id) => {
                policy.bind(send_id, turn_id);
                policy::Decision::Ignore
            }
            RuntimeEvent::ItemUpdated { thread_id, item }
                if context.thread_id.as_ref() == Some(thread_id)
                    && item
                        .system_subtype
                        .as_deref()
                        .is_some_and(|kind| kind == "api_error" || kind == "provider_retry") =>
            {
                if let Some(turn) = item.tool_flow_id.as_deref() {
                    policy.clear_error(turn);
                }
                if let (Some(turn), Some(signal)) = (
                    item.tool_flow_id.as_deref(),
                    item.tool_input.as_ref().and_then(|value| {
                        serde_json::from_value::<RuntimeErrorSignal>(value.clone()).ok()
                    }),
                ) {
                    if !signal.retryable && item.system_subtype.as_deref() == Some("api_error") {
                        let failure = match (signal.retryable_kind, signal.retry_at_ms) {
                            (Some(RuntimeRetryableErrorKind::Transient), _) => {
                                Some(policy::Failure::Transient)
                            }
                            (Some(RuntimeRetryableErrorKind::RateLimitedUntil), Some(at)) => {
                                Some(policy::Failure::RateLimitedUntil(at))
                            }
                            _ => None,
                        };
                        if let Some(failure) = failure {
                            policy.error(turn, failure);
                        }
                    }
                }
                policy::Decision::Ignore
            }
            RuntimeEvent::TurnUpdated {
                thread_id,
                turn_id: Some(turn),
                state: RuntimeTurnState::Idle,
                detail,
                ..
            } if context.thread_id.as_ref() == Some(thread_id) => policy.finish(
                turn,
                detail.as_ref() == Some(&RuntimeTurnDetail::Failed),
                at,
            ),
            _ => policy::Decision::Ignore,
        };
        (decision, policy.send_id.clone())
    };
    match decision {
        policy::Decision::Ignore => {}
        policy::Decision::Exhausted(failure) => {
            let item = codeterm_types::agent::chat_types::ChatMsg {
                id: format!("capacity-retry:{source_send}:exhausted"),
                kind: "system".into(),
                content: match failure {
                    policy::Failure::Transient => "Automatic retry stopped after 10 attempts. Choose another model or send a new turn.",
                    policy::Failure::RateLimitedUntil(_) => "The limit is still unavailable after the reset-time retry. Automatic retry stopped; send a new turn when the limit is available.",
                }.into(),
                system_subtype: Some("api_error".into()),
                ..Default::default()
            };
            let item = session.cache.lock().reconcile_live(item, at);
            crate::service::tab_chat::structured_sink::emit_chat_msgs(
                svc.ui.as_ref(),
                &context.tab_id,
                &[item],
                None,
                context.thread_id.as_deref(),
                Some(&context.binding_nonce),
            );
        }
        policy::Decision::Schedule {
            attempt,
            at,
            failure,
        } => {
            let service = svc.clone();
            let session = session.clone();
            tokio::task::spawn_blocking(move || {
                let mut policy = session.capacity_retry.lock();
                if policy.text.is_empty() || policy.send_id != source_send {
                    return Ok(());
                }
                let metadata = TurnRetry {
                    kind: match failure {
                        policy::Failure::Transient => RuntimeRetryableErrorKind::Transient,
                        policy::Failure::RateLimitedUntil(_) => {
                            RuntimeRetryableErrorKind::RateLimitedUntil
                        }
                    },
                    attempt,
                    epoch: context.epoch,
                    binding_nonce: context.binding_nonce,
                    thread_id: context.thread_id.unwrap_or_default(),
                };
                let msg = service.ext::<DelayedSendQueue>().enqueue_turn_retry(
                    &context.tab_id,
                    &policy.text,
                    at,
                    metadata,
                )?;
                policy.pending = Some(msg.id);
                drop(policy);
                crate::delayed_send::emit_changed(&service, &context.tab_id);
                Ok::<_, String>(())
            })
            .await
            .map_err(|error| RuntimeFailure::HookFailed(error.to_string()))?
            .map_err(RuntimeFailure::HookFailed)?;
        }
    }
    Ok(())
}

pub(crate) async fn deliver(svc: &ServiceContext, msg: &DelayedMessage) -> Result<(), String> {
    let metadata = msg
        .turn_retry
        .as_ref()
        .ok_or("Automatic retry metadata absent")?;
    let session = engine::for_tab(&msg.tab_id).ok_or("Automatic retry session ended")?;
    let _permit = session
        .gate
        .acquire()
        .await
        .map_err(|_| "Automatic retry session ended")?;
    engine::validate_binding(svc, &session).map_err(|error| error.to_string())?;
    let context = session.context.lock().clone();
    if context.epoch != metadata.epoch
        || context.binding_nonce != metadata.binding_nonce
        || context.thread_id.as_deref() != Some(metadata.thread_id.as_str())
        || context.active_turn_id.is_some()
    {
        return Err("Automatic retry cancelled: runtime turn or binding changed".into());
    }
    let process = session
        .process
        .lock()
        .clone()
        .ok_or("Automatic retry runtime ended")?;
    let (pending, text) = {
        let mut policy = session.capacity_retry.lock();
        if policy.pending.as_deref() != Some(msg.id.as_str()) || policy.text != msg.text {
            return Err("Automatic retry cancelled or queued turn edited".into());
        }
        let pending = svc.chat_send_ledger.try_start_scoped_with_normalization(
            &msg.tab_id,
            &msg.text,
            None,
            None,
            Default::default(),
        )?;
        let text = policy
            .claim(&msg.id, &pending.send_id)
            .ok_or("Automatic retry cancelled")?;
        (pending, text)
    };
    let result = svc
        .gateway
        .submit_runtime_loaded_turn(
            svc,
            &session,
            &process,
            &pending.send_id,
            AgentInput::Human(HumanInput::composer(&text)),
        )
        .await
        .map_err(|error| error.ledger_message());
    if let Some(status) = svc
        .chat_send_ledger
        .finish(&pending.send_id, result.clone())
    {
        svc.ui.emit(
            "chat-send-status",
            serde_json::to_value(status).unwrap_or_default(),
        );
    }
    result
}
