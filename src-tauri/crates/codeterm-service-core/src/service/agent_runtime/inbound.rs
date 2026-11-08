//! Input that reaches a busy agent without a prompt of its own: inside this tab's own host tool
//! results while a turn runs, or folded into one digest when that turn ends.
//!
//! Each held send stays write-once. A durable claim takes it off the queue before any writer sees
//! it, nothing replays a claim, and only proof that no writer could have seen it returns it.

use super::{
    engine::{self, Session},
    subscriber, turn_requests, user_delivery,
};
use crate::service::{
    chat_send_transaction::{ChatSendState, SendLiveBinding},
    messenger::{AgentInput, MessageEnvelope, Messenger},
    pending_delivery_payloads::user_sends::UserSendRecord,
    ServiceContext,
};
use codeterm_plugin::agent_runtime::*;
use codeterm_types::agent::chat_types::ChatMsg;
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    sync::{atomic::Ordering, Arc, LazyLock},
};

/// Agents truncate long tool output, and truncation would lose a message counted as delivered.
const TOOL_BUDGET_BYTES: usize = 32 * 1024;
const DIGEST_BUDGET_BYTES: usize = 1024 * 1024;
const RETAINED_SETTLED_CLAIMS: usize = 8;
const DIGEST_CONTEXT: &str = "inbound-digest";

struct ToolClaim {
    id: String,
    nonce: String,
    epoch: u64,
    thread: String,
    turn: String,
    records: Vec<UserSendRecord>,
    settled: bool,
}

static TOOL_CLAIMS: LazyLock<Mutex<HashMap<String, Vec<ToolClaim>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn is_framed(record: &UserSendRecord) -> bool {
    record
        .client_token
        .as_deref()
        .is_some_and(|token| token.starts_with("runtime-envelope:"))
}

/// Plain text only: a command, attachment, capture or foreign provenance keeps its own prompt.
fn foldable(record: &UserSendRecord, text: &str) -> bool {
    !text.starts_with('/')
        && !record.capture_required
        && record.typed_follow_up.is_none()
        && record.input_provenance.is_local()
        && user_delivery::restore_input(record, text)
            .map_err(|reason| reason.to_string())
            .and_then(|input| {
                super::transport::runtime_input(&input).map_err(|reason| reason.to_string())
            })
            .is_ok_and(|input| input.attachments.is_empty())
}

fn section(entries: &[(UserSendRecord, String)]) -> String {
    let total = entries.len();
    let mut text = String::from(
        "Domios held these messages while your turn was running. They are in arrival order, each is delivered once and never repeated. Handle each one as its own routed message.\n",
    );
    for (index, (record, body)) in entries.iter().enumerate() {
        text.push_str(&format!(
            "\n--- message {}/{total} id={} ---\n",
            index + 1,
            record.send_id
        ));
        if !is_framed(record) {
            text.push_str("From your owner:\n");
        }
        text.push_str(body);
        text.push('\n');
    }
    text
}

fn now() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

fn emit_status(svc: &ServiceContext, status: impl serde::Serialize) {
    svc.ui.emit(
        "chat-send-status",
        serde_json::to_value(status).unwrap_or_default(),
    );
}

fn hand_off(
    svc: &ServiceContext,
    tab: &str,
    record: &UserSendRecord,
    text: &str,
    thread: &str,
    binding: &SendLiveBinding,
) -> bool {
    let store = &svc.pending_delivery_payloads.user_sends;
    if !store.claim(record, Some(thread)).unwrap_or(false) {
        return false;
    }
    if svc
        .chat_send_ledger
        .begin_user_handoff(record, text, svc.chat_send_anchor(tab))
        && svc
            .chat_send_ledger
            .bind_rpc_send(&record.send_id, binding.clone())
    {
        return true;
    }
    // No writer has seen this claim, so it goes back in order.
    svc.chat_send_ledger
        .zero_write_user_handoff(&record.send_id);
    if let Err(reason) = store.finish_handoff(record, true) {
        log::error!(
            "[inbound] tab={tab} send_id={} claim release failed: {reason}",
            record.send_id
        );
    }
    false
}

/// Settles a handed-off send whose writer was reached: it can no longer return to the queue.
fn retire_claim(svc: &ServiceContext, tab: &str, record: &UserSendRecord) {
    let store = &svc.pending_delivery_payloads.user_sends;
    if let Err(reason) = store
        .finish_handoff(record, false)
        .and_then(|()| store.complete(record).map(|_| ()))
    {
        log::error!(
            "[inbound] tab={tab} send_id={} claim settlement failed: {reason}",
            record.send_id
        );
    }
}

