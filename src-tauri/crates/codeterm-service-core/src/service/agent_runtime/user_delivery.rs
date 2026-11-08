use super::{engine, model::RuntimePhase, transport};
use crate::service::{
    parked_user_delivery::UserDrain,
    pending_delivery_payloads::user_sends::{UserSendAuthority, MAX_UNSETTLED},
    ServiceContext,
};
use std::sync::atomic::Ordering;

/// Client token of a restart continuation, inside its runtime envelope token.
pub(crate) const TURN_RESUME_TOKEN: &str = "turn-resume:";
const RECOVERY_HOLD: &str = "runtime_recovery_in_progress";

fn resumes_interrupted_turn(
    record: &crate::service::pending_delivery_payloads::user_sends::UserSendRecord,
) -> bool {
    record
        .client_token
        .as_deref()
        .and_then(|token| token.strip_prefix("runtime-envelope:"))
        .is_some_and(|token| token.starts_with(TURN_RESUME_TOKEN))
}

/// While recovery owns the runtime only its continuation may go: held sends follow the turn it resumes.
fn next_queued(
    records: Vec<crate::service::pending_delivery_payloads::user_sends::UserSendRecord>,
    recovering: bool,
) -> Option<crate::service::pending_delivery_payloads::user_sends::UserSendRecord> {
    records
        .into_iter()
        .find(|record| !recovering || resumes_interrupted_turn(record))
}

fn hold_for_recovery(svc: &ServiceContext, tab: &str) -> Result<(), String> {
    for record in svc.pending_delivery_payloads.user_sends.queued(tab)? {
        if let Some(status) = svc
            .chat_send_ledger
            .set_user_pending_reason(&record.send_id, Some(RECOVERY_HOLD))
        {
            svc.ui.emit(
                "chat-send-status",
                serde_json::to_value(status).unwrap_or_default(),
            );
        }
    }
    Ok(())
}

pub(super) fn restore_input(
    record: &crate::service::pending_delivery_payloads::user_sends::UserSendRecord,
    text: &str,
) -> Result<crate::service::messenger::AgentInput, String> {
    use crate::service::messenger::{AgentInput, HumanInput, Messenger};
    if record
        .client_token
        .as_deref()
        .is_some_and(|token| token.starts_with("runtime-envelope:"))
    {
        Ok(AgentInput::Framed(
            Messenger::restore_framed_delivery(text)?.with_provenance(record.input_provenance),
        ))
    } else {
        Ok(AgentInput::Human(HumanInput::composer(text)))
    }
}

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
        // Ready lands right after resume; history backfill continues behind it and never gates a send.
        && matches!(session.state.lock().phase, RuntimePhase::Ready)
        && process.is_some_and(|process| {
            process.ended.borrow().is_none() && process.client.ended().borrow().is_none()
        })
        && engine::validate_binding(svc, &session).is_ok()
}

pub(crate) fn is_current_session(
    svc: &ServiceContext,
    authority: &UserSendAuthority,
    thread: &str,
    nonce: &str,
) -> bool {
    if authority.binding_nonce != nonce || !is_ready(svc, authority) {
        return false;
    }
    let Some(session) = engine::for_tab(&authority.tab_id) else {
        return false;
    };
    let context = session.context.lock().clone();
    let current_epoch = session.state.lock().epoch;
    context.thread_id.as_deref() == Some(thread)
        && context.binding_nonce == nonce
        && current_epoch == context.epoch
        && svc.mux.read().get_tab(&authority.tab_id).is_some_and(|tab| {
            matches!(&tab.backing, crate::multiplexer::types::TabBacking::ChatBackend { plugin_id, session_id }
                if plugin_id == &authority.provider_id && session_id == thread)
                && tab.launch_session_authenticated()
                && tab.reported_session_binding_nonce.as_deref() == Some(nonce)
        })
}

