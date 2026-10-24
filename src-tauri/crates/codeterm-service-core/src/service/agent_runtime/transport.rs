use super::{
    engine::{self, Session},
    model::RuntimeFailure,
};
use crate::service::MuxEmitCause;
use crate::service::{
    chat_send_transaction::SendLiveBinding, delivery_error::DeliveryError, ServiceContext,
};
use codeterm_plugin::agent_runtime::*;
use serde_json::Value;
use std::sync::Arc;

const REJECTED_BEFORE_WRITE: &str = "runtime_rejected_before_write";

/// The provider failed the turn before any frame reached the runtime, so holding the send would only repeat it.
pub(crate) fn rejected_before_write(error: &DeliveryError) -> bool {
    matches!(error, DeliveryError::TabBusyRefused { detail, .. } if detail.starts_with(REJECTED_BEFORE_WRITE))
}

fn before_write(tab: &str, error: RuntimeFailure) -> DeliveryError {
    DeliveryError::TabBusyRefused {
        tab: tab.into(),
        detail: error.to_string(),
    }
}

pub fn send(
    svc: &ServiceContext,
    tab: &str,
    input: &crate::service::messenger::AgentInput,
    send_id: Option<&str>,
) -> Result<(), DeliveryError> {
    send_inner(svc, tab, input, send_id, None)
}

pub(crate) fn send_pinned(
    svc: &ServiceContext,
    tab: &str,
    input: &crate::service::messenger::AgentInput,
    send_id: &str,
    session_id: &str,
    binding_nonce: &str,
) -> Result<(), DeliveryError> {
    send_inner(
        svc,
        tab,
        input,
        Some(send_id),
        Some((session_id, binding_nonce)),
    )
}

fn send_inner(
    svc: &ServiceContext,
    tab: &str,
    input: &crate::service::messenger::AgentInput,
    send_id: Option<&str>,
    pinned: Option<(&str, &str)>,
) -> Result<(), DeliveryError> {
    let rendered = input.text();
    let text = rendered.as_ref();
    svc.require_admitted_input_allowed(tab, send_id)
        .map_err(|detail| DeliveryError::TabBusyRefused {
            tab: tab.into(),
            detail,
        })?;
    svc.require_managed_delivery_allowed(tab, None)
        .map_err(|detail| DeliveryError::TabBusyRefused {
            tab: tab.into(),
            detail,
        })?;
    super::capacity_retry::cancel(svc, tab)
        .map_err(|reason| before_write(tab, RuntimeFailure::Refused(reason)))?;
    svc.wake_frozen_tab(tab)
        .map_err(|detail| DeliveryError::TabBusyRefused {
            tab: tab.into(),
            detail,
        })?;
    let expected = svc
        .mux
        .read()
        .get_tab(tab)
        .and_then(|tab| tab.session_binding_nonce.clone())
        .ok_or_else(|| before_write(tab, RuntimeFailure::Superseded))?;
    if send_id.is_some_and(|id| svc.chat_send_ledger.has_rpc_handoff(id)) {
        return Err(DeliveryError::SubmitUnconfirmed {
            tab: tab.into(),
            detail: "Runtime handoff already claimed; replay refused".into(),
        });
    }
    let session = if pinned.is_some() {
        engine::for_tab(tab).ok_or_else(|| before_write(tab, RuntimeFailure::Superseded))?
    } else {
        crate::async_runtime::block_on(engine::ensure(svc, tab))
            .map_err(|error| before_write(tab, error))?
    };
    let _permit =
        super::blocking::deadline(std::time::Duration::from_secs(30), session.gate.acquire())
            .map_err(|_| before_write(tab, RuntimeFailure::Deadline))?
            .map_err(|_| before_write(tab, RuntimeFailure::Superseded))?;
    let context = session.context.lock().clone();
    if context.binding_nonce != expected {
        return Err(before_write(tab, RuntimeFailure::Superseded));
    }
    if let Some((thread, nonce)) = pinned {
        if context.thread_id.as_deref() != Some(thread)
            || context.binding_nonce != nonce
            || !session.thread_loaded.load(std::sync::atomic::Ordering::Acquire)
            || !svc.mux.read().get_tab(tab).is_some_and(|tab| {
                tab.launch_session_authenticated()
                    && tab.agent_session_id.as_deref() == Some(thread)
                    && tab.session_binding_nonce.as_deref() == Some(nonce)
                    && matches!(&tab.backing, crate::multiplexer::types::TabBacking::ChatBackend { session_id, .. } if session_id == thread)
            })
        {
            return Err(before_write(tab, RuntimeFailure::Superseded));
        }
    }
    let process = session
        .process
        .lock()
        .clone()
        .ok_or_else(|| before_write(tab, RuntimeFailure::PipeEof))?;
    let pending = match send_id {
        Some(id) => svc.chat_send_ledger.status(id).ok_or_else(|| {
            before_write(
                tab,
                RuntimeFailure::Refused("Ledger admission absent".into()),
            )
        })?,
        None => svc
            .chat_send_ledger
            .try_start_scoped_with_normalization(tab, text, None, None, Default::default())
            .map_err(|reason| before_write(tab, RuntimeFailure::Refused(reason)))?,
    };
    crate::async_runtime::block_on(svc.gateway.submit_runtime_loaded_turn(
        svc,
        &session,
        &process,
        &pending.send_id,
        input.clone(),
    ))
}