fn persist_outcome(svc: &ServiceContext, send_id: &str) {
    let Some(status) = svc.chat_send_ledger.status(send_id) else {
        return;
    };
    if let Err(reason) = svc
        .pending_delivery_payloads
        .user_sends
        .persist_runtime_status(&status)
        .and_then(|()| super::envelope_delivery::record_write(svc, &status))
    {
        log::error!("[inbound] send_id={send_id} outcome persistence failed: {reason}");
    }
}

fn claimed_row(session: &Session, thread: &str, send_id: &str) -> Option<ChatMsg> {
    let claim = session
        .context
        .lock()
        .claimed_sends
        .iter()
        .find(|claim| claim.send_id == send_id && claim.thread_id == thread)
        .cloned()?;
    let mut item = session
        .maps
        .lock()
        .routed_input(send_id, thread)
        .unwrap_or_else(|| ChatMsg {
            kind: "user".into(),
            content: claim.input.text.clone(),
            ..Default::default()
        });
    item.id = super::events::RuntimeMaps::written_input_id(thread, send_id);
    item.attachments =
        (!claim.input.attachments.is_empty()).then(|| claim.input.attachments.clone());
    Some(item)
}

/// The input reached the agent inside its running turn: show it there and confirm it, once.
pub(super) async fn publish_delivered(
    svc: &ServiceContext,
    session: &Arc<Session>,
    epoch: u64,
    thread: &str,
    turn: &str,
    send_id: &str,
) {
    let mut events = Vec::with_capacity(2);
    if let Some(item) = claimed_row(session, thread, send_id) {
        events.push(AgentRuntimeEvent::ChatItem {
            thread_id: thread.into(),
            item,
        });
    }
    events.push(AgentRuntimeEvent::SendEvidence {
        thread_id: thread.into(),
        send_id: send_id.into(),
        turn_id: turn.into(),
        item_id: None,
    });
    if let Err(reason) = subscriber::publish_events(svc, session, epoch, events, now()).await {
        log::warn!("[inbound] send_id={send_id} delivery publication failed: {reason}");
    }
}

impl ServiceContext {
    /// The text a host tool result of this exact launch should carry, in arrival order.
    /// The caller writes it, then settles `claim_id`; an unsettled claim is never written again.
    pub fn claim_inbound_tool_messages(
        &self,
        tab: &str,
        nonce: &str,
        claim_id: &str,
    ) -> Result<Option<String>, String> {
        let Some(session) = engine::for_tab(tab) else {
            return Ok(None);
        };
        let context = session.context.lock().clone();
        let (Some(thread), Some(turn)) =
            (context.thread_id.clone(), context.active_turn_id.clone())
        else {
            return Ok(None);
        };
        if context.binding_nonce != nonce
            || context.host_mcp.is_none()
            || !turn_requests::holds_new_submit(&session)
            || session.prompts_cancelled.load(Ordering::Acquire)
            || session.compaction.lock().held(context.epoch)
            || self.require_managed_delivery_allowed(tab, None).is_err()
        {
            return Ok(None);
        }
        let store = &self.pending_delivery_payloads.user_sends;
        let owner = store.tab(tab);
        // A running drain owns the queue; the next tool result looks again.
        let Ok(_drain) = owner.runtime_drain.try_acquire() else {
            return Ok(None);
        };
        if owner.revoked.load(Ordering::Acquire) || owner.failed.load(Ordering::Acquire) {
            return Ok(None);
        }
        let binding = SendLiveBinding {
            session_id: Some(thread.clone()),
            binding_nonce: Some(context.binding_nonce.clone()),
        };
        let room = MAX_RUNTIME_BATCH.saturating_sub(context.claimed_sends.len());
        let mut entries = Vec::new();
        let mut bytes = 0;
        for record in store.queued(tab)?.into_iter().take(room) {
            self.chat_send_ledger.restore_user_record(&record);
            if !user_delivery::is_ready(self, &record.authority)
                || !store.eligible(&record)
                || self
                    .require_admitted_input_allowed(tab, Some(&record.send_id))
                    .is_err()
            {
                break;
            }
            let Ok(text) = store.prepared(&record) else {
                break;
            };
            if !foldable(&record, &text)
                || bytes + text.len() > TOOL_BUDGET_BYTES
                || !hand_off(self, tab, &record, &text, &thread, &binding)
            {
                break;
            }
            bytes += text.len();
            entries.push((record, text));
        }
        if entries.is_empty() {
            return Ok(None);
        }
        for (record, text) in &entries {
            let Ok(input) = user_delivery::restore_input(record, text) else {
                continue;
            };
            if let AgentInput::Framed(frame) = &input {
                let mut maps = session.maps.lock();
                maps.sync_active_turn(&context);
                if let Err(reason) = maps.remember_routed_input(&record.send_id, &thread, frame) {
                    log::warn!(
                        "[inbound] tab={tab} send_id={} row unavailable: {reason}",
                        record.send_id
                    );
                }
            }
            if let Ok(input) = super::transport::runtime_input(&input) {
                session.context.lock().claimed_sends.push(ClaimedSend {
                    send_id: record.send_id.clone(),
                    thread_id: thread.clone(),
                    input,
                });
            }
        }
        let text = format!("## Inbound messages\n\n{}", section(&entries));
        log::info!(
            "[inbound] tab={tab} claim={claim_id} turn={turn} messages={} claimed for a tool result",
            entries.len()
        );
        TOOL_CLAIMS
            .lock()
            .entry(tab.into())
            .or_default()
            .push(ToolClaim {
                id: claim_id.into(),
                nonce: nonce.into(),
                epoch: context.epoch,
                thread,
                turn,
                records: entries.into_iter().map(|(record, _)| record).collect(),
                settled: false,
            });
        Ok(Some(text))
    }

