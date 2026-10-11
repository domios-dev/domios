mod gate;
mod startup;

pub(crate) use startup::{admit_startup, send_startup};

use super::{client::ClientError, lifecycle::Sidecar, protocol::Frame};
use crate::service::{
    chat_send_transaction::{ChatSendStatus, SendLiveBinding},
    delivery_error::DeliveryError,
    ServiceContext,
};
use gate::SendGate;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{atomic::Ordering, Arc, LazyLock, Weak},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    User,
    Agent,
}

static OBSERVERS: LazyLock<Mutex<HashMap<String, Weak<Sidecar>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn route_or_pty<E>(
    rpc: Option<Result<(), E>>,
    pty: impl FnOnce() -> Result<(), E>,
) -> Result<(), E> {
    match rpc {
        Some(result) => result,
        None => pty(),
    }
}

pub fn try_send_to_pty(
    svc: &ServiceContext,
    pty_id: &str,
    text: &str,
) -> Option<Result<(), DeliveryError>> {
    let tab = svc.mux.read().tab_id_for_pty(pty_id)?;
    try_send(svc, &tab, text, Origin::Agent, None)
}

pub(crate) fn try_send_owned(
    svc: &ServiceContext,
    record: &crate::service::pending_delivery_payloads::user_sends::UserSendRecord,
) -> Option<Result<(), DeliveryError>> {
    if svc.chat_send_ledger.has_rpc_handoff(&record.send_id) {
        return Some(Err(DeliveryError::SubmitUnconfirmed {
            tab: record.authority.tab_id.clone(),
            detail: "RPC handoff already claimed; replay refused".into(),
        }));
    }
    if !super::launch::requested(svc, &record.authority.provider_id) {
        return None;
    }
    let text = match svc.pending_delivery_payloads.user_sends.prepared(record) {
        Ok(text) => format!(
            "{text}{}",
            record.typed_follow_up.as_deref().unwrap_or_default()
        ),
        Err(detail) => {
            return Some(Err(DeliveryError::TabBusyRefused {
                tab: record.authority.tab_id.clone(),
                detail,
            }))
        }
    };
    try_send(
        svc,
        &record.authority.tab_id,
        &text,
        Origin::User,
        Some(&record.send_id),
    )
}