fn turn_input(text: &str) -> TurnInput {
    use codeterm_types::agent::chat_types::ChatAttachment;
    use codeterm_wire::envelope::{parse_envelope, MessageSegment};

    fn collect(segments: Vec<MessageSegment>, attachments: &mut Vec<ChatAttachment>) {
        for segment in segments {
            match segment {
                MessageSegment::Attachment { kind, path, .. }
                    if kind == "image" && !path.is_empty() =>
                {
                    if attachments
                        .iter()
                        .any(|image| image.path.as_ref() == Some(&path))
                    {
                        continue;
                    }
                    let extension = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
                    let mime_type = match extension.as_str() {
                        "jpg" | "jpeg" => "image/jpeg",
                        "gif" => "image/gif",
                        "webp" => "image/webp",
                        "bmp" => "image/bmp",
                        "avif" => "image/avif",
                        "svg" => "image/svg+xml",
                        _ => "image/png",
                    };
                    attachments.push(ChatAttachment {
                        id: format!("attachment:{path}"),
                        mime_type: mime_type.into(),
                        path: Some(path),
                        ..Default::default()
                    });
                }
                MessageSegment::Frame { segments, .. } => collect(segments, attachments),
                _ => {}
            }
        }
    }

    let mut attachments = Vec::new();
    collect(parse_envelope(text), &mut attachments);
    TurnInput {
        text: text.into(),
        attachments,
    }
}