    /// `written` means the tool result holding the claim went to the agent. False means the
    /// caller never received the text, the only case that returns a claim to the queue.
    pub fn settle_inbound_tool_messages(
        &self,
        tab: &str,
        nonce: &str,
        claim_id: &str,
        written: bool,
    ) -> Result<(), String> {
        let claim = {
            let mut claims = TOOL_CLAIMS.lock();
            let Some(list) = claims.get_mut(tab) else {
                return Ok(());
            };
            let Some(index) = list
                .iter()
                .position(|claim| claim.id == claim_id && claim.nonce == nonce)
            else {
                return Ok(());
            };
            list.remove(index)
        };
        if written {
            deliver_tool_claim(self, tab, &claim);
        } else if !claim.settled {
            release_tool_claim(self, tab, &claim);
        }
        Ok(())
    }
}

fn deliver_tool_claim(svc: &ServiceContext, tab: &str, claim: &ToolClaim) {
    for record in &claim.records {
        if !claim.settled {
            retire_claim(svc, tab, record);
        }
        if let Some(status) = svc.chat_send_ledger.mark_rpc_accepted(&record.send_id) {
            emit_status(svc, status);
        }
    }
    let session = engine::for_tab(tab).filter(|session| {
        let context = session.context.lock();
        context.epoch == claim.epoch && context.binding_nonce == claim.nonce
    });
    for record in &claim.records {
        match &session {
            Some(session) => crate::async_runtime::block_on(publish_delivered(
                svc,
                session,
                claim.epoch,
                &claim.thread,
                &claim.turn,
                &record.send_id,
            )),
            None => {
                if let Some(status) = svc.chat_send_ledger.confirm_rpc_send(
                    tab,
                    &claim.thread,
                    &claim.nonce,
                    &record.send_id,
                    None,
                ) {
                    emit_status(svc, status);
                }
                persist_outcome(svc, &record.send_id);
            }
        }
        log::info!(
            "[inbound] tab={tab} send_id={} delivered in a tool result",
            record.send_id
        );
    }
}

fn release_tool_claim(svc: &ServiceContext, tab: &str, claim: &ToolClaim) {
    let store = &svc.pending_delivery_payloads.user_sends;
    let binding = SendLiveBinding {
        session_id: Some(claim.thread.clone()),
        binding_nonce: Some(claim.nonce.clone()),
    };
    for record in &claim.records {
        release_handoff(svc, tab, store, record, &binding);
    }
    if let Some(session) = engine::for_tab(tab) {
        session
            .context
            .lock()
            .claimed_sends
            .retain(|claimed| !claim.records.iter().any(|r| r.send_id == claimed.send_id));
    }
    svc.wake_user_owner_on_state_change(tab);
}