pub fn try_send(
    svc: &ServiceContext,
    tab: &str,
    text: &str,
    origin: Origin,
    send_id: Option<&str>,
) -> Option<Result<(), DeliveryError>> {
    if send_id.is_some_and(|id| svc.chat_send_ledger.has_rpc_handoff(id)) {
        return Some(Err(DeliveryError::SubmitUnconfirmed {
            tab: tab.into(),
            detail: "RPC handoff already claimed; replay refused".into(),
        }));
    }
    let provider = svc
        .mux
        .read()
        .get_tab(tab)
        .and_then(|tab| tab.agent_type.clone());
    let capability = provider
        .as_deref()
        .map(|provider| super::launch::requested(svc, provider));
    let state = super::lifecycle::delivery_state(tab);
    let sidecar = super::lifecycle::for_tab(tab);
    let endpoint_present = sidecar.is_some();
    let client_alive = sidecar
        .as_ref()
        .is_some_and(|sidecar| sidecar.client.death().borrow().is_none());
    let process_alive = sidecar
        .as_ref()
        .is_some_and(|sidecar| !*sidecar.process.death.borrow());
    let available = client_alive && process_alive;
    let thread = sidecar
        .as_ref()
        .map(|sidecar| sidecar.thread.lock().clone());
    let server_verified = sidecar
        .as_ref()
        .is_some_and(|sidecar| sidecar.verified.load(Ordering::Acquire));
    let (authenticated, thread_matches, nonce_present, nonce_matches, bootstrap_pending, binding) = {
        let mux = svc.mux.read();
        let target = mux.get_tab(tab);
        let authenticated = target.is_some_and(|tab| tab.launch_session_authenticated());
        let thread_matches =
            target.is_some_and(|tab| tab.agent_session_id.as_deref() == thread.as_deref());
        let nonce_present = target.is_some_and(|tab| tab.session_binding_nonce.is_some());
        let nonce_matches = target.is_some_and(|tab| {
            sidecar.as_ref().is_some_and(|sidecar| {
                tab.session_binding_nonce.as_deref() == Some(sidecar.launch_nonce.as_str())
            })
        });
        let bootstrap_pending = target
            .and_then(|tab| tab.pending_delivery.as_ref())
            .is_some_and(|pending| !pending.state().is_accepted());
        let binding = target.and_then(|tab| {
            (sidecar.is_some()
                && provider.is_some()
                && server_verified
                && authenticated
                && thread_matches)
                .then(|| tab.session_binding_nonce.clone())
                .flatten()
        });
        (
            authenticated,
            thread_matches,
            nonce_present,
            nonce_matches,
            bootstrap_pending,
            binding,
        )
    };
    let attachment_ready = super::lifecycle::startup_attachment_ready(tab);
    let binding_verified = binding.is_some();
    let first_readiness_reached = svc
        .user_send_authority(tab)
        .ok()
        .is_some_and(|authority| svc.user_authority_is_ready(&authority));
    let gate = SendGate::classify(
        capability,
        state,
        available,
        binding_verified,
        first_readiness_reached,
    );
    log::debug!("[rpc/send] tab={tab} provider={provider:?} capability={capability:?} gate={gate:?} runtime_entered={}", tokio::runtime::Handle::try_current().is_ok());
    let (sidecar, thread, nonce) = match (gate, sidecar, thread, binding) {
        (SendGate::Pty, _, _, _) => return None,
        (SendGate::Refuse(reason), _, _, _) => {
            log::info!("[codex-rpc-send] tab={tab} state=refused reason={reason:?} delivery_state={state:?} origin={origin:?} capability={capability:?} endpoint_present={endpoint_present} client_alive={client_alive} process_alive={process_alive} server_verified={server_verified} launch_authenticated={authenticated} thread_matches={thread_matches} nonce_present={nonce_present} nonce_matches={nonce_matches} binding_verified={binding_verified} attachment_ready={attachment_ready} bootstrap_pending={bootstrap_pending}");
            return Some(Err(DeliveryError::TabBusyRefused {
                tab: tab.into(),
                detail: reason.into(),
            }));
        }
        (SendGate::Rpc, Some(sidecar), Some(thread), Some(nonce)) => (sidecar, thread, nonce),
        _ => {
            return Some(Err(DeliveryError::TabBusyRefused {
                tab: tab.into(),
                detail: "sidecar readiness changed before admission".into(),
            }));
        }
    };
    let pending = match send_id {
        Some(id) => svc.chat_send_ledger.status(id),
        None => match svc.chat_send_ledger.try_start_scoped_with_normalization(
            tab,
            "",
            None,
            None,
            Default::default(),
        ) {
            Ok(status) => Some(status),
            Err(detail) => {
                return Some(Err(DeliveryError::TabBusyRefused {
                    tab: tab.into(),
                    detail,
                }))
            }
        },
    };
    let Some(pending) = pending else {
        return Some(Err(DeliveryError::TabBusyRefused {
            tab: tab.into(),
            detail: "RPC ledger admission absent".into(),
        }));
    };
    if !svc.chat_send_ledger.bind_rpc_send(
        &pending.send_id,
        SendLiveBinding {
            session_id: Some(thread.clone()),
            binding_nonce: Some(nonce),
        },
    ) {
        return Some(Err(DeliveryError::SubmitUnconfirmed {
            tab: tab.into(),
            detail: "RPC send was already claimed; replay refused".into(),
        }));
    }
    observe(svc, tab, &sidecar);
    let cwd = svc.mux.read().get_tab(tab).and_then(|tab| tab.cwd.clone());
    let params = start_params(
        &thread,
        &pending.send_id,
        text,
        cwd.as_deref(),
        super::model::turn_overrides(tab),
    );
    let active = super::control::active_turn(tab);
    log::debug!("[rpc/send] tab={tab} branch=block_on origin={origin:?}");
    let result = tauri::async_runtime::block_on(submit(
        origin,
        active.as_deref(),
        params,
        |method, params| sidecar.client.request(method, params),
    ));
    if origin == Origin::User {
        if let Ok(reply) = &result {
            if let Some(turn) = reply.pointer("/turn/id").and_then(Value::as_str) {
                let mut control = sidecar.control.lock();
                if control.started(&thread, turn) {
                    sidecar.busy.store(true, Ordering::Release);
                }
            }
        }
    }
    Some(finish_handoff(svc, tab, &thread, &pending.send_id, result))
}

fn finish_handoff(
    svc: &ServiceContext,
    tab: &str,
    thread: &str,
    send_id: &str,
    result: Result<Value, ClientError>,
) -> Result<(), DeliveryError> {
    match result {
        Ok(_) => {
            if let Some(status) = svc.chat_send_ledger.mark_rpc_accepted(send_id) {
                log::info!(
                    "[codex-rpc-send] tab={tab} thread={thread} send_id={send_id} state=written"
                );
                emit(svc, &status);
            }
            Ok(())
        }
        Err(error) => {
            if let Some(status) = svc.chat_send_ledger.mark_rpc_uncertain(send_id) {
                emit(svc, &status);
            }
            Err(DeliveryError::SubmitUnconfirmed {
                tab: tab.into(),
                detail: format!("RPC handoff uncertain: {error:?}"),
            })
        }
    }
}

pub fn start_params(
    thread: &str,
    id: &str,
    text: &str,
    cwd: Option<&str>,
    overrides: Value,
) -> Value {
    let mut params = json!({"threadId":thread, "clientUserMessageId":id,
        "input":[{"type":"text","text":text}], "approvalsReviewer":"user"});
    if let Some(overrides) = overrides.as_object() {
        for (key, value) in overrides {
            params[key] = value.clone();
        }
    }
    if params["approvalsReviewer"].as_str().is_none() {
        params["approvalsReviewer"] = json!("user");
    }
    if let Some(cwd) = cwd {
        params["cwd"] = json!(cwd);
    }
    params
}

