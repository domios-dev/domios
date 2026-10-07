use super::{client::ClientError, lifecycle::Sidecar, protocol::Frame};
use crate::service::{
    chat_send_transaction::{ChatSendStatus, SendLiveBinding},
    delivery_error::DeliveryError,
    ServiceContext,
};
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
        .and_then(|tab| tab.agent_type.clone())?;
    if !super::launch::requested(svc, &provider) {
        return None;
    }
    let sidecar = super::lifecycle::for_tab(tab)?;
    if sidecar.client.death().borrow().is_some() || *sidecar.process.death.borrow() {
        return None;
    }
    let thread = sidecar.thread.lock().clone();
    let binding = {
        let mux = svc.mux.read();
        mux.get_tab(tab).and_then(|tab| {
            (sidecar.verified.load(Ordering::Acquire)
                && tab.launch_session_authenticated()
                && tab.agent_session_id.as_deref() == Some(&thread))
            .then(|| tab.session_binding_nonce.clone())
            .flatten()
        })
    };
    let Some(nonce) = binding else {
        return Some(Err(DeliveryError::TabBusyRefused {
            tab: tab.into(),
            detail: "sidecar thread binding pending".into(),
        }));
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
    let result = tauri::async_runtime::block_on(submit(
        origin,
        active.as_deref(),
        params,
        |method, params| sidecar.client.request(method, params),
    ));
    match result {
        Ok(_) => {
            log::info!(
                "[codex-rpc-send] tab={tab} thread={thread} clientUserMessageId={} state=written",
                pending.send_id
            );
            if let Some(status) = svc.chat_send_ledger.mark_rpc_accepted(&pending.send_id) {
                emit(svc, &status);
            }
            Some(Ok(()))
        }
        Err(error) => {
            let error = DeliveryError::SubmitUnconfirmed {
                tab: tab.into(),
                detail: format!("RPC handoff uncertain: {error:?}"),
            };
            if let Some(status) = svc.chat_send_ledger.mark_rpc_uncertain(&pending.send_id) {
                emit(svc, &status);
            }
            Some(Err(error))
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
    if let (Origin::User, Some(active)) = (origin, active) {
        log::info!("[codex-rpc-send] method=turn/steer thread={} clientUserMessageId={} expectedTurnId={active}", params["threadId"], params["clientUserMessageId"]);
        let steer = json!({"threadId":params["threadId"], "expectedTurnId":active,
            "input":params["input"], "clientUserMessageId":params["clientUserMessageId"]});
        match request("turn/steer", steer).await {
            Err(ClientError::Rpc(error)) if not_steerable(&error) => {}
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

fn not_steerable(error: &super::protocol::RpcError) -> bool {
    error.data.as_ref().is_some_and(|data| {
        data.get("codexErrorInfo").is_some_and(|info| {
            info.get("activeTurnNotSteerable")
                .and_then(Value::as_object)
                .is_some()
        }) || data
            .get("activeTurnNotSteerable")
            .and_then(Value::as_object)
            .is_some()
    })
}

fn user_client_id<'a>(method: &str, params: &'a Value) -> Option<(&'a str, &'a str)> {
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
                            log::info!("[codex-rpc-send] tab={tab} thread={thread} clientUserMessageId={client_id} state=delivered");
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