/// A recovery attempt failed on a degraded runtime, and nothing will retry it for these sends.
/// Sign-in and a replaced launch each have an owner that wakes the queue again.
fn degrade_ends_held_sends(
    failure: &super::model::RuntimeFailure,
    published_state: Option<&str>,
) -> bool {
    use super::model::RuntimeFailure;
    published_state == Some("degraded")
        && !matches!(
            failure,
            RuntimeFailure::SignInRequired | RuntimeFailure::Superseded
        )
}

/// A sleeping tab refuses the attempt without trying, so it says nothing about the runtime.
fn published_state(svc: &ServiceContext, tab: &str) -> Option<String> {
    let mux = svc.mux.read();
    let target = mux.get_tab(tab).filter(|target| !target.frozen)?;
    target.agent_runtime_state.as_ref()?["state"]
        .as_str()
        .map(str::to_owned)
}

fn settle_held_sends(svc: &ServiceContext, tab: &str) -> Result<(), String> {
    let store = &svc.pending_delivery_payloads.user_sends;
    for record in store.queued(tab)? {
        svc.chat_send_ledger.restore_user_record(&record);
        if svc
            .chat_send_ledger
            .user_payload_may_have_been_written(&record.send_id)
            || !store.discard_queued_for_degraded_launch(&record)?
        {
            continue;
        }
        if let Some(status) = svc
            .chat_send_ledger
            .expire_owned_before_write(&record.send_id, "launch_degraded_before_write")
        {
            svc.ui.emit(
                "chat-send-status",
                serde_json::to_value(status).unwrap_or_default(),
            );
        }
        super::capture::retire(&record.send_id);
    }
    Ok(())
}