fn release_handoff(
    svc: &ServiceContext,
    tab: &str,
    store: &crate::service::pending_delivery_payloads::user_sends::UserSendStore,
    record: &UserSendRecord,
    binding: &SendLiveBinding,
) {
    svc.chat_send_ledger
        .clear_rpc_send_after_definite_refusal(&record.send_id, binding);
    svc.chat_send_ledger
        .zero_write_user_handoff(&record.send_id);
    if let Err(reason) = store.finish_handoff(record, true) {
        log::error!(
            "[inbound] tab={tab} send_id={} claim release failed: {reason}",
            record.send_id
        );
    }
    if let Some(status) = svc
        .chat_send_ledger
        .finish(&record.send_id, Err("held".into()))
    {
        emit_status(svc, status);
    }
}

/// A claim the agent's turn outlived, or a launch ended, has no writer left to confirm it.
/// It may have been written, so it settles as uncertain and is never replayed.
pub(super) fn settle_abandoned_tool_claims(svc: &ServiceContext, tab: &str) {
    let running = engine::for_tab(tab)
        .filter(|session| turn_requests::holds_new_submit(session))
        .map(|session| session.context.lock().binding_nonce.clone());
    let mut abandoned = Vec::new();
    {
        let mut claims = TOOL_CLAIMS.lock();
        let Some(list) = claims.get_mut(tab) else {
            return;
        };
        for claim in list
            .iter_mut()
            .filter(|claim| !claim.settled && running.as_deref() != Some(claim.nonce.as_str()))
        {
            claim.settled = true;
            abandoned.extend(claim.records.iter().cloned());
        }
        while list.iter().filter(|claim| claim.settled).count() > RETAINED_SETTLED_CLAIMS {
            let Some(oldest) = list.iter().position(|claim| claim.settled) else {
                break;
            };
            list.remove(oldest);
        }
    }
    for record in abandoned {
        log::warn!(
            "[inbound] tab={tab} send_id={} tool claim unconfirmed; not replayed",
            record.send_id
        );
        retire_claim(svc, tab, &record);
        if let Some(status) = svc.chat_send_ledger.mark_rpc_uncertain(&record.send_id) {
            emit_status(svc, status);
        }
        persist_outcome(svc, &record.send_id);
    }
}

pub(super) struct Digest {
    pub(super) input: AgentInput,
    head: String,
    thread: String,
    nonce: String,
    epoch: u64,
    members: Vec<(UserSendRecord, String)>,
}

fn own_row(record: &UserSendRecord, text: &str) -> Option<ChatMsg> {
    match user_delivery::restore_input(record, text).ok()? {
        AgentInput::Framed(frame) => frame.chat_message(record.send_id.clone()),
        _ => Some(ChatMsg {
            id: record.send_id.clone(),
            kind: "user".into(),
            content: text.into(),
            ..Default::default()
        }),
    }
}

/// Folds the sends queued behind an already claimed head into one prompt, so a finished turn costs
/// one more turn however many messages it held. `None` when the head travels alone.
pub(super) fn begin_digest(
    svc: &ServiceContext,
    tab: &str,
    session: &Arc<Session>,
    head: &UserSendRecord,
    head_text: &str,
    context: &RuntimeContext,
) -> Option<Digest> {
    let thread = context.thread_id.clone()?;
    if !turn_requests::submit_owns_turn(session) || !foldable(head, head_text) {
        return None;
    }
    let store = &svc.pending_delivery_payloads.user_sends;
    let mut bytes = head_text.len();
    let mut candidates = Vec::new();
    for next in store
        .queued(tab)
        .ok()?
        .into_iter()
        .filter(|next| next.send_id != head.send_id)
    {
        svc.chat_send_ledger.restore_user_record(&next);
        if next.authority != head.authority
            || !store.eligible(&next)
            || svc
                .require_admitted_input_allowed(tab, Some(&next.send_id))
                .is_err()
        {
            break;
        }
        let Ok(text) = store.prepared(&next) else {
            break;
        };
        if !foldable(&next, &text) || bytes + text.len() > DIGEST_BUDGET_BYTES {
            break;
        }
        bytes += text.len();
        candidates.push((next, text));
    }
    if candidates.is_empty() {
        return None;
    }
    // Owner input ends an automatic retry, whichever transport it travels in.
    if (!is_framed(head) || candidates.iter().any(|(next, _)| !is_framed(next)))
        && super::capacity_retry::cancel(svc, tab).is_err()
    {
        return None;
    }
    let binding = SendLiveBinding {
        session_id: Some(thread.clone()),
        binding_nonce: Some(context.binding_nonce.clone()),
    };
    let mut members = Vec::with_capacity(candidates.len());
    for (next, text) in candidates {
        if !hand_off(svc, tab, &next, &text, &thread, &binding) {
            break;
        }
        members.push((next, text));
    }
    if members.is_empty() {
        return None;
    }
    let row = own_row(head, head_text);
    {
        let mut maps = session.maps.lock();
        if let Some(row) = row {
            if let Err(reason) = maps.remember_routed_row(&head.send_id, &thread, row) {
                log::warn!("[inbound] tab={tab} digest head row unavailable: {reason}");
            }
        }
        maps.remember_digest(
            &head.send_id,
            members.iter().map(|(m, _)| m.send_id.clone()).collect(),
        );
    }
    let mut entries = Vec::with_capacity(members.len() + 1);
    entries.push((head.clone(), head_text.to_owned()));
    entries.extend(members.iter().cloned());
    log::info!(
        "[inbound] tab={tab} head={} members={} folded into one digest",
        head.send_id,
        members.len()
    );
    Some(Digest {
        input: AgentInput::Framed(
            Messenger::from(MessageEnvelope::codeterm_reminder(DIGEST_CONTEXT))
                .framed_delivery(&section(&entries)),
        ),
        head: head.send_id.clone(),
        thread,
        nonce: context.binding_nonce.clone(),
        epoch: context.epoch,
        members,
    })
}

