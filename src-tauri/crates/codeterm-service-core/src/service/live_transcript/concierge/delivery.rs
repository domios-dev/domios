use std::{
    collections::{HashMap, VecDeque},
    fmt::Write as _,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Weak,
    },
};

use serde::{Deserialize, Serialize};

use super::RoutingState;
use crate::service::live_transcript::{
    BindingTarget, RoutedAction, TranscriptBinding, TranscriptContent, TranscriptRange,
    TranscriptRow, TranscriptStore, DEFAULT_TRANSCRIPT_TURN_LIMIT,
};
use crate::service::{
    agent_runtime::{engine, model::RuntimeFailure, restart, user_delivery},
    chat_send_transaction::ChatSendState,
    messenger::{MessageEnvelope, Messenger},
    wall_now_ms, ServiceContext,
};

const INLINE_CHARS: usize = 5000;

impl super::OwnerDispatch {
    pub(super) fn settle_route(&mut self, result: Result<(), String>) {
        self.last_route_error = match result {
            Ok(()) => None,
            Err(error) if is_retryable_route_error(&error) => return,
            Err(error) => Some(error),
        };
    }
}

fn is_retryable_route_error(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    [
        "busy",
        "requires an idle",
        "in progress",
        "awaiting confirmation",
        "already has a send record",
        "admission is held",
        "binding change is held",
    ]
    .iter()
    .any(|&reason| error.contains(reason))
}
const ATTACHED_SEND_PREFIX: &str = "runtime-envelope:live-transcript:";
const EXTERNAL_SEND_PREFIX: &str = "runtime-envelope:live-transcript-external:";

pub(in crate::service) fn is_transcript_delivery_token(token: &str) -> bool {
    token.starts_with(ATTACHED_SEND_PREFIX) || token.starts_with(EXTERNAL_SEND_PREFIX)
}

#[derive(Default)]
struct ExternalHandoffs(parking_lot::Mutex<HashMap<String, Weak<ExternalHandoff>>>);

pub(in crate::service) struct ExternalHandoff {
    owner: String,
    store: TranscriptStore,
    binding: TranscriptBinding,
    tab: String,
    session: String,
    nonce: String,
    state: AtomicU8,
}

struct ExternalPermit(Arc<ExternalHandoff>);

impl Drop for ExternalPermit {
    fn drop(&mut self) {
        self.0
            .state
            .fetch_or(ExternalHandoff::RETIRED, Ordering::AcqRel);
    }
}

impl ExternalHandoff {
    const CLAIMED: u8 = 1;
    const RETIRED: u8 = 2;

    pub(in crate::service) fn claim(&self, svc: &ServiceContext) -> Result<(), RuntimeFailure> {
        if self.state.load(Ordering::Acquire) & Self::CLAIMED != 0 {
            return Err(RuntimeFailure::WrittenAwaitingEvidence);
        }
        let mux = svc.mux.read();
        if mux.ensure_tab_membership_mutable(&self.owner).is_err()
            || mux.get_tab(&self.owner).is_none()
            || !mux.get_tab(&self.tab).is_some_and(|tab| {
                !tab.is_transcript_router()
                    && tab.agent_session_id.as_deref() == Some(self.session.as_str())
                    && tab.session_binding_nonce.as_deref() == Some(self.nonce.as_str())
            })
            || self
                .store
                .verify_live_binding(&self.owner, &self.binding)
                .is_err()
        {
            return Err(RuntimeFailure::Superseded);
        }
        self.state
            .compare_exchange(0, Self::CLAIMED, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|state| {
                if state & Self::CLAIMED != 0 {
                    RuntimeFailure::WrittenAwaitingEvidence
                } else {
                    RuntimeFailure::Superseded
                }
            })
    }
}

pub(in crate::service) fn external_handoff(
    svc: &ServiceContext,
    tab: &str,
    send_id: &str,
) -> Result<Option<Arc<ExternalHandoff>>, RuntimeFailure> {
    let is_external_route = svc.chat_send_ledger.status(send_id).is_some_and(|status| {
        status
            .correlation_id
            .as_deref()
            .is_some_and(|token| token.starts_with(EXTERNAL_SEND_PREFIX))
    });
    if !is_external_route {
        return Ok(None);
    }
    svc.ext_or_init(ExternalHandoffs::default)
        .0
        .lock()
        .get(send_id)
        .and_then(Weak::upgrade)
        .filter(|authority| authority.tab == tab)
        .map(Some)
        .ok_or(RuntimeFailure::Superseded)
}

#[derive(Deserialize, Serialize)]
pub(super) struct PendingAction {
    binding: TranscriptBinding,
    action: RoutedAction,
}

