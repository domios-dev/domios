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
) -> Result<Delivery, String> {
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
        return outcome(
            svc.chat_send_ledger
                .status(&id)
                .ok_or("Durable runtime envelope status unavailable")?,
        );
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
        return outcome(status);
    }
    let store = &svc.pending_delivery_payloads.user_sends;
    let record = store.admit_with_follow_up(&reservation, input, input, None, None, false)?;
    let status = svc.chat_send_ledger.admit_user(&record);
    let status = svc
        .chat_send_ledger
        .attach_system_send(
            &record.send_id,
            input,
            chrono::Utc::now().timestamp_millis().max(0) as u64,
        )
        .unwrap_or(status);
    crate::api::events::emit_everywhere(&svc.app_handle, "chat-send-status", &status);
    let result = super::user_delivery::drain(svc, tab);
    drop(reservation);
    result?;
    let status = svc
        .chat_send_ledger
        .status(&record.send_id)
        .ok_or("Runtime envelope outcome unavailable")?;
    outcome(status)
}

#[cfg(test)]
mod tests {
    use super::*;
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