pub(crate) async fn submit_loaded(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<super::process::Process>,
    send_id: &str,
    classified: crate::service::messenger::AgentInput,
) -> Result<(), DeliveryError> {
    let rendered = classified.text();
    let input = match &classified {
        crate::service::messenger::AgentInput::Launch(_) => TurnInput {
            text: rendered.into_owned(),
            attachments: Vec::new(),
        },
        crate::service::messenger::AgentInput::Shell(_) => {
            return Err(before_write(
                &session.context.lock().tab_id,
                RuntimeFailure::Refused("shell input cannot target a runtime agent".into()),
            ))
        }
        _ => turn_input(&rendered),
    };
    let context = session.context.lock().clone();
    let tab = context.tab_id.as_str();
    if session.compaction.lock().held(context.epoch) {
        return Err(before_write(
            tab,
            RuntimeFailure::Refused("Context restoration pending or failed; sends held".into()),
        ));
    }
    let thread = context
        .thread_id
        .clone()
        .ok_or_else(|| before_write(tab, RuntimeFailure::Superseded))?;
    let pending = svc.chat_send_ledger.status(send_id).ok_or_else(|| {
        before_write(
            tab,
            RuntimeFailure::Refused("Ledger admission absent".into()),
        )
    })?;
    if pending.tab_id != tab {
        return Err(before_write(tab, RuntimeFailure::Superseded));
    }
    super::capture::validate_if_present(svc, tab, send_id)
        .map_err(|reason| before_write(tab, reason))?;
    engine::validate_binding(svc, &session).map_err(|error| before_write(tab, error))?;
    if context.claimed_sends.len() >= MAX_RUNTIME_BATCH
        || serde_json::to_vec(&input).map_or(true, |value| value.len() > MAX_RUNTIME_VALUE_BYTES)
    {
        return Err(before_write(
            tab,
            RuntimeFailure::Refused("Runtime input or outstanding claim limit".into()),
        ));
    }
    if !session.capacity_retry.lock().permits_send(send_id) {
        return Err(before_write(
            tab,
            RuntimeFailure::Refused("Capacity retry cancelled".into()),
        ));
    }
    if !svc.chat_send_ledger.bind_rpc_send(
        &pending.send_id,
        SendLiveBinding {
            session_id: Some(thread.clone()),
            binding_nonce: Some(context.binding_nonce.clone()),
        },
    ) {
        return Err(DeliveryError::SubmitUnconfirmed {
            tab: tab.into(),
            detail: "Runtime send already claimed".into(),
        });
    }
    if matches!(classified, crate::service::messenger::AgentInput::Human(_)) {
        session.capacity_retry.lock().begin(send_id, &input.text);
    } else {
        super::capacity_retry::cancel(svc, tab)
            .map_err(|reason| before_write(tab, RuntimeFailure::Refused(reason)))?;
    }
    session.context.lock().claimed_sends.push(ClaimedSend {
        send_id: pending.send_id.clone(),
        thread_id: thread.clone(),
        input: input.clone(),
    });
    let before_completion = session.completions.lock().sequence();
    let submitted_turn = context.active_turn_id.clone();
    let operation = RuntimeOperation::SubmitTurn {
        thread_id: thread.clone(),
        active_turn_id: context.active_turn_id.clone(),
        send_id: pending.send_id.clone(),
        input,
        settings: context.settings.clone(),
    };
    let starts = svc.runtime_turn_starts(tab, &context.binding_nonce, context.epoch);
    let before_start = starts.sequence();
    let (result, handed_off) = engine::run_submit(svc, session, process, operation).await;
    if let Err(
        reason @ (RuntimeFailure::PipeEof
        | RuntimeFailure::PipeFailure { .. }
        | RuntimeFailure::ProcessExited {
            requested: false, ..
        }),
    ) = &result
    {
        svc.recover_dead_chatbackend(
            tab,
            &context.binding_nonce,
            context.epoch,
            &reason.to_string(),
        );
    }
    let result = match result {
        Err(
            reason @ (RuntimeFailure::PipeEof
            | RuntimeFailure::PipeFailure { .. }
            | RuntimeFailure::ProcessExited {
                requested: false, ..
            }),
        ) if !handed_off => Err(RuntimeFailure::PrewriteRefused(format!(
            "{REJECTED_BEFORE_WRITE}: {reason}"
        ))),
        Err(RuntimeFailure::HookFailed(reason)) if !handed_off => Err(
            RuntimeFailure::PrewriteRefused(format!("{REJECTED_BEFORE_WRITE}: {reason}")),
        ),
        other => other,
    };
    if let Err(failure) = &result {
        log::warn!(
            "[runtime-send] tab={tab} send_id={} handed_off={handed_off} failed: {failure}",
            pending.send_id
        );
    }
    match result {
        Ok(RuntimeResult::TurnAccepted {
            thread_id,
            send_id,
            turn_id,
        }) if thread_id == thread && send_id == pending.send_id => {
            if turn_id.is_none() || turn_id.as_ref() != submitted_turn.as_ref() {
                svc.arm_runtime_turn_start(
                    session,
                    context.clone(),
                    starts,
                    before_start,
                    turn_id.clone(),
                );
            }
            if let Some(turn) = turn_id {
                session.capacity_retry.lock().bind(&send_id, &turn);
                let mut current = session.context.lock();
                if current.epoch == context.epoch
                    && session.completions.lock().permits_receipt(
                        &thread,
                        &turn,
                        current.active_turn_id.as_deref(),
                        submitted_turn.as_deref(),
                        before_completion,
                    )
                {
                    current.active_turn_id = Some(turn);
                }
            }
            if let Some(status) = svc.chat_send_ledger.mark_rpc_accepted(&pending.send_id) {
                svc.ui.emit(
                    "chat-send-status",
                    serde_json::to_value(status).unwrap_or_default(),
                );
            }
            Ok(())
        }
        Err(RuntimeFailure::PrewriteRefused(reason)) => {
            if svc.chat_send_ledger.clear_rpc_send_after_definite_refusal(
                &pending.send_id,
                &SendLiveBinding {
                    session_id: Some(thread),
                    binding_nonce: Some(context.binding_nonce),
                },
            ) {
                session
                    .context
                    .lock()
                    .claimed_sends
                    .retain(|claim| claim.send_id != pending.send_id);
                return Err(DeliveryError::TabBusyRefused {
                    tab: tab.into(),
                    detail: reason,
                });
            }
            Err(DeliveryError::SubmitUnconfirmed {
                tab: tab.into(),
                detail: reason,
            })
        }
        other => {
            if let Some(status) = svc.chat_send_ledger.mark_rpc_uncertain(&pending.send_id) {
                svc.ui.emit(
                    "chat-send-status",
                    serde_json::to_value(status).unwrap_or_default(),
                );
            }
            Err(DeliveryError::SubmitUnconfirmed {
                tab: tab.into(),
                detail: format!("Runtime handoff uncertain: {other:?}"),
            })
        }
    }
}