pub(super) fn settle(
    svc: &ServiceContext,
    store: &TranscriptStore,
    state: &mut RoutingState,
) -> Result<(), String> {
    let Some(pending) = &state.pending else {
        return Ok(());
    };
    let action = &pending.action;
    svc.restore_durable_user_statuses(&action.destination)?;
    let Some(status) = svc.chat_send_ledger.status(&action.action_id) else {
        return Ok(());
    };
    if status.tab_id != action.destination {
        return Err("Transcript send receipt identity mismatch".into());
    }
    match status.state {
        ChatSendState::Sent => {
            if store
                .routed_action(&action.action_id)
                .map_err(|error| error.to_string())?
                .is_none()
            {
                let mut proven = action.clone();
                proven.at_ms = wall_now_ms();
                store
                    .record_action(&pending.binding, proven)
                    .map_err(|error| error.to_string())?;
            }
        }
        ChatSendState::Failed => {
            if state.attempted_through == action.range.to_seq {
                state.attempted_through = action.range.from_seq.saturating_sub(1);
            }
            if let Some(session) = engine::for_tab(&action.destination) {
                super::wake_when_idle(
                    svc,
                    store.clone(),
                    &pending.binding.owner_tab_id,
                    action.destination.clone(),
                    session,
                );
            }
        }
        _ => return Ok(()),
    }
    state.pending = None;
    store.save_routing_state(state)
}

pub(super) fn route(
    svc: &ServiceContext,
    store: &TranscriptStore,
    owner: &str,
    observed: &TranscriptBinding,
    range: TranscriptRange,
    rationale: &str,
    state: &mut RoutingState,
) -> Result<(), String> {
    store
        .verify_live_binding(owner, observed)
        .map_err(|error| error.to_string())?;
    let (mut attached, external_gate, binding, destination) = match &observed.target {
        BindingTarget::External { tab_id } => {
            let gate = svc
                .ext_or_init(super::super::TranscriptOwners::default)
                .gate(&observed.owner_tab_id)
                .try_lock_owned()
                .map_err(|_| "Transcript binding change is held")?;
            store
                .verify_live_binding(owner, observed)
                .map_err(|error| error.to_string())?;
            (None, Some(gate), observed.clone(), tab_id.clone())
        }
        _ => {
            let permit =
                svc.admit_attached_transcript_turn(owner, DEFAULT_TRANSCRIPT_TURN_LIMIT)?;
            (Some(permit), None, observed.clone(), String::new())
        }
    };
    let (binding, destination) = attached.as_ref().map_or((binding, destination), |permit| {
        (
            permit.binding().clone(),
            permit.epoch().runtime_tab_id.clone(),
        )
    });
    let session = engine::for_tab(&destination)
        .ok_or("Transcript router needs a ready structured runtime")?;
    if !svc
        .mux
        .read()
        .get_tab(&destination)
        .is_some_and(engine::has_live_bound_session)
        || restart::model_restart_busy(&session)
    {
        return Err("Transcript router is busy or unavailable".into());
    }
    let turn_active = session.state.lock().active_turn_started_at_ms.is_some();
    if session.gate.available_permits() == 0 || turn_active {
        super::wake_when_idle(svc, store.clone(), owner, destination.clone(), session);
        return Err("Transcript router is busy or unavailable".into());
    }
    let cursor = store.cursor(&binding).map_err(|error| error.to_string())?;
    if range.from_seq <= cursor.acked_seq {
        return Err("Transcript range precedes the active cursor".into());
    }
    let body = route_body(
        store,
        owner,
        &binding,
        range,
        attached.is_some(),
        cursor.acked_seq,
    );
    let frame = Messenger::from(MessageEnvelope::from_codeterm()).framed_delivery(&body);
    let input = frame.to_message();
    let prefix = if attached.is_some() {
        ATTACHED_SEND_PREFIX
    } else {
        EXTERNAL_SEND_PREFIX
    };
    let token = format!(
        "{prefix}{}:{}:{}",
        binding.owner_tab_id, range.from_seq, range.to_seq
    );
    let _update_input = svc.admit_update_input(&destination)?;
    let reservation = svc.reserve_user_send(&destination, Some(token))?;
    if reservation.wait_existing()? {
        return Err("This transcript range already has a send record".into());
    }
    let send_id = reservation.send_id.clone();
    let external_permit = if let Some(permit) = &mut attached {
        let epoch = permit.epoch();
        if reservation.authority.session_id.as_deref() != Some(epoch.session_id.as_str())
            || reservation.authority.binding_nonce != epoch.binding_nonce
        {
            return Err("Transcript router recipient became stale".into());
        }
        permit.authorize_handoff(svc, &send_id)?;
        None
    } else {
        let authority = Arc::new(ExternalHandoff {
            owner: owner.into(),
            store: store.clone(),
            binding: binding.clone(),
            tab: destination.clone(),
            session: reservation
                .authority
                .session_id
                .clone()
                .ok_or("External transcript runtime requires a bound session")?,
            nonce: reservation.authority.binding_nonce.clone(),
            state: AtomicU8::new(0),
        });
        let mut entries = svc.ext_or_init(ExternalHandoffs::default).0.lock();
        entries.retain(|_, authority| authority.strong_count() > 0);
        entries.insert(send_id.clone(), Arc::downgrade(&authority));
        Some(ExternalPermit(authority))
    };
    state.attempted_through = range.to_seq;
    state.pending = Some(PendingAction {
        binding,
        action: RoutedAction {
            action_id: send_id.clone(),
            range,
            at_ms: wall_now_ms(),
            destination: destination.clone(),
            rationale: rationale.into(),
        },
    });
    store.save_routing_state(state)?;
    let admission = svc
        .pending_delivery_payloads
        .user_sends
        .admit_runtime_envelope(&reservation, &input, None, frame.provenance());
    let record = match admission {
        Ok(record) => record,
        Err(_) => return Err("Transcript delivery admission outcome is unavailable".into()),
    };
    svc.chat_send_ledger.admit_user(&record);
    svc.emit_live_transcript_changed(owner);
    super::watch_delivery(svc, store.clone(), owner, &send_id);
    drop(reservation);
    user_delivery::drain(svc, &destination)?;
    settle(svc, store, state)?;
    drop((attached, external_permit, external_gate));
    Ok(())
}

