use super::{lifecycle, protocol::Frame};
use crate::service::chat_send_transaction::ChatSendState;
use crate::service::{
    launch_delivery::FallbackDeliverySpec, launch_readiness::LaunchAttempt, ServiceContext,
};

pub(crate) fn consume(
    svc: &ServiceContext,
    attempt: &LaunchAttempt,
    spec: FallbackDeliverySpec,
) -> Result<bool, String> {
    let tab = &attempt.tab_id;
    let sidecar = lifecycle::for_tab(tab).ok_or("Startup sidecar missing")?;
    let thread = sidecar.thread.lock().clone();
    let mut events = sidecar.client.observe();
    let payload = spec.payload.clone();
    let desired = crate::service::launch_delivery::pending_record_for_spec(spec);
    let record = {
        let mut mux = svc.mux.write();
        let tab = mux.get_tab_mut(tab).ok_or("Startup tab missing")?;
        if tab
            .pending_delivery
            .as_ref()
            .is_some_and(|pending| pending.content_digest != desired.content_digest)
        {
            if tab
                .pending_delivery
                .as_ref()
                .is_some_and(|pending| !pending.state().is_accepted())
            {
                return Err("An unresolved startup record owns this launch".into());
            }
            tab.pending_delivery = None;
        }
        let pending = tab.pending_delivery.get_or_insert(desired);
        if pending.state().is_accepted() {
            return Ok(true);
        }
        pending.clone()
    };
    let send_id = match record.send_id() {
        Some(id) => id.to_owned(),
        None => super::send::admit_startup(svc, tab).map_err(|error| error.to_string())?,
    };
    {
        let mut mux = svc.mux.write();
        let target = mux.get_tab_mut(tab).ok_or("Startup tab missing")?;
        if target.session_binding_nonce.as_deref() != Some(&attempt.binding_nonce)
            || target.pty_id.as_deref() != Some(&attempt.pty_id)
            || target
                .pending_delivery
                .as_ref()
                .map(|pending| pending.record_id())
                != Some(record.record_id())
        {
            return Err("Startup authority changed before handoff".into());
        }
        target.pending_delivery = Some(record.clone().with_send_id(send_id.clone()));
    }
    crate::service::session_persist::persist_session_after_mutation(
        svc,
        crate::service::session_persist::SessionPersistUrgency::Prompt,
    )?;
    let already_delivered = svc
        .chat_send_ledger
        .status(&send_id)
        .is_some_and(|status| status.state == ChatSendState::Sent);
    if !already_delivered {
        let result = super::send::send_startup(svc, tab, &payload, &send_id);
        if let Err(error) = result {
            if !error.bytes_written() {
                return Err(error.to_string());
            }
        }
        crate::async_runtime::block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(30), async {
                loop {
                    let frame = events
                        .recv()
                        .await
                        .map_err(|_| "Startup clientId evidence lost".to_string())?;
                    let Frame::Notification { method, params } = frame else {
                        continue;
                    };
                    if evidence_matches(&method, &params, &thread, &send_id) {
                        return Ok::<(), String>(());
                    }
                }
            })
            .await
        })
        .map_err(|_| {
            "Startup clientId evidence deadline; handoff will not be replayed".to_string()
        })??;
    }
    {
        let mut mux = svc.mux.write();
        let target = mux.get_tab_mut(tab).ok_or("Startup tab missing")?;
        if target.session_binding_nonce.as_deref() != Some(&attempt.binding_nonce)
            || target.pty_id.as_deref() != Some(&attempt.pty_id)
            || target.agent_session_id.as_deref() != Some(&thread)
            || !sidecar.verified.load(std::sync::atomic::Ordering::Acquire)
            || *sidecar.thread.lock() != thread
        {
            return Err("Startup authority changed before consumption".into());
        }
        let pending = target
            .pending_delivery
            .as_mut()
            .filter(|pending| pending.send_id() == Some(send_id.as_str()))
            .ok_or("Startup record replaced")?;
        if !pending.state().is_accepted() {
            pending
                .confirm_strong()
                .map_err(|error| format!("Startup confirmation failed: {error:?}"))?;
        }
    }
    svc.cleanup_confirmed_pending_delivery(tab)
        .map_err(|error| error.to_string())?;
    Ok(true)
}

fn evidence_matches(method: &str, params: &serde_json::Value, thread: &str, send_id: &str) -> bool {
    super::send::user_client_id(method, params) == Some((thread, send_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn codex_rpc_review_bootstrap_consumes_only_own_client_id() {
        let event = json!({"threadId":"own", "item":{"type":"userMessage","clientId":"send"}});
        assert!(evidence_matches("item/started", &event, "own", "send"));
        assert!(!evidence_matches("item/started", &event, "foreign", "send"));
        assert!(!evidence_matches("item/started", &event, "own", "other"));
        assert!(!evidence_matches("turn/completed", &event, "own", "send"));
    }
}
