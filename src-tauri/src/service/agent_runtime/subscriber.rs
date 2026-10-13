use super::{
    engine::{self, Session},
    model::{RuntimeFailure, RuntimePhase},
    process::Process,
};
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::*;
use std::sync::Arc;

pub fn start(svc: ServiceContext, _provider: String, session: Arc<Session>, process: Arc<Process>) {
    let mut events = process.client.subscribe();
    let mut ended = process.client.ended();
    let mut exited = process.ended.clone();
    let epoch = session.context.lock().epoch;
    tauri::async_runtime::spawn(async move {
        loop {
            let received = match events.try_recv() {
                Ok(frame) => Ok(frame),
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                    if ended.borrow().is_some() || exited.borrow().is_some() {
                        break;
                    }
                    tokio::select! {
                        biased;
                        event = events.recv() => event,
                        _ = ended.changed() => continue,
                        _ = exited.changed() => continue,
                    }
                }
                Err(error) => {
                    process.client.close(RuntimeFailure::PipeFailure {
                        stream: "events".into(),
                        reason: error.to_string(),
                    });
                    break;
                }
            };
            let frame = match received {
                Ok(frame) => frame,
                Err(error) => {
                    process.client.close(RuntimeFailure::PipeFailure {
                        stream: "events".into(),
                        reason: error.to_string(),
                    });
                    break;
                }
            };
            if let RuntimeFrame::Notification { method, .. } = &frame {
                let accepted = session
                    .adapter
                    .lock()
                    .as_ref()
                    .and_then(|adapter| adapter.agent_runtime())
                    .is_some_and(|capability| capability.notification_methods.contains(method));
                if !accepted {
                    continue;
                }
            }
            if session.context.lock().epoch != epoch
                || engine::validate_binding(&svc, &session).is_err()
            {
                break;
            }
            if let RuntimeFrame::Request {
                request_id,
                method,
                params,
            } = &frame
            {
                let mut pending = session.prompts.lock();
                if pending.contains_key(request_id) {
                    process.client.close(RuntimeFailure::InvalidFrame(
                        "Duplicate pending request ID".into(),
                    ));
                    break;
                }
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
            }
            let Some(adapter) = session.adapter.lock().clone() else {
                break;
            };
            let (context, state) = (
                session.context.lock().clone(),
                session.hook_state.lock().clone(),
            );
            let result = tokio::time::timeout(
                RUNTIME_HOOK_DEADLINE,
                adapter.runtime_reduce(&context, &state, &[frame]),
            )
            .await;
            let reduced = match result {
                Ok(Ok(reduced)) => reduced,
                other => {
                    process.client.close(RuntimeFailure::HookFailed(format!(
                        "Event mapping failed: {other:?}"
                    )));
                    break;
                }
            };
            let received_at = chrono::Utc::now().timestamp_millis().max(0) as u64;
            if session.context.lock().epoch != epoch {
                break;
            }
            if super::model::validate_state_size(&reduced.state).is_err()
                || reduced.events.len() > MAX_RUNTIME_BATCH
            {
                process.client.close(RuntimeFailure::Refused(
                    "Reducer state or event limit".into(),
                ));
                break;
            }
            *session.hook_state.lock() = reduced.state;
            if let Err(reason) =
                publish_events(&svc, &session, epoch, reduced.events, received_at).await
            {
                session.state.lock().degrade(epoch, &reason);
                process.client.close(reason);
                break;
            }
        }
        if session.context.lock().epoch == epoch {
            let reason = exited
                .borrow()
                .clone()
                .or_else(|| ended.borrow().clone())
                .unwrap_or(RuntimeFailure::PipeEof);
            let expired = session.context.lock().clone();
            super::prompt_answers::clear(&svc, &expired);
            session.prompts.lock().clear();
            if unload_retaining_degradation(&mut session.state.lock(), epoch, &reason) {
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
                    crate::api::events::emit_everywhere(
                        &svc.app_handle,
                        "chat-send-status",
                        status,
                    );
                }
                engine::publish(&svc, &context.tab_id, &session);
                svc.emit_state_changed();
            }
            process.stop();
        }
    });
}

