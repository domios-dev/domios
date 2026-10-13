use super::{
    engine::{self, Session},
    model::RuntimeFailure,
};
use crate::service::{
    chat_send_transaction::SendLiveBinding, delivery_error::DeliveryError, ServiceContext,
};
use codeterm_plugin::agent_runtime::*;
use serde_json::Value;
use std::sync::Arc;

fn before_write(tab: &str, error: RuntimeFailure) -> DeliveryError {
    DeliveryError::TabBusyRefused {
        tab: tab.into(),
        detail: error.to_string(),
    }
}

pub fn send(
    svc: &ServiceContext,
    tab: &str,
    text: &str,
    send_id: Option<&str>,
) -> Result<(), DeliveryError> {
    svc.require_managed_send_allowed(tab, None)
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
    let session = tauri::async_runtime::block_on(engine::ensure(svc, tab))
        .map_err(|error| before_write(tab, error))?;
    let _permit = tauri::async_runtime::block_on(tokio::time::timeout(
        std::time::Duration::from_secs(30),
        session.gate.acquire(),
    ))
    .map_err(|_| before_write(tab, RuntimeFailure::Deadline))?
    .map_err(|_| before_write(tab, RuntimeFailure::Superseded))?;
    let context = session.context.lock().clone();
    if context.binding_nonce != expected {
        return Err(before_write(tab, RuntimeFailure::Superseded));
    }
    let thread = context
        .thread_id
        .clone()
        .ok_or_else(|| before_write(tab, RuntimeFailure::Superseded))?;
    let process = session
        .process
        .lock()
        .clone()
        .ok_or_else(|| before_write(tab, RuntimeFailure::PipeEof))?;
    let provider = svc
        .mux
        .read()
        .get_tab(tab)
        .and_then(|tab| tab.agent_type.clone())
        .ok_or_else(|| before_write(tab, RuntimeFailure::Superseded))?;
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
    if pending.tab_id != tab {
        return Err(before_write(tab, RuntimeFailure::Superseded));
    }
    engine::validate_binding(svc, &session).map_err(|error| before_write(tab, error))?;
    if context.claimed_sends.len() >= MAX_RUNTIME_BATCH || text.len() > MAX_RUNTIME_VALUE_BYTES {
        return Err(before_write(
            tab,
            RuntimeFailure::Refused("Runtime input or outstanding claim limit".into()),
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
    let input = TurnInput {
        text: text.into(),
        attachments: Vec::new(),
    };
    session.context.lock().claimed_sends.push(ClaimedSend {
        send_id: pending.send_id.clone(),
        thread_id: thread.clone(),
        input: input.clone(),
    });
    let operation = RuntimeOperation::SubmitTurn {
        thread_id: thread.clone(),
        active_turn_id: context.active_turn_id,
        send_id: pending.send_id.clone(),
        input,
        settings: context.settings,
    };
    let result =
        tauri::async_runtime::block_on(engine::run(svc, &provider, &session, &process, operation));
    match result {
        Ok(RuntimeResult::TurnAccepted {
            thread_id, send_id, ..
        }) if thread_id == thread && send_id == pending.send_id => {
            if let Some(status) = svc.chat_send_ledger.mark_rpc_accepted(&pending.send_id) {
                crate::api::events::emit_everywhere(&svc.app_handle, "chat-send-status", status);
            }
            Ok(())
        }
        other => {
            if let Some(status) = svc.chat_send_ledger.mark_rpc_uncertain(&pending.send_id) {
                crate::api::events::emit_everywhere(&svc.app_handle, "chat-send-status", status);
            }
            Err(DeliveryError::SubmitUnconfirmed {
                tab: tab.into(),
                detail: format!("Runtime handoff uncertain: {other:?}"),
            })
        }
    }
}

pub fn control(
    svc: &ServiceContext,
    tab: &str,
    operation: RuntimeOperation,
) -> Result<RuntimeResult, String> {
    let session = tauri::async_runtime::block_on(engine::ensure(svc, tab))
        .map_err(|error| error.to_string())?;
    let _permit = tauri::async_runtime::block_on(tokio::time::timeout(
        std::time::Duration::from_secs(30),
        session.gate.acquire(),
    ))
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
    tauri::async_runtime::block_on(engine::run(svc, &provider, &session, &process, operation))
        .map_err(|error| error.to_string())
}

pub fn launch(
    svc: &ServiceContext,
    provider: &str,
    tab: &str,
    config: Value,
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
    let result = tauri::async_runtime::block_on(engine::ensure(svc, tab))
        .map(|_| ())
        .map_err(|error| format!("Runtime tab {tab} degraded: {error}"));
    svc.emit_state_changed();
    result
}

pub fn snapshot(
    svc: &ServiceContext,
    tab: &str,
) -> Result<(Arc<Session>, Vec<crate::agent::chat_types::ChatMsg>), String> {
    let session = tauri::async_runtime::block_on(engine::ensure(svc, tab))
        .map_err(|error| error.to_string())?;
    let items = session.cache.lock().snapshot();
    Ok((session, items))
}

#[cfg(test)]
mod tests;

pub fn interrupt(svc: &ServiceContext, tab: &str) -> Result<(), String> {
    let session = tauri::async_runtime::block_on(engine::ensure(svc, tab))
        .map_err(|reason| reason.to_string())?;
    let context = session.context.lock().clone();
    let thread_id = context.thread_id.ok_or("Runtime thread unavailable")?;
    control(
        svc,
        tab,
        RuntimeOperation::Interrupt {
            thread_id,
            active_turn_id: context.active_turn_id,
        },
    )
    .map(|_| ())
}