pub(crate) fn send_envelope(
    svc: &ServiceContext,
    tab: &str,
    input: &crate::service::messenger::FramedDelivery,
    token: &str,
    observer: Option<crate::service::messenger::DeliveryWriteObserver>,
) -> Result<crate::service::messenger::Delivery, String> {
    super::envelope_delivery::send(svc, tab, &input.to_message(), token, observer)
}

#[cfg(test)]
pub(crate) fn control(
    svc: &ServiceContext,
    tab: &str,
    operation: RuntimeOperation,
) -> Result<RuntimeResult, String> {
    let session = crate::async_runtime::block_on(engine::ensure(svc, tab))
        .map_err(|error| error.to_string())?;
    let _permit =
        super::blocking::deadline(std::time::Duration::from_secs(30), session.gate.acquire())
            .map_err(|_| "Runtime permit deadline")?
            .map_err(|_| "Runtime stopped")?;
    let process = session
        .process
        .lock()
        .clone()
        .ok_or("Runtime process unavailable")?;
    let provider = svc
        .mux
        .read()
        .get_tab(tab)
        .and_then(|tab| tab.agent_type.clone())
        .ok_or("Runtime provider absent")?;
    crate::async_runtime::block_on(engine::run(svc, &provider, &session, &process, operation))
        .map_err(|error| error.to_string())
}

pub fn launch(
    svc: &ServiceContext,
    provider: &str,
    tab: &str,
    config: Value,
) -> Result<(), String> {
    launch_with_fork(svc, provider, tab, config, None)
}

pub(crate) fn launch_fork(
    svc: &ServiceContext,
    provider: &str,
    tab: &str,
    config: Value,
    source_thread_id: &str,
    turn_id: &str,
) -> Result<(), String> {
    launch_with_fork(
        svc,
        provider,
        tab,
        config,
        Some((source_thread_id, turn_id)),
    )
}

fn launch_with_fork(
    svc: &ServiceContext,
    provider: &str,
    tab: &str,
    config: Value,
    fork: Option<(&str, &str)>,
) -> Result<(), String> {
    svc.ensure_agent_pad(tab)?;
    let nonce = svc.ensure_session_binding_nonce(tab);
    {
        let mut mux = svc.mux.write();
        let target = mux.get_tab_mut(tab).ok_or("Runtime tab absent")?;
        target.agent_type = Some(provider.into());
        target.agent_runtime_config = Some(config);
        target.session_binding_nonce = Some(nonce.clone());
        target.chat_view = true;
    }
    super::authority::record_spawn(svc, tab, provider, &nonce)?;
    let result = crate::async_runtime::block_on(async {
        match fork {
            Some((source_thread_id, turn_id)) => {
                engine::ensure_fork(svc, tab, source_thread_id, turn_id).await
            }
            None => engine::ensure(svc, tab).await,
        }
    })
    .or_else(|error| match fork {
        None => engine::login_pending_session(svc, tab, error),
        Some(_) => Err(error),
    })
    .map(|_| ())
    .map_err(|error| format!("Runtime tab {tab} degraded: {error}"));
    if fork.is_none() {
        svc.emit_state_changed(MuxEmitCause::TabLifecycle);
    }
    result
}

pub fn snapshot(
    svc: &ServiceContext,
    tab: &str,
) -> Result<(Arc<Session>, Vec<crate::agent::chat_types::ChatMsg>), String> {
    let started = std::time::Instant::now();
    let cached = engine::for_tab(tab).filter(|session| {
        session
            .thread_loaded
            .load(std::sync::atomic::Ordering::Acquire)
            && engine::validate_binding(svc, session).is_ok()
    });
    let ensured = match cached {
        Some(session) => Ok(session),
        None => crate::async_runtime::block_on(engine::ensure(svc, tab)),
    };
    log::debug!(
        "[chat-attach] tab={tab} phase=ensure elapsed_ms={} ok={}",
        started.elapsed().as_millis(),
        ensured.is_ok()
    );
    let session = ensured
        .or_else(|error| engine::login_pending_session(svc, tab, error))
        .map_err(|error| error.to_string())?;
    let reading = std::time::Instant::now();
    let items = session.cache.lock().snapshot();
    log::debug!(
        "[chat-attach] tab={tab} phase=cache_read elapsed_ms={} rows={}",
        reading.elapsed().as_millis(),
        items.len()
    );
    Ok((session, items))
}

#[cfg(test)]
mod tests;

pub fn interrupt(svc: &ServiceContext, tab: &str) -> Result<(), String> {
    super::controls::interrupt(svc, tab).map(|_| ())
}