pub(super) async fn publish_events(
    svc: &ServiceContext,
    session: &Session,
    epoch: u64,
    events: Vec<AgentRuntimeEvent>,
    at: u64,
) -> Result<(), RuntimeFailure> {
    let context = session.context.lock().clone();
    if context.epoch != epoch {
        return Err(RuntimeFailure::Superseded);
    }
    engine::validate_binding(svc, session)?;
    let evidence = super::model::validated_evidence(&context.claimed_sends, &events);
    let enriched = events
        .into_iter()
        .map(|event| HostAgentRuntimeEvent { at, event })
        .collect::<Vec<_>>();
    validate_host_runtime_events(&context, &enriched)
        .map_err(|reason| RuntimeFailure::Refused(reason.to_string()))?;
    for event in enriched {
        if matches!(&event.event, AgentRuntimeEvent::SendEvidence { send_id, .. } if !evidence.contains(send_id))
        {
            continue;
        }
        if let AgentRuntimeEvent::Question {
            thread_id,
            item_id,
            questions,
            response,
        } = &event.event
        {
            let record = super::questions::QuestionRecord {
                thread_id: thread_id.clone(),
                item_id: item_id.clone(),
                questions: questions.clone(),
                response: response.clone(),
                created_at: at,
                resolved: None,
            };
            let owner = svc.clone();
            let tab = context.tab_id.clone();
            let questions = tokio::task::spawn_blocking(move || {
                owner
                    .pending_delivery_payloads
                    .user_sends
                    .persist_runtime_question(&tab, record)
            })
            .await
            .map_err(|reason| RuntimeFailure::HookFailed(reason.to_string()))?
            .map_err(RuntimeFailure::HookFailed)?;
            super::question_delivery::publish(svc, &context.tab_id, session, questions)
                .map_err(RuntimeFailure::HookFailed)?;
        }
        if let AgentRuntimeEvent::RateLimits { rate_limits, .. } = &event.event {
            super::projections::rate_limits(svc, session, rate_limits.clone(), at).await?;
        }
        super::model::observe_turn(&mut session.context.lock(), &event.event);
        super::controls::observe_event(session, &event.event);
        let send_id = match &event.event {
            AgentRuntimeEvent::SendEvidence { send_id, .. } => Some(send_id.clone()),
            _ => None,
        };
        apply(svc, session, epoch, event);
        if let Some(status) = send_id.and_then(|id| svc.chat_send_ledger.status(&id)) {
            let owner = svc.clone();
            tokio::task::spawn_blocking(move || {
                owner
                    .pending_delivery_payloads
                    .user_sends
                    .persist_runtime_status(&status)?;
                super::envelope_delivery::record_write(&owner, &status)
            })
            .await
            .map_err(|reason| RuntimeFailure::HookFailed(reason.to_string()))?
            .map_err(RuntimeFailure::HookFailed)?;
        }
    }
    Ok(())
}