/// Members share the head's write: all return to the queue, or all are settled with its outcome.
pub(super) fn finish_digest(
    svc: &ServiceContext,
    tab: &str,
    session: &Arc<Session>,
    digest: Digest,
    zero_write: bool,
    outcome: Result<(), String>,
) {
    let store = &svc.pending_delivery_payloads.user_sends;
    if zero_write {
        session.maps.lock().take_digest(&digest.head);
        let binding = SendLiveBinding {
            session_id: Some(digest.thread.clone()),
            binding_nonce: Some(digest.nonce.clone()),
        };
        for (record, _) in &digest.members {
            release_handoff(svc, tab, store, record, &binding);
        }
        return;
    }
    let mut rows = Vec::with_capacity(digest.members.len());
    for (record, text) in &digest.members {
        retire_claim(svc, tab, record);
        let status = match &outcome {
            Ok(()) => svc.chat_send_ledger.mark_rpc_accepted(&record.send_id),
            Err(_) => svc.chat_send_ledger.mark_rpc_uncertain(&record.send_id),
        };
        if let Some(status) = status {
            emit_status(svc, status);
        }
        if let Some(status) = svc
            .chat_send_ledger
            .finish(&record.send_id, outcome.clone())
        {
            emit_status(svc, status);
        }
        persist_outcome(svc, &record.send_id);
        if let Some(item) = own_row(record, text) {
            rows.push(AgentRuntimeEvent::ChatItem {
                thread_id: digest.thread.clone(),
                item,
            });
        }
    }
    if let Err(reason) = crate::async_runtime::block_on(subscriber::publish_events(
        svc,
        session,
        digest.epoch,
        rows,
        now(),
    )) {
        log::warn!(
            "[inbound] tab={tab} head={} member rows unavailable: {reason}",
            digest.head
        );
    }
}

/// Exact evidence for the digest prompt is evidence for everything that travelled inside it.
pub(super) async fn confirm_digest_members(
    svc: &ServiceContext,
    session: &Arc<Session>,
    context: &RuntimeContext,
    head: &str,
) {
    let delivered = svc
        .chat_send_ledger
        .status(head)
        .is_some_and(|status| status.state == ChatSendState::Sent);
    let Some(thread) = context.thread_id.as_deref().filter(|_| delivered) else {
        return;
    };
    let members = session.maps.lock().take_digest(head);
    for member in members {
        if let Some(status) = svc.chat_send_ledger.confirm_rpc_send(
            &context.tab_id,
            thread,
            &context.binding_nonce,
            &member,
            None,
        ) {
            emit_status(svc, status);
        }
        let owner = svc.clone();
        if let Err(reason) =
            tokio::task::spawn_blocking(move || persist_outcome(&owner, &member)).await
        {
            log::error!("[inbound] digest member outcome task failed: {reason}");
        }
    }
}

#[cfg(test)]
mod tests;
