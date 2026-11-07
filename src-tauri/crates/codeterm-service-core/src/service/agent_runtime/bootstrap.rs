use super::{
    engine::{self, Session},
    model::RuntimeFailure,
    process::Process,
};
use crate::{
    multiplexer::types::{PendingDeliveryRecord, PendingDeliveryState},
    service::ServiceContext,
};
use std::sync::Arc;

fn claimable(record: &PendingDeliveryRecord) -> bool {
    record.state() == PendingDeliveryState::Pending && record.attempt_provenance().is_none()
}

pub(super) async fn submit(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
) -> Result<(), RuntimeFailure> {
    let context = session.context.lock().clone();
    let pending = svc
        .mux
        .read()
        .get_tab(&context.tab_id)
        .and_then(|tab| tab.pending_delivery.clone());
    let Some(pending) = pending else {
        return Ok(());
    };
    if !claimable(&pending) {
        return Ok(());
    }
    let send_id = pending
        .send_id()
        .ok_or_else(|| {
            RuntimeFailure::Refused("Initial runtime input has no bound send ID".into())
        })?
        .to_string();
    let owner = svc.clone();
    let tab = context.tab_id.clone();
    let record = pending.clone();
    let id = send_id.clone();
    let payload = tokio::task::spawn_blocking(move || {
        let payload = owner
            .pending_delivery_payloads
            .resolve(&record)
            .map_err(|reason| reason.to_string())?;
        owner
            .chat_send_ledger
            .start_runtime_claim(&tab, &payload, &id)?;
        {
            let mut mux = owner.mux.write();
            let target = mux
                .get_tab_mut(&tab)
                .ok_or("Initial runtime tab unavailable")?;
            if target.session_binding_nonce.as_deref() != Some(context.binding_nonce.as_str()) {
                return Err("Initial runtime binding changed".into());
            }
            let pending = target
                .pending_delivery
                .as_mut()
                .filter(|pending| {
                    pending.record_id() == record.record_id()
                        && pending.send_id() == Some(id.as_str())
                        && claimable(pending)
                })
                .ok_or("Initial runtime claim changed")?;
            pending
                .begin_attempt(crate::service::launch_delivery::next_attempt())
                .map_err(|_| "Initial runtime claim already in flight")?;
            pending.mark_unconfirmed();
        }
        let outcome = crate::service::session_persist::write_session_snapshot(&owner, &owner.mux)?;
        if !matches!(outcome, crate::session::store::SaveOutcome::Saved) {
            return Err("Initial runtime claim persistence refused".into());
        }
        Ok::<_, String>(payload)
    })
    .await
    .map_err(|reason| RuntimeFailure::HookFailed(reason.to_string()))?
    .map_err(RuntimeFailure::HookFailed)?;
    engine::validate_binding(svc, session)?;
    let status = svc
        .chat_send_ledger
        .status(&send_id)
        .ok_or(RuntimeFailure::Superseded)?;
    svc.ui.emit(
        "chat-send-status",
        serde_json::to_value(status).unwrap_or_default(),
    );
    let binding = session.context.lock().clone();
    svc.run_watcher
        .arm_runtime(&binding.tab_id, &binding.binding_nonce);
    let result = svc
        .gateway
        .submit_runtime_loaded_turn(
            svc,
            session,
            process,
            &send_id,
            crate::service::messenger::AgentInput::Launch(
                crate::service::messenger::LaunchBootstrap::new(payload),
            ),
        )
        .await;
    readiness_after_submit(result)
}

fn readiness_after_submit(
    result: Result<(), crate::service::delivery_error::DeliveryError>,
) -> Result<(), RuntimeFailure> {
    result
        .or_else(|reason| {
            reason.into_surfaced(crate::service::delivery_error::SendOrigin::Programmatic)
        })
        .map_err(|reason| RuntimeFailure::Refused(reason.ledger_message()))
}

#[cfg(test)]
mod tests;
