use super::{engine, model::RuntimePhase, transport};
use crate::service::{
    parked_user_delivery::UserDrain,
    pending_delivery_payloads::user_sends::{UserSendAuthority, MAX_UNSETTLED},
    ServiceContext,
};
use std::sync::atomic::Ordering;

pub(crate) fn is_ready(svc: &ServiceContext, authority: &UserSendAuthority) -> bool {
    let owner = svc
        .pending_delivery_payloads
        .user_sends
        .tab(&authority.tab_id);
    if owner.revoked.load(Ordering::Acquire) || owner.failed.load(Ordering::Acquire) {
        return false;
    }
    let Some(session) = engine::for_tab(&authority.tab_id) else {
        return false;
    };
    let context = session.context.lock().clone();
    let process = session.process.lock().clone();
    authority.provider_id == session.provider
        && authority.binding_nonce == context.binding_nonce
        && authority
            .observed_session_id
            .as_deref()
            .is_none_or(|thread| context.thread_id.as_deref() == Some(thread))
        && context.thread_id.is_some()
        && matches!(session.state.lock().phase, RuntimePhase::Ready)
        && process.is_some_and(|process| {
            process.ended.borrow().is_none() && process.client.ended().borrow().is_none()
        })
        && engine::validate_binding(svc, &session).is_ok()
}

pub(crate) fn drain(svc: &ServiceContext, tab: &str) -> Result<UserDrain, String> {
    let store = &svc.pending_delivery_payloads.user_sends;
    let owner = store.tab(tab);
    if owner.revoked.load(Ordering::Acquire) || owner.failed.load(Ordering::Acquire) {
        return Ok(UserDrain::Idle);
    }
    if store.queued(tab)?.is_empty() {
        return Ok(UserDrain::Idle);
    }
    let session = match tauri::async_runtime::block_on(engine::ensure(svc, tab)) {
        Ok(session) => session,
        Err(reason) => {
            for record in store.queued(tab)? {
                if let Some(status) = svc
                    .chat_send_ledger
                    .set_user_pending_reason(&record.send_id, Some(&reason.to_string()))
                {
                    crate::api::events::emit_everywhere(
                        &svc.app_handle,
                        "chat-send-status",
                        status,
                    );
                }
            }
            return Ok(UserDrain::Idle);
        }
    };
    for _ in 0..MAX_UNSETTLED {
        let Some(mut record) = store.queued(tab)?.into_iter().next() else {
            break;
        };
        svc.chat_send_ledger.restore_user_record(&record);
        let authority = svc.user_send_authority(tab)?;
        if let Some(carried) = store.carry_held_send(&record, authority)? {
            if !svc.chat_send_ledger.rebind_queued_user_authority(
                &record.send_id,
                &record.authority,
                &carried.authority,
            ) {
                return Err("Runtime queued authority has no joined ledger admission".into());
            }
            record = carried;
        }
        let context = session.context.lock().clone();
        if record.authority.provider_id != session.provider
            || record
                .authority
                .observed_session_id
                .as_deref()
                .is_some_and(|thread| context.thread_id.as_deref() != Some(thread))
        {
            if store.discard_queued_from_ended_launch(&record)? {
                if let Some(status) = svc
                    .chat_send_ledger
                    .expire_owned_before_write(&record.send_id, "launch_ended_before_write")
                {
                    crate::api::events::emit_everywhere(
                        &svc.app_handle,
                        "chat-send-status",
                        status,
                    );
                }
                continue;
            }
            return Ok(UserDrain::Idle);
        }
        if !is_ready(svc, &record.authority) || !store.eligible(&record) {
            return Ok(UserDrain::Idle);
        }
        let text = store.prepared(&record)?;
        let thread = context
            .thread_id
            .as_deref()
            .ok_or("Runtime thread unavailable")?;
        if !store.claim(&record, Some(thread))? {
            return Ok(UserDrain::Idle);
        }
        if !svc
            .chat_send_ledger
            .begin_user_handoff(&record, &text, svc.chat_send_anchor(tab))
        {
            return Err(
                "Durable runtime claim has no joined ledger admission; held uncertainty".into(),
            );
        }
        let dispatch_generation = owner.dispatch_generation.load(Ordering::Acquire);
        let result = transport::send(svc, tab, &text, Some(&record.send_id));
        let zero_write = result
            .as_ref()
            .err()
            .is_some_and(|error| !error.bytes_written())
            && !svc
                .chat_send_ledger
                .user_payload_may_have_been_written(&record.send_id);
        store.finish_handoff(&record, zero_write)?;
        if zero_write {
            svc.chat_send_ledger
                .zero_write_user_handoff(&record.send_id);
        }
        if let Some(status) = svc.chat_send_ledger.finish(
            &record.send_id,
            result.map_err(|reason| reason.ledger_message()),
        ) {
            crate::api::events::emit_everywhere(&svc.app_handle, "chat-send-status", status);
        }
        if zero_write {
            return Ok(UserDrain::ZeroWriteRefusal {
                dispatch_generation,
            });
        }
        store.complete(&record)?;
        if let Some(status) = svc.chat_send_ledger.status(&record.send_id) {
            store.persist_runtime_status(&status)?;
        }
    }
    Ok(UserDrain::Idle)
}