pub(super) fn route_body(
    store: &TranscriptStore,
    owner: &str,
    binding: &TranscriptBinding,
    range: TranscriptRange,
    allow_inline: bool,
    acked_seq: u64,
) -> String {
    let pointer = format!(
        "Live transcript {}–{} (owner tab: {owner}, revision: {}). Ack after handling: domios live-transcript ack --tab {owner} --revision {} --through-seq {}. If not inline, read with: domios live-transcript read --tab {owner} --revision {} --from-seq {} --through-seq {}.",
        range.from_seq, range.to_seq, binding.revision, binding.revision, range.to_seq, binding.revision, range.from_seq, range.to_seq,
    );
    if allow_inline {
        let budget = INLINE_CHARS.saturating_sub(pointer.chars().count() + 1);
        let row_count = range.to_seq.saturating_sub(range.from_seq) + 1;
        if row_count <= 16 {
            if let Ok(page) = store.history(range.from_seq, row_count as usize) {
                if page.gaps.is_empty()
                    && page.rows.last().is_some_and(|row| row.seq >= range.to_seq)
                {
                    if let Some(inline) = inline_speech(store, range, &page.rows, budget) {
                        return format!("{pointer}\n{inline}");
                    }
                }
            }
        }
        if acked_seq.checked_add(1) == Some(range.from_seq) {
            if let Ok(page) = store.read_page(binding, None, budget, None) {
                if page.next.is_none() && page.gaps.is_empty() && page.through_seq == range.to_seq {
                    let text = escape_inline(&page.text);
                    if text.chars().count() <= budget {
                        return format!("{pointer}\n{text}");
                    }
                }
            }
        }
    }
    pointer
}

fn inline_speech(
    store: &TranscriptStore,
    range: TranscriptRange,
    rows: &[TranscriptRow],
    budget: usize,
) -> Option<String> {
    let mut unread = String::from("[UNREAD]");
    for row in rows {
        if let TranscriptContent::Speech { text } = &row.content {
            let _ = write!(unread, "\n{} {}", row.seq, escape_inline(text));
        }
    }
    if unread == "[UNREAD]" || unread.chars().count() > budget {
        return None;
    }

    let mut remaining = budget.saturating_sub(unread.chars().count() + "[CONTEXT]\n".len());
    let mut context = VecDeque::new();
    if let Ok(recent) = store.recent_context(range.from_seq.saturating_sub(1)) {
        for row in recent.into_iter().rev() {
            if let TranscriptContent::Speech { text } = row.content {
                let line = format!("\n{} {}", row.seq, escape_inline(&text));
                let Some(next) = remaining.checked_sub(line.chars().count()) else {
                    break;
                };
                remaining = next;
                context.push_front(line);
            }
        }
    }
    if context.is_empty() {
        return Some(unread);
    }
    let mut inline = String::from("[CONTEXT]");
    for line in context {
        inline.push_str(&line);
    }
    let _ = write!(inline, "\n{unread}");
    Some(inline)
}

fn escape_inline(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
