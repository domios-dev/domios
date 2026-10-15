use crate::service::{
    chat_send_transaction::{ChatSendState, ChatSendStatus},
    messenger::Delivery,
    ServiceContext,
};

fn outcome(status: ChatSendStatus) -> Result<Delivery, String> {
    match status.state {
        ChatSendState::Pending => Ok(Delivery::Pending {
            delivery_id: status.send_id,
        }),
        ChatSendState::Written | ChatSendState::Sent => Ok(Delivery::ChatBackend {
            message_id: status.send_id,
        }),
        ChatSendState::Unknown => Err(
            "submit_unconfirmed: runtime envelope already has a possible handoff; replay refused"
                .into(),
        ),
        ChatSendState::Failed => Err(status
            .error
            .unwrap_or_else(|| "Runtime envelope was not delivered".into())),
    }
}

pub(super) fn send(
    svc: &ServiceContext,
    tab: &str,
    input: &str,
    token: &str,
    observer: Option<crate::service::messenger::DeliveryWriteObserver>,
) -> Result<Delivery, String> {
    svc.wake_frozen_tab(tab)?;
    let token = format!("runtime-envelope:{token}");
    svc.restore_durable_user_statuses(tab)?;
    let store = &svc.pending_delivery_payloads.user_sends;
    if let Some(id) = store.runtime_envelope_id(tab, &token)? {
        let status = svc
            .chat_send_ledger
            .status(&id)
            .ok_or("Durable runtime envelope receipt unavailable")?;
        if status.state == ChatSendState::Pending {
            super::user_delivery::drain(svc, tab)?;
        }
        let status = svc
            .chat_send_ledger
            .status(&id)
            .ok_or("Durable runtime envelope status unavailable")?;
        record_write(svc, &status)?;
        return outcome(status);
    }
    tauri::async_runtime::block_on(super::engine::ensure(svc, tab))
        .map_err(|reason| reason.to_string())?;
    let reservation = svc.reserve_user_send(tab, Some(token))?;
    if reservation.wait_existing()? {
        let status = svc
            .chat_send_ledger
            .status(&reservation.send_id)
            .ok_or("Runtime envelope receipt unavailable")?;
        if status.state == ChatSendState::Pending {
            svc.wake_user_owner(
                tab,
                crate::service::parked_user_delivery::UserWake::Admission,
            );
        }
        record_write(svc, &status)?;
        return outcome(status);
    }
    let store = &svc.pending_delivery_payloads.user_sends;
    let record = store.admit_runtime_envelope(&reservation, input, observer)?;
    let status = svc.chat_send_ledger.admit_user(&record);
    let status = svc
        .chat_send_ledger
        .attach_system_send(
            &record.send_id,
            input,
            chrono::Utc::now().timestamp_millis().max(0) as u64,
        )
        .unwrap_or(status);
    crate::events::emit_everywhere(&svc.app_handle, "chat-send-status", &status);
    drop(reservation);
    super::user_delivery::drain(svc, tab)?;
    let status = svc
        .chat_send_ledger
        .status(&record.send_id)
        .ok_or("Runtime envelope outcome unavailable")?;
    outcome(status)
}

pub(super) fn record_write(svc: &ServiceContext, status: &ChatSendStatus) -> Result<(), String> {
    let observation = svc
        .pending_delivery_payloads
        .user_sends
        .take_runtime_write_observer(status);
    match observation {
        Ok(Some(observer)) => {
            svc.record_runtime_delivery_written(&status.tab_id, &status.send_id, observer)
        }
        Ok(None) => {}
        Err(reason) => {
            if let Some(session) = super::engine::for_tab(&status.tab_id) {
                if super::engine::validate_binding(svc, &session).is_ok() {
                    let epoch = session.state.lock().epoch;
                    session.state.lock().degrade(
                        epoch,
                        &super::model::RuntimeFailure::HookFailed(reason.clone()),
                    );
                    super::engine::publish(svc, &status.tab_id, &session);
                }
            }
            return Err(reason);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepted_runtime_status_is_written_and_exact_live_evidence_is_delivered() {
        let ledger = crate::service::chat_send_transaction::ChatSendLedger::default();
        let pending = ledger.try_start("tab", "opaque", None).unwrap();
        assert!(ledger.bind_rpc_send(
            &pending.send_id,
            crate::service::chat_send_transaction::SendLiveBinding {
                session_id: Some("thread".into()),
                binding_nonce: Some("nonce".into()),
            }
        ));
        let accepted = ledger.mark_rpc_accepted(&pending.send_id).unwrap();
        assert!(
            matches!(outcome(accepted).unwrap(), Delivery::ChatBackend {message_id} if message_id == pending.send_id)
        );
        let finished = ledger.finish(&pending.send_id, Ok(())).unwrap();
        assert!(
            matches!(outcome(finished).unwrap(), Delivery::ChatBackend {message_id} if message_id == pending.send_id)
        );
        assert!(ledger
            .confirm_rpc_send("tab", "foreign", "nonce", &pending.send_id)
            .is_none());
        let delivered = ledger
            .confirm_rpc_send("tab", "thread", "nonce", &pending.send_id)
            .unwrap();
        assert_eq!(delivered.state, ChatSendState::Sent);
        assert!(
            matches!(outcome(delivered).unwrap(), Delivery::ChatBackend {message_id} if message_id == pending.send_id)
        );
    }
    #[test]
    fn repeated_uncertain_receipt_is_a_refusal_and_never_a_second_submit() {
        let ledger = crate::service::chat_send_transaction::ChatSendLedger::default();
        let status = ledger.try_start("tab", "opaque", None).unwrap();
        assert!(
            matches!(outcome(status.clone()).unwrap(), Delivery::Pending {delivery_id} if delivery_id == status.send_id)
        );
        let uncertain = ledger
            .finish(&status.send_id, Err("submit_unconfirmed: opaque".into()))
            .unwrap();
        assert_eq!(uncertain.state, ChatSendState::Unknown);
        assert!(outcome(uncertain.clone()).is_err());
        assert!(outcome(uncertain).is_err());
        let delivered = ledger.confirm_send(&status.send_id).unwrap();
        assert!(
            matches!(outcome(delivered).unwrap(), Delivery::ChatBackend {message_id} if message_id == status.send_id)
        );
    }
}