fn apply(svc: &ServiceContext, session: &Session, epoch: u64, enriched: HostAgentRuntimeEvent) {
    let at = enriched.at;
    let event = enriched.event;
    let context = session.context.lock().clone();
    if context.epoch != epoch || engine::validate_binding(svc, session).is_err() {
        return;
    }
    let owned = |thread: &str| context.thread_id.as_deref() == Some(thread);
    match event {
        AgentRuntimeEvent::Auth { state, detail } => {
            let previous = std::mem::replace(&mut *session.auth.lock(), state.clone());
            if matches!(state, RuntimeAuthState::SignedIn)
                && matches!(previous, RuntimeAuthState::SignedOut)
            {
                if session
                    .thread_loaded
                    .load(std::sync::atomic::Ordering::Acquire)
                {
                    if let Some(thread) = &context.thread_id {
                        session.state.lock().load(epoch, thread);
                    }
                } else {
                    session.state.lock().phase = RuntimePhase::Loading;
                }
                engine::publish(svc, &context.tab_id, session);
            }
            if matches!(state, RuntimeAuthState::SignedOut) {
                session.state.lock().degrade(
                    epoch,
                    &RuntimeFailure::Refused(detail.map_or_else(
                        || "Sign-in required".into(),
                        |detail| format!("Sign-in required: {detail}"),
                    )),
                );
                engine::publish(svc, &context.tab_id, session);
            }
        }
        AgentRuntimeEvent::ChatItem { thread_id, item } if owned(&thread_id) => {
            let item = session.cache.lock().reconcile(item);
            crate::service::tab_chat::structured_sink::emit_chat_msgs(
                &svc.app_handle,
                &context.tab_id,
                &[item],
                None,
                Some(&thread_id),
                Some(&context.binding_nonce),
            );
        }
        AgentRuntimeEvent::SendEvidence {
            thread_id,
            send_id,
            turn_id,
        } if owned(&thread_id) => {
            if !context
                .claimed_sends
                .iter()
                .any(|claim| claim.send_id == send_id && claim.thread_id == thread_id)
            {
                return;
            }
            if let Some(status) = svc.chat_send_ledger.confirm_rpc_send(
                &context.tab_id,
                &thread_id,
                &context.binding_nonce,
                &send_id,
            ) {
                session
                    .context
                    .lock()
                    .claimed_sends
                    .retain(|claim| claim.send_id != send_id);
                crate::api::events::emit_everywhere(&svc.app_handle, "chat-send-status", status);
                let consumed = {
                    let mut mux = svc.mux.write();
                    let pending = mux
                        .get_tab_mut(&context.tab_id)
                        .and_then(|tab| tab.pending_delivery.as_mut());
                    pending
                        .filter(|pending| pending.send_id() == Some(send_id.as_str()))
                        .is_some_and(|pending| {
                            pending.state().is_accepted() || pending.confirm_strong().is_ok()
                        })
                };
                if consumed {
                    let _ = svc.cleanup_confirmed_pending_delivery(&context.tab_id);
                }
                log::debug!(
                    "[agent-runtime] tab={} turn={turn_id} send={send_id} evidence=delivered",
                    context.tab_id
                );
            }
        }
        AgentRuntimeEvent::TurnState {
            thread_id,
            turn_id,
            state,
            started_at,
            completed_at,
            ..
        } if owned(&thread_id) => {
            if !super::model::accepts_turn_transition(
                context.active_turn_id.as_deref(),
                turn_id.as_deref(),
                &state,
            ) {
                return;
            }
            match state {
                RuntimeTurnState::Working => {
                    session
                        .state
                        .lock()
                        .working(context.active_turn_id == turn_id, at, started_at)
                }
                RuntimeTurnState::Idle => {
                    engine::record_completed_turn(session, &thread_id, turn_id.as_deref());
                    session.state.lock().completed(at, completed_at);
                }
                RuntimeTurnState::NeedsInput => {}
            }
            let verdict = match state {
                RuntimeTurnState::Working => crate::agent::provider::ProviderStateVerdict::Working,
                RuntimeTurnState::Idle => crate::agent::provider::ProviderStateVerdict::Idle,
                RuntimeTurnState::NeedsInput => {
                    crate::agent::provider::ProviderStateVerdict::Waiting
                }
            };
            crate::agent::state_verdict::apply(&svc.mux, &context.tab_id, &thread_id, verdict);
            session.context.lock().active_turn_id = match state {
                RuntimeTurnState::Idle => None,
                _ => turn_id,
            };
            match state {
                RuntimeTurnState::Working => svc.mark_agent_turn_started(&context.tab_id),
                RuntimeTurnState::Idle => svc.mark_agent_turn_complete(&context.tab_id),
                RuntimeTurnState::NeedsInput => svc.mark_agent_waiting(&context.tab_id),
            };
            engine::publish(svc, &context.tab_id, session);
            if matches!(state, RuntimeTurnState::Idle) {
                svc.staged_model_turn_completed(
                    &context.tab_id,
                    &thread_id,
                    &context.binding_nonce,
                );
            }
        }
        AgentRuntimeEvent::Prompt {
            thread_id,
            request_id,
            prompt,
        } if owned(&thread_id) => {
            let accepted = {
                let mut prompts = session.prompts.lock();
                if let Some(pending) = prompts.get_mut(&request_id) {
                    pending.prompt = Some(prompt.clone());
                    true
                } else {
                    false
                }
            };
            if accepted {
                super::prompt_answers::publish(svc, session, request_id, prompt);
            }
        }
        AgentRuntimeEvent::PromptResolved {
            thread_id,
            request_id,
        } if owned(&thread_id) => {
            session.prompts.lock().remove(&request_id);
            super::prompt_answers::resolve(svc, session, &request_id);
        }
        AgentRuntimeEvent::Usage { thread_id, usage } if owned(&thread_id) => {
            super::projections::usage(svc, session, usage, at);
        }
        AgentRuntimeEvent::ControlsConfirmed {
            thread_id,
            settings,
        } if owned(&thread_id) => {
            super::projections::controls(svc, session, settings);
        }
        AgentRuntimeEvent::ContextCompacted { thread_id, .. } if owned(&thread_id) => {}
        _ => {}
    }
    if !matches!(session.state.lock().phase, RuntimePhase::Loading) {
        svc.emit_state_changed();
    }
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
