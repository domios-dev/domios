use super::{engine, events::RuntimeEvent, model::RuntimeFailure};
use crate::delayed_send::{DelayedMessageState, DelayedSendQueue, ReminderCreator};
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::{
    RuntimeErrorSignal, RuntimeRetryableErrorKind, RuntimeTurnDetail, RuntimeTurnState,
};

pub(crate) mod transcript;

const MAX_ATTEMPTS: u8 = 10;
const DELAY_MS: u64 = 10_000;

#[derive(Default)]
pub(super) struct RetryState {
    attempts: u8,
    pending: Option<String>,
    cancelled: std::collections::HashSet<String>,
    error: Option<(String, RuntimeErrorSignal)>,
}

#[derive(Debug, PartialEq, Eq)]
enum Decision {
    Ignore,
    Schedule { attempt: u8, at: u64 },
    Stopped,
    Reset,
}

impl RetryState {
    fn finish(&mut self, turn: &str, detail: Option<&RuntimeTurnDetail>, now: u64) -> Decision {
        if detail.is_none() {
            self.attempts = 0;
            self.error = None;
            return Decision::Reset;
        }
        if detail != Some(&RuntimeTurnDetail::Failed) {
            self.error = None;
            return Decision::Ignore;
        }
        let Some((error_turn, signal)) = self.error.take() else {
            return Decision::Ignore;
        };
        if error_turn != turn || signal.retryable || self.attempts >= MAX_ATTEMPTS {
            return Decision::Ignore;
        }
        let at = match (signal.retryable_kind, signal.retry_at_ms) {
            (Some(RuntimeRetryableErrorKind::Transient), _) => now.saturating_add(DELAY_MS),
            (Some(RuntimeRetryableErrorKind::RateLimitedUntil), Some(at))
                if at > now && at <= 9_007_199_254_740_991 =>
            {
                at
            }
            _ => return Decision::Ignore,
        };
        self.attempts += 1;
        if self.attempts == MAX_ATTEMPTS {
            Decision::Stopped
        } else {
            Decision::Schedule {
                attempt: self.attempts,
                at,
            }
        }
    }
}

pub(crate) fn cancel(svc: &ServiceContext, tab: &str) -> Result<(), String> {
    transcript::cancel(svc, tab)?;
    crate::delayed_send::emit_changed(svc, tab);
    if let Some(session) = engine::for_tab(tab) {
        reset(svc, &session)?;
    }
    Ok(())
}

pub(crate) fn skip_retry(svc: &ServiceContext, tab: &str, message: &str) -> bool {
    if let Some(skip) = transcript::skip_retry(svc, tab, message) {
        return skip;
    }
    let Some(session) = engine::for_tab(tab) else {
        return false;
    };
    let busy = {
        let context = session.context.lock();
        context.active_turn_id.is_some() || !context.active_turns.is_empty()
    };
    let mut retry = session.capacity_retry.lock();
    if retry.cancelled.remove(message) {
        return true;
    }
    if retry.pending.as_deref() != Some(message) {
        return false;
    }
    retry.pending = None;
    busy
}

fn reset(svc: &ServiceContext, session: &engine::Session) -> Result<(), String> {
    let tab = session.context.lock().tab_id.clone();
    let mut retry = session.capacity_retry.lock();
    remove_pending(svc, &tab, &mut retry)?;
    retry.attempts = 0;
    retry.error = None;
    drop(retry);
    crate::delayed_send::emit_changed(svc, &tab);
    Ok(())
}

fn remove_pending(svc: &ServiceContext, tab: &str, retry: &mut RetryState) -> Result<(), String> {
    if let Some(id) = retry.pending.as_deref() {
        let queue = svc.ext::<DelayedSendQueue>();
        if let Some(message) = queue.list(Some(tab)).iter().find(|msg| msg.id == id) {
            retry.cancelled.insert(id.to_owned());
            if message.state != DelayedMessageState::Sending {
                queue.delete(id)?;
                retry.cancelled.remove(id);
            }
        }
    }
    retry.pending = None;
    Ok(())
}