pub(crate) fn drain(svc: &ServiceContext, tab: &str) -> Result<UserDrain, String> {
    let store = &svc.pending_delivery_payloads.user_sends;
    let owner = store.tab(tab);
    let _drain = super::blocking::deadline(
        std::time::Duration::from_secs(30),
        owner.runtime_drain.acquire(),
    )
    .map_err(|_| "submit_unconfirmed: runtime drain outcome pending; replay refused")?
    .map_err(|_| "submit_unconfirmed: runtime drain closed; replay refused")?;
    if owner.revoked.load(Ordering::Acquire) || owner.failed.load(Ordering::Acquire) {
        return Ok(UserDrain::Idle);
    }
    super::inbound::settle_abandoned_tool_claims(svc, tab);
    let queued = store.queued(tab)?;
    if queued.is_empty() {
        return Ok(UserDrain::Idle);
    }
    if next_queued(queued, svc.runtime_recovery_owns_sends(tab)).is_none() {
        hold_for_recovery(svc, tab)?;
        return Ok(UserDrain::Idle);
    }
    let session = match crate::async_runtime::block_on(engine::ensure(svc, tab)) {
        Ok(session) => session,
        Err(reason) => {
            // Recovery may have claimed the runtime while this attempt ran; its completion wakes the owner.
            if svc.runtime_recovery_owns_sends(tab) {
                hold_for_recovery(svc, tab)?;
                return Ok(UserDrain::Idle);
            }
            if degrade_ends_held_sends(&reason, published_state(svc, tab).as_deref()) {
                settle_held_sends(svc, tab)?;
                return Ok(UserDrain::Idle);
            }
            for record in store.queued(tab)? {
                if let Some(status) = svc
                    .chat_send_ledger
                    .set_user_pending_reason(&record.send_id, Some(&reason.to_string()))
                {
                    svc.ui.emit(
                        "chat-send-status",
                        serde_json::to_value(status).unwrap_or_default(),
                    );
                }
            }
            return Ok(UserDrain::Idle);
        }
    };
    for _ in 0..MAX_UNSETTLED {
        let recovering = svc.runtime_recovery_owns_sends(tab);
        let Some(mut record) = next_queued(store.queued(tab)?, recovering) else {
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
                    svc.ui.emit(
                        "chat-send-status",
                        serde_json::to_value(status).unwrap_or_default(),
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
        if super::turn_requests::holds_submit(&session, &text) {
            if let Some(status) = svc.chat_send_ledger.set_user_pending_reason(
                &record.send_id,
                Some(super::turn_requests::ACTIVE_TURN_HOLD),
            ) {
                svc.ui.emit(
                    "chat-send-status",
                    serde_json::to_value(status).unwrap_or_default(),
                );
            }
            return Ok(UserDrain::Idle);
        }
        if record.capture_required && !super::capture::valid(svc, tab, &record.send_id) {
            if store.discard_queued_from_ended_launch(&record)? {
                if let Some(status) = svc.chat_send_ledger.expire_owned_before_write(
                    &record.send_id,
                    "recording_recipient_changed_before_write",
                ) {
                    svc.ui.emit(
                        "chat-send-status",
                        serde_json::to_value(status).unwrap_or_default(),
                    );
                }
                super::capture::retire(&record.send_id);
                continue;
            }
            return Ok(UserDrain::Idle);
        }
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
        let digest = if recovering {
            None
        } else {
            super::inbound::begin_digest(svc, tab, &session, &record, &text, &context)
        };
        let input = match &digest {
            Some(digest) => digest.input.clone(),
            None => restore_input(&record, &text)?,
        };
        let result = transport::send(svc, tab, &input, Some(&record.send_id));
        let zero_write = result
            .as_ref()
            .err()
            .is_some_and(|error| !error.bytes_written())
            && !svc
                .chat_send_ledger
                .user_payload_may_have_been_written(&record.send_id);
        let rejection = result
            .as_ref()
            .err()
            .filter(|error| zero_write && transport::rejected_before_write(error))
            .map(|error| error.ledger_message());
        store.finish_handoff(&record, zero_write)?;
        if zero_write {
            svc.chat_send_ledger
                .zero_write_user_handoff(&record.send_id);
        }
        if let Some(digest) = digest {
            let outcome = result
                .as_ref()
                .map(|_| ())
                .map_err(|reason| reason.ledger_message());
            super::inbound::finish_digest(svc, tab, &session, digest, zero_write, outcome);
        }
        if let Some(status) = svc.chat_send_ledger.finish(
            &record.send_id,
            result.map_err(|reason| reason.ledger_message()),
        ) {
            svc.ui.emit(
                "chat-send-status",
                serde_json::to_value(status).unwrap_or_default(),
            );
        }
        if let Some(reason) = rejection {
            if !store.discard_queued_from_ended_launch(&record)? {
                return Ok(UserDrain::ZeroWriteRefusal {
                    dispatch_generation,
                });
            }
            if let Some(status) = svc
                .chat_send_ledger
                .expire_owned_before_write(&record.send_id, &reason)
            {
                svc.ui.emit(
                    "chat-send-status",
                    serde_json::to_value(status).unwrap_or_default(),
                );
            }
            super::capture::retire(&record.send_id);
            continue;
        }
        if zero_write && record.capture_required {
            if store.discard_queued_from_ended_launch(&record)? {
                if let Some(status) = svc.chat_send_ledger.expire_owned_before_write(
                    &record.send_id,
                    "recording_recipient_changed_before_write",
                ) {
                    svc.ui.emit(
                        "chat-send-status",
                        serde_json::to_value(status).unwrap_or_default(),
                    );
                }
            }
            super::capture::retire(&record.send_id);
            continue;
        }
        if zero_write {
            return Ok(UserDrain::ZeroWriteRefusal {
                dispatch_generation,
            });
        }
        store.complete(&record)?;
        super::capture::retire(&record.send_id);
        if let Some(status) = svc.chat_send_ledger.status(&record.send_id) {
            store.persist_runtime_status(&status)?;
            super::envelope_delivery::record_write(svc, &status)?;
        }
    }
    Ok(UserDrain::Idle)
}

#[cfg(test)]
mod tests;
