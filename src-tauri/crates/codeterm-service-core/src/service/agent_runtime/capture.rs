use crate::service::{
    chat_send_transaction::ChatSendStatus, local_recording::chat::CapturedChat, ServiceContext,
};
use parking_lot::Mutex;
use std::{collections::HashMap, sync::LazyLock};
static CAPTURES: LazyLock<Mutex<HashMap<String, CapturedChat>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) fn revoke(tab: &str) {
    CAPTURES
        .lock()
        .retain(|_, capture| capture.identity.tab_id != tab);
}
pub(super) fn retire(send: &str) {
    CAPTURES.lock().remove(send);
}
pub(super) fn valid(svc: &ServiceContext, tab: &str, send: &str) -> bool {
    let capture = CAPTURES.lock().get(send).cloned();
    capture.is_some_and(|capture| {
        capture.identity.tab_id == tab && capture.validate(svc.captured_chat_identity(tab)).is_ok()
    })
}

pub(super) fn validate_if_present(
    svc: &ServiceContext,
    tab: &str,
    send: &str,
) -> Result<(), super::model::RuntimeFailure> {
    let capture = CAPTURES.lock().get(send).cloned();
    if let Some(capture) = capture {
        capture
            .validate(svc.captured_chat_identity(tab))
            .map_err(|reason| super::model::RuntimeFailure::Refused(reason.ledger_message()))?;
    }
    Ok(())
}

pub(crate) fn admit(
    svc: &ServiceContext,
    capture: CapturedChat,
    text: &str,
    client_token: Option<String>,
) -> Result<ChatSendStatus, String> {
    let tab = capture.identity.tab_id.clone();
    capture
        .validate(svc.captured_chat_identity(&tab))
        .map_err(|reason| reason.ledger_message())?;
    let reservation = svc.reserve_user_send(&tab, client_token)?;
    if reservation.wait_existing()? {
        return svc
            .chat_send_ledger
            .status(&reservation.send_id)
            .ok_or_else(|| "Original captured send status unavailable".into());
    }
    if reservation.authority.binding_nonce != capture.identity.binding_nonce
        || reservation.authority.session_id.as_deref() != Some(&capture.identity.session_id)
    {
        return Err("Recording runtime authority changed".into());
    }
    {
        let mut captures = CAPTURES.lock();
        if captures.len() >= 32 {
            return Err("Runtime capture ticket capacity exceeded".into());
        }
        captures.insert(reservation.send_id.clone(), capture);
    }
    let record = match svc
        .pending_delivery_payloads
        .user_sends
        .admit_runtime_capture(
            &reservation,
            text,
            !svc.user_authority_is_ready(&reservation.authority),
        ) {
        Ok(record) => record,
        Err(reason) => {
            retire(&reservation.send_id);
            return Err(reason);
        }
    };
    let status = svc.chat_send_ledger.admit_user(&record);
    crate::api::events::emit_everywhere(&svc.app_handle, "chat-send-status", &status);
    drop(reservation);
    svc.wake_user_owner(
        &tab,
        crate::service::parked_user_delivery::UserWake::Admission,
    );
    Ok(status)
}
