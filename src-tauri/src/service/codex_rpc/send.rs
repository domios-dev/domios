mod busy;
mod gate;
mod startup;
pub(crate) use busy::{joined_text, user_turn_pending};

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

pub fn try_send(
    svc: &ServiceContext,
    tab: &str,
    text: &str,
    origin: Origin,
    send_id: Option<&str>,
) -> Option<Result<(), DeliveryError>> {
    try_send_group(svc, tab, text, origin, send_id, &[])
}

pub(crate) fn try_send_group(
    svc: &ServiceContext,
    tab: &str,
    text: &str,
    origin: Origin,
    send_id: Option<&str>,
    members: &[&str],
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
    let gate = SendGate::classify(capability, state, available, binding_verified);
    let (sidecar, thread, nonce) = match (gate, sidecar, thread, binding) {
        (SendGate::Pty, _, _, _) => return None,
        (SendGate::Hold(reason), _, _, _) => {
            log::info!("[codex-rpc-send] tab={tab} state=held reason={reason:?} delivery_state={state:?} origin={origin:?} capability={capability:?} endpoint_present={endpoint_present} client_alive={client_alive} process_alive={process_alive} server_verified={server_verified} launch_authenticated={authenticated} thread_matches={thread_matches} nonce_present={nonce_present} nonce_matches={nonce_matches} binding_verified={binding_verified} attachment_ready={attachment_ready} bootstrap_pending={bootstrap_pending}");
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
    let mut admission = match busy::Admission::acquire(
        &sidecar,
        origin,
        super::control::active_turn(tab).is_some(),
    ) {
        Ok(admission) => admission,
        Err(detail) => {
            return Some(Err(DeliveryError::TabBusyRefused {
                tab: tab.into(),
                detail: detail.into(),
            }))
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
    let ids = if members.is_empty() {
        vec![pending.send_id.as_str()]
    } else {
        members.to_vec()
    };
    if ids.first().copied() != Some(pending.send_id.as_str())
        || !svc.chat_send_ledger.bind_rpc_group(
            &ids,
            SendLiveBinding {
                session_id: Some(thread.clone()),
                binding_nonce: Some(nonce),
            },
        )
    {
        return Some(Err(DeliveryError::SubmitUnconfirmed {
            tab: tab.into(),
            detail: "RPC send was already claimed; replay refused".into(),
        }));
    }
    admission.commit();
    observe(svc, tab, &sidecar);
    let cwd = svc.mux.read().get_tab(tab).and_then(|tab| tab.cwd.clone());
    let params = start_params(
        &thread,
        &pending.send_id,
        text,
        cwd.as_deref(),
        super::model::turn_overrides(tab),
    );
    let result = tauri::async_runtime::block_on(submit(origin, None, params, |method, params| {
        sidecar.client.request(method, params)
    }));
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
    for id in ids.iter().skip(1) {
        let _ = finish_handoff(svc, tab, &thread, id, result.clone());
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
    if busy::hold(origin, active.is_some()) {
        return Err(ClientError::Closed(
            "busy user input must remain in the durable queue before RPC claim".into(),
        ));
    }
    log::info!(
        "[codex-rpc-send] method=turn/start thread={} clientUserMessageId={}",
        params["threadId"],
        params["clientUserMessageId"]
    );
    request("turn/start", params).await
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
                        for status in svc.chat_send_ledger.confirm_rpc_group(&tab, thread, &nonce, client_id) {
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