pub(super) async fn observe(
    svc: &ServiceContext,
    session: &std::sync::Arc<engine::Session>,
    event: &RuntimeEvent,
    at: u64,
) -> Result<(), RuntimeFailure> {
    let context = session.context.lock().clone();
    match event {
        RuntimeEvent::ItemUpdated { thread_id, item }
            if context.thread_id.as_ref() == Some(thread_id)
                && matches!(
                    item.system_subtype.as_deref(),
                    Some("api_error" | "provider_retry")
                ) =>
        {
            if let Some(turn) = item.tool_flow_id.as_deref() {
                if !session.completions.lock().contains(thread_id, turn) {
                    let signal = (item.system_subtype.as_deref() == Some("api_error"))
                        .then(|| {
                            item.tool_input.as_ref().and_then(|value| {
                                serde_json::from_value::<RuntimeErrorSignal>(value.clone()).ok()
                            })
                        })
                        .flatten();
                    session.capacity_retry.lock().error =
                        signal.map(|signal| (turn.into(), signal));
                }
            }
        }
        RuntimeEvent::TurnUpdated {
            thread_id,
            turn_id: Some(turn),
            state: RuntimeTurnState::Idle,
            detail,
            ..
        } if context.thread_id.as_ref() == Some(thread_id)
            && super::model::accepts_turn_transition(
                context.active_turn_id.as_deref(),
                Some(turn),
                &RuntimeTurnState::Idle,
            )
            && !session.completions.lock().contains(thread_id, turn) =>
        {
            let service = svc.clone();
            let owned_session = session.clone();
            let turn = turn.clone();
            let detail = *detail;
            let tab = context.tab_id.clone();
            let line = tokio::task::spawn_blocking(move || {
                let mut retry = owned_session.capacity_retry.lock();
                let decision = retry.finish(&turn, detail.as_ref(), at);
                let line = apply_decision(
                    &service,
                    &context.tab_id,
                    context.thread_id.clone(),
                    &mut retry,
                    decision,
                )?;
                Ok::<_, String>(line)
            })
            .await
            .map_err(|error| RuntimeFailure::HookFailed(error.to_string()))?
            .map_err(RuntimeFailure::HookFailed)?;
            crate::delayed_send::emit_changed(svc, &tab);
            if let Some(line) = line {
                publish_line(svc, session, line, at);
            }
        }
        _ => {}
    }
    Ok(())
}

fn apply_decision(
    svc: &ServiceContext,
    tab: &str,
    session: Option<String>,
    retry: &mut RetryState,
    decision: Decision,
) -> Result<Option<String>, String> {
    match decision {
        Decision::Ignore => Ok(None),
        Decision::Reset => {
            remove_pending(svc, tab, retry)?;
            Ok(None)
        }
        Decision::Stopped => {
            remove_pending(svc, tab, retry)?;
            Ok(Some("Automatic retry stopped after 10 attempts. Choose another model or send a new turn.".into()))
        }
        Decision::Schedule { attempt, at } => {
            remove_pending(svc, tab, retry)?;
            let message = svc.ext::<DelayedSendQueue>().enqueue(
                tab.into(),
                None,
                session,
                "continue".into(),
                Some(at),
                ReminderCreator::default(),
            )?;
            retry.pending = Some(message.id);
            Ok(Some(format!(
                "Automatic retry: attempt {attempt}/{MAX_ATTEMPTS}"
            )))
        }
    }
}

fn publish_line(svc: &ServiceContext, session: &engine::Session, content: String, at: u64) {
    let context = session.context.lock().clone();
    let item = codeterm_types::agent::chat_types::ChatMsg {
        id: format!("capacity-retry:{}", uuid::Uuid::new_v4().simple()),
        kind: "system".into(),
        content,
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

#[cfg(test)]
mod tests;