pub async fn submit<F, Fut>(
    origin: Origin,
    active: Option<&str>,
    params: Value,
    mut request: F,
) -> Result<Value, ClientError>
where
    F: FnMut(&'static str, Value) -> Fut,
    Fut: std::future::Future<Output = Result<Value, ClientError>>,
{
    if let (Origin::User, Some(turn)) = (origin, active) {
        let steer = steer_params(&params, turn);
        log::info!("[codex-rpc-send] method=turn/steer thread={} expectedTurnId={turn} clientUserMessageId={}", steer["threadId"], steer["clientUserMessageId"]);
        match request("turn/steer", steer).await {
            Err(ClientError::Rpc(error)) if steer_turn_ended(&error, turn) => {}
            result => return result,
        }
    }
    log::info!(
        "[codex-rpc-send] method=turn/start thread={} clientUserMessageId={}",
        params["threadId"],
        params["clientUserMessageId"]
    );
    request("turn/start", params).await
}

fn steer_params(start: &Value, turn: &str) -> Value {
    let mut params = json!({"threadId":start["threadId"],
        "clientUserMessageId":start["clientUserMessageId"],
        "input":start["input"], "expectedTurnId":turn});
    for key in ["additionalContext", "responsesapiClientMetadata"] {
        if let Some(value) = start.get(key) {
            params[key] = value.clone();
        }
    }
    params
}

fn steer_turn_ended(error: &super::protocol::RpcError, expected: &str) -> bool {
    // Codex exposes these pre-acceptance refusals only as INVALID_REQUEST text.
    error.code == -32600
        && error.data.is_none()
        && (error.message == "no active turn to steer"
            || error
                .message
                .starts_with(&format!("expected active turn id `{expected}` but found `"))
                && error.message.ends_with('`'))
}

pub(crate) fn user_client_id<'a>(method: &str, params: &'a Value) -> Option<(&'a str, &'a str)> {
    if !matches!(method, "item/started" | "item/completed") {
        return None;
    }
    let item = params.get("item")?;
    if item.get("type")?.as_str()? != "userMessage" {
        return None;
    }
    Some((
        params.get("threadId")?.as_str()?,
        item.get("clientId")?.as_str()?,
    ))
}

fn observe(svc: &ServiceContext, tab: &str, sidecar: &Arc<Sidecar>) {
    let mut events = {
        let mut observers = OBSERVERS.lock();
        if observers
            .get(tab)
            .and_then(Weak::upgrade)
            .is_some_and(|old| Arc::ptr_eq(&old, sidecar))
        {
            return;
        }
        let events = sidecar.client.observe();
        observers.insert(tab.into(), Arc::downgrade(sidecar));
        events
    };
    let mut death = sidecar.client.death();
    let mut process_death = sidecar.process.death.clone();
    let weak = Arc::downgrade(sidecar);
    let nonce = svc
        .mux
        .read()
        .get_tab(tab)
        .and_then(|tab| tab.session_binding_nonce.clone());
    let svc = svc.clone();
    let tab = tab.to_string();
    tauri::async_runtime::spawn(async move {
        loop {
            if death.borrow().is_some() || *process_death.borrow() {
                break;
            }
            tokio::select! {
                _ = death.changed() => break,
                _ = process_death.changed() => break,
                event = events.recv() => {
                    let event = match event {
                        Ok(event) => event,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            for status in svc.chat_send_ledger.rpc_uncertain_for_launch(&tab, nonce.as_deref()) { emit(&svc, &status); }
                            continue;
                        },
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    };
                    let Frame::Notification { method, params } = event else { continue };
                    let Some((thread, client_id)) = user_client_id(&method, &params) else { continue };
                    let Some(sidecar) = weak.upgrade() else { break };
                    if !super::lifecycle::for_tab(&tab).is_some_and(|current| Arc::ptr_eq(&current, &sidecar))
                        || !sidecar.verified.load(Ordering::Acquire) || *sidecar.thread.lock() != thread { continue; }
                    let nonce = {
                        let mux = svc.mux.read();
                        mux.get_tab(&tab).filter(|tab| tab.launch_session_authenticated()
                            && tab.agent_session_id.as_deref() == Some(thread))
                            .and_then(|tab| tab.session_binding_nonce.clone())
                    };
                    if let Some(nonce) = nonce {
                        if let Some(status) = svc.chat_send_ledger.confirm_rpc_send(&tab, thread, &nonce, client_id) {
                            log::info!("[codex-rpc-send] tab={tab} thread={thread} clientUserMessageId={client_id} send_id={} state=delivered", status.send_id);
                            emit(&svc, &status);
                        }
                    }
                }
            }
        }
        for status in svc
            .chat_send_ledger
            .rpc_uncertain_for_launch(&tab, nonce.as_deref())
        {
            emit(&svc, &status);
        }
        let mut observers = OBSERVERS.lock();
        if observers
            .get(&tab)
            .is_some_and(|current| Weak::ptr_eq(current, &weak))
        {
            observers.remove(&tab);
        }
    });
}

fn emit(svc: &ServiceContext, status: &ChatSendStatus) {
    crate::api::events::emit_everywhere(&svc.app_handle, "chat-send-status", status);
}

#[cfg(test)]
mod tests;
