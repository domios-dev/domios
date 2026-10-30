use super::DelayedMessage;
use crate::service::ServiceState;
use std::time::Duration;

pub(super) async fn deliver(svc: &ServiceState, msg: &DelayedMessage) -> Result<(), String> {
    let ready = {
        let service = svc.clone();
        let tab_id = msg.tab_id.clone();
        match tokio::task::spawn_blocking(move || {
            service.ensure_tab_pty_for_send(&tab_id)?;
            let deadline = std::time::Instant::now() + Duration::from_secs(120);
            let ready = crate::service::boot_wait::wait_until(deadline, || {
                service.mux.read().get_tab(&tab_id).is_some_and(|tab| {
                    tab.boot_stage == Some(codeterm_types::pty::BootStage::AgentReady)
                })
            });
            if ready {
                Ok(())
            } else {
                Err("recipient did not reach AgentReady before the delivery deadline".to_string())
            }
        })
        .await
        {
            Ok(result) => result,
            Err(error) => Err(format!("delayed readiness task failed: {error}")),
        }
    };
    ready?;

    let envelope = super::reminder_envelope(msg);
    let svc_for_deliver = svc.clone();
    let tab_id = msg.tab_id.clone();
    let body = svc
        .plan_store
        .prepend_heartbeat_for_orchestrator_tab(&tab_id, &msg.text);
    let body = svc
        .plan_store
        .prepend_worker_charter_for_tab(&tab_id, &body);
    let deliver_tab_id = tab_id.clone();
    let message_id = msg.id.clone();
    tokio::task::spawn_blocking(move || {
        if crate::service::agent_runtime::capacity_retry::skip_retry(&deliver_tab_id, &message_id) {
            return Ok(());
        }
        // A turn starting after this check may harmlessly receive an extra continue like user input.
        svc_for_deliver
            .gateway_deliver(
                &deliver_tab_id,
                envelope,
                &body,
                crate::service::message_gateway::DeliveryClass::AgentTraffic,
            )
            .map(|_| ())
    })
    .await
    .map_err(|e| format!("delayed delivery task failed: {e}"))
    .and_then(|res| res)
}
