use super::super::engine::{self, Session};
use super::super::events::{self, RuntimeEvent};
use super::super::model::{RuntimeFailure, RuntimePhase};
use crate::service::MuxEmitCause;
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::*;
use std::sync::Arc;

pub(in crate::service::agent_runtime) async fn publish_events(
    svc: &ServiceContext,
    session: &Arc<Session>,
    epoch: u64,
    events: Vec<AgentRuntimeEvent>,
    at: u64,
) -> Result<(), RuntimeFailure> {
    let context = session.context.lock().clone();
    if context.epoch != epoch {
        return Err(RuntimeFailure::Superseded);
    }
    engine::validate_binding(svc, session)?;
    let evidence = super::super::model::validated_evidence(&context.claimed_sends, &events);
    let enriched = events
        .into_iter()
        .map(|event| HostAgentRuntimeEvent { at, event })
        .collect::<Vec<_>>();
    validate_host_runtime_events(&context, &enriched)
        .map_err(|reason| RuntimeFailure::Refused(reason.to_string()))?;
    if !enriched.is_empty() {
        svc.ext::<crate::usage::TabActivity>().mark(&context.tab_id);
    }
    let mut run_completed = false;
    let mut run_chat_item = false;
    for event in enriched {
        if session.context.lock().epoch != epoch {
            return Err(RuntimeFailure::Superseded);
        }
        engine::validate_binding(svc, session)?;
        if matches!(&event.event, AgentRuntimeEvent::SendEvidence { send_id, item_id, .. } if !evidence.contains(&(send_id.clone(), item_id.clone())))
        {
            continue;
        }
        let mut runtime = events::translate(&event.event);
        if !super::turn_lifecycle::prepare(svc, session, at, &mut runtime) {
            continue;
        }
        events::fold(&mut session.maps.lock(), epoch, &runtime);
        if let RuntimeEvent::RequestUpdated {
            thread_id,
            item_id: Some(item_id),
            questions: Some(questions),
            response: Some(response),
            ..
        } = &runtime
        {
            let record = super::super::questions::QuestionRecord {
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
            super::super::question_delivery::publish(svc, &context.tab_id, session, questions)
                .map_err(RuntimeFailure::HookFailed)?;
        }
        if let RuntimeEvent::RateLimitsUpdated {
            rate_limits,
            resets,
            account_credits,
            ..
        } = &runtime
        {
            super::super::projections::rate_limits(
                svc,
                session,
                rate_limits.clone(),
                resets.clone(),
                account_credits.clone(),
                at,
            )
            .await?;
        }
        events::observe(&mut session.context.lock(), &runtime);
        let retry = super::super::capacity_retry::observe(svc, session, &runtime, at).await;
        super::super::controls::observe_event(session, &runtime);
        let send_id = match &runtime {
            RuntimeEvent::EvidenceUpdated { send_id, .. } => Some(send_id.clone()),
            _ => None,
        };
        let completed_turn = match &runtime {
            RuntimeEvent::TurnUpdated {
                thread_id,
                turn_id: Some(turn),
                state: RuntimeTurnState::Idle,
                ..
            } if Some(thread_id) == context.thread_id.as_ref()
                && super::super::model::accepts_turn_transition(
                    session.context.lock().active_turn_id.as_deref(),
                    Some(turn),
                    &RuntimeTurnState::Idle,
                ) =>
            {
                Some((thread_id.clone(), turn.clone()))
            }
            _ => None,
        };
        let chat_item = matches!(runtime, RuntimeEvent::ItemUpdated { .. });
        retry?;
        let handling = match &runtime {
            RuntimeEvent::EvidenceUpdated {
                thread_id,
                turn_id,
                send_id,
                ..
            } if Some(thread_id) == context.thread_id.as_ref() => svc
                .chat_send_ledger
                .status(send_id)
                .and_then(|status| status.correlation_id)
                .and_then(|token| neighbor_message_id(&token).map(str::to_owned))
                .map(|message| {
                    (
                        thread_id.clone(),
                        turn_id.clone(),
                        Some(message),
                        session.context.lock().active_turn_id.as_ref() == Some(turn_id),
                        session.completions.lock().contains(thread_id, turn_id),
                    )
                }),
            RuntimeEvent::TurnUpdated {
                thread_id,
                turn_id: Some(turn),
                state,
                ..
            } if Some(thread_id) == context.thread_id.as_ref() => Some((
                thread_id.clone(),
                turn.clone(),
                None,
                matches!(
                    state,
                    RuntimeTurnState::Working | RuntimeTurnState::NeedsInput
                ),
                matches!(state, RuntimeTurnState::Idle),
            )),
            _ => None,
        };
        if let Some((thread, turn, message, working, finished)) = handling {
            use crate::service::neighbors::history::handling::State;
            let failed = matches!(
                &runtime,
                RuntimeEvent::TurnUpdated {
                    detail: Some(RuntimeTurnDetail::Failed),
                    ..
                }
            );
            let state = if failed {
                State::Failed
            } else if finished {
                State::NoReply
            } else if working {
                State::Working
            } else {
                State::DeliveredToConcierge
            };
            if let Err(error) = svc
                .neighbor_runtime_handling_event(
                    &context.tab_id,
                    &context.binding_nonce,
                    &thread,
                    &turn,
                    message.as_deref(),
                    state,
                )
                .await
            {
                log::debug!("[neighbors] incoming runtime handling: {error}");
            }
        }
        apply(svc, session, epoch, at, runtime);
        run_completed |= completed_turn
            .is_some_and(|(thread, turn)| session.completions.lock().contains(&thread, &turn));
        run_chat_item |= chat_item;
        if let Some(status) = send_id.and_then(|id| svc.chat_send_ledger.status(&id)) {
            let owner = svc.clone();
            tokio::task::spawn_blocking(move || {
                owner
                    .pending_delivery_payloads
                    .user_sends
                    .persist_runtime_status(&status)?;
                super::super::envelope_delivery::record_write(&owner, &status)
            })
            .await
            .map_err(|reason| RuntimeFailure::HookFailed(reason.to_string()))?
            .map_err(RuntimeFailure::HookFailed)?;
        }
    }
    if (run_completed || run_chat_item) && svc.run_watcher.has_runtime_run(&context.tab_id) {
        let owner = svc.clone();
        let binding = session.context.lock().clone();
        let _ = tokio::task::spawn_blocking(move || {
            if run_completed {
                owner.run_watcher.runtime_turn_completed(&owner, &binding);
            } else {
                owner.run_watcher.check_runtime_completion(&owner, &binding);
            }
        })
        .await;
    }
    Ok(())
}

fn apply(svc: &ServiceContext, session: &Arc<Session>, epoch: u64, at: u64, event: RuntimeEvent) {
    let context = session.context.lock().clone();
    if context.epoch != epoch || engine::validate_binding(svc, session).is_err() {
        return;
    }
    let owned = |thread: &str| context.thread_id.as_deref() == Some(thread);
    match event {
        RuntimeEvent::SessionUpdated {
            login: Some(login), ..
        } => super::super::login::apply(svc, session, epoch, login),
        RuntimeEvent::SessionUpdated {
            auth: Some(state),
            billing_mode,
            detail,
            ..
        } => {
            super::super::projections::billing(svc, session, billing_mode.unwrap_or_default(), at);
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
                if let Some(detail) = detail {
                    log::debug!(
                        "[runtime-auth] tab={} sign-in pending: {detail}",
                        context.tab_id
                    );
                }
                session
                    .state
                    .lock()
                    .degrade(epoch, &RuntimeFailure::SignInRequired);
                engine::publish(svc, &context.tab_id, session);
            }
        }
        RuntimeEvent::ItemUpdated { thread_id, item } if owned(&thread_id) => {
            let item = session.cache.lock().reconcile_live(item, at);
            crate::flow_trace::SourceCursor::rpc(
                &context.tab_id,
                &session.provider,
                &thread_id,
                Some(&context.binding_nonce),
                &item,
            );
            crate::service::tab_chat::structured_sink::emit_chat_msgs(
                svc.ui.as_ref(),
                &context.tab_id,
                &[item],
                None,
                Some(&thread_id),
                Some(&context.binding_nonce),
            );
        }
        RuntimeEvent::EvidenceUpdated {
            item_id,
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
            svc.chat_send_ledger.trace_rpc_turn(
                &context.tab_id,
                &thread_id,
                &context.binding_nonce,
                &send_id,
                &turn_id,
                &session.provider,
            );
            if let Some(status) = svc.chat_send_ledger.confirm_rpc_send(
                &context.tab_id,
                &thread_id,
                &context.binding_nonce,
                &send_id,
                item_id.as_deref(),
            ) {
                session
                    .context
                    .lock()
                    .claimed_sends
                    .retain(|claim| claim.send_id != send_id);
                session.maps.lock().take_evidence(&send_id);
                svc.ui.emit(
                    "chat-send-status",
                    serde_json::to_value(status).unwrap_or_default(),
                );
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
        RuntimeEvent::TurnUpdated {
            thread_id,
            turn_id,
            state,
            detail,
            status_line,
            started_at,
            completed_at,
            ..
        } if owned(&thread_id) => {
            if !super::super::model::accepts_turn_transition(
                context.active_turn_id.as_deref(),
                turn_id.as_deref(),
                &state,
            ) {
                return;
            }
            match state {
                RuntimeTurnState::Working => {
                    if let Some(turn) = turn_id.as_deref() {
                        svc.runtime_turn_starts(
                            &context.tab_id,
                            &context.binding_nonce,
                            context.epoch,
                        )
                        .observe(turn);
                    }
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
            let live = crate::agent::state_verdict::live_state_for_runtime(
                &state,
                detail,
                status_line
                    .as_deref()
                    .filter(|_| !matches!(state, RuntimeTurnState::Idle)),
            );
            crate::agent::state_verdict::apply_live(&svc.mux, &context.tab_id, &thread_id, live);
            svc.record_turn_resume_edge(&context.tab_id);
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
        RuntimeEvent::RequestUpdated {
            thread_id,
            request_id: Some(request_id),
            prompt: Some(prompt),
            resolved: false,
            ..
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
                super::super::prompt_answers::publish(svc, session, request_id, prompt);
            }
        }
        RuntimeEvent::RequestUpdated {
            thread_id,
            request_id: Some(request_id),
            resolved: true,
            prompt: None,
            questions: None,
            ..
        } if owned(&thread_id) => {
            session.prompts.lock().remove(&request_id);
            super::super::prompt_answers::resolve(svc, session, &request_id);
        }
        RuntimeEvent::UsageUpdated { thread_id, usage } if owned(&thread_id) => {
            super::super::projections::usage(svc, session, usage, at);
        }
        RuntimeEvent::ThreadUpdated {
            thread_id,
            settings: Some(settings),
            ..
        } if owned(&thread_id) => {
            super::super::projections::controls(svc, session, settings);
        }
        RuntimeEvent::CompactionUpdated { thread_id, item_id } if owned(&thread_id) => {
            let _ = super::compaction::schedule(svc, session, epoch, &thread_id, &item_id);
        }
        RuntimeEvent::ThreadReverted { .. } => {}
        _ => {}
    }
    if !matches!(session.state.lock().phase, RuntimePhase::Loading) {
        svc.emit_state_changed(MuxEmitCause::RuntimeSignal);
    }
}

fn neighbor_message_id(correlation: &str) -> Option<&str> {
    correlation
        .strip_prefix("runtime-envelope:neighbor:")?
        .rsplit('_')
        .next()
        .filter(|id| !id.is_empty())
}

#[cfg(test)]
#[path = "apply/neighbor_tests.rs"]
mod neighbor_tests;
