use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::cdp_wire::CdpEvent;

const DIALOG_STREAM_CAPACITY: usize = 64;

/// Transport-stamped JavaScript dialog opening, ordered with respect to outgoing CDP writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WireDialogOpening {
    pub generation: u64,
    pub dialog_type: String,
    pub message: String,
    /// Present only when this `beforeunload` arrived on the wire strictly after the transport
    /// writer wrote the matching navigation/close command and before any intervening invalidation.
    pub owner_token: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WireDialogEvent {
    Opening(WireDialogOpening),
    Closed,
}

#[derive(Default)]
struct SessionWireState {
    next_generation: u64,
    active: Option<WireDialogOpening>,
    armed_token: Option<u64>,
    subscribers: Vec<mpsc::Sender<WireDialogEvent>>,
}

/// Ordered transport-level causality for JavaScript dialogs per flattened CDP session.
/// Shared by the socket reader (`on_incoming_event`) and writer (`arm_*` / `can_accept` / `disarm`)
/// so dialog generations and causal ownership are determined strictly at the wire boundary.
#[derive(Default)]
pub struct WireDialogOrder {
    next_token: AtomicU64,
    sessions: Mutex<HashMap<String, SessionWireState>>,
}

impl WireDialogOrder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocates a non-zero causal token for one agent navigation or target close.
    pub fn allocate_token(&self) -> u64 {
        self.next_token.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn subscribe(&self, session_id: &str) -> mpsc::Receiver<WireDialogEvent> {
        let (tx, rx) = mpsc::channel(DIALOG_STREAM_CAPACITY);
        let mut sessions = self.sessions.lock();
        let entry = sessions.entry(session_id.to_string()).or_default();
        entry.subscribers.retain(|sender| !sender.is_closed());
        entry.subscribers.push(tx);
        rx
    }

    pub fn close_all(&self) {
        self.sessions.lock().clear();
    }

    pub fn has_active_dialog(&self, session_id: &str) -> bool {
        self.sessions
            .lock()
            .get(session_id)
            .is_some_and(|session| session.active.is_some())
    }

    pub fn active_dialog(&self, session_id: &str) -> Option<WireDialogOpening> {
        self.sessions
            .lock()
            .get(session_id)
            .and_then(|session| session.active.clone())
    }

    /// Arms `token` at the exact moment an agent navigation is written to the transport.
    /// Returns `false` (without arming) if any dialog is already open on the wire.
    pub fn arm_for_navigation(&self, session_id: &str, token: u64) -> bool {
        let mut sessions = self.sessions.lock();
        let session = sessions.entry(session_id.to_string()).or_default();
        if session.active.is_some() {
            return false;
        }
        session.armed_token = Some(token);
        true
    }

    /// Arms `token` for a target close only if no dialog is already open on the wire.
    pub fn arm_for_close(&self, session_id: &str, token: u64) {
        let mut sessions = self.sessions.lock();
        let session = sessions.entry(session_id.to_string()).or_default();
        if session.active.is_none() {
            session.armed_token = Some(token);
        }
    }

    /// Revokes causal ownership for `token` (or any owner when `token` is `None`).
    pub fn disarm(&self, session_id: &str, token: Option<u64>) {
        let mut sessions = self.sessions.lock();
        let Some(session) = sessions.get_mut(session_id) else {
            return;
        };
        if token.is_none() || session.armed_token == token {
            session.armed_token = None;
        }
        if let Some(active) = &mut session.active {
            if token.is_none() || active.owner_token == token {
                active.owner_token = None;
            }
        }
    }

    /// True iff `expected_generation` is STILL the currently open `beforeunload` dialog on the
    /// wire and is still causally owned by `expected_owner_token`.
    pub fn can_accept(
        &self,
        session_id: &str,
        expected_generation: u64,
        expected_owner_token: u64,
    ) -> bool {
        self.sessions.lock().get(session_id).is_some_and(|session| {
            session.active.as_ref().is_some_and(|active| {
                active.generation == expected_generation
                    && active
                        .dialog_type
                        .trim()
                        .eq_ignore_ascii_case("beforeunload")
                    && active.owner_token == Some(expected_owner_token)
            })
        })
    }

    /// Records incoming wire events synchronously in socket-reader order before subscriber fan-out.
    pub fn on_incoming_event(&self, event: &CdpEvent) {
        let Some(sid) = event.session_id.as_deref() else {
            return;
        };
        match event.method.as_str() {
            "Page.javascriptDialogOpening" => {
                let dialog_type = event
                    .params
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let message = event
                    .params
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let is_beforeunload = dialog_type.trim().eq_ignore_ascii_case("beforeunload");
                let mut sessions = self.sessions.lock();
                let session = sessions.entry(sid.to_string()).or_default();
                session.next_generation += 1;
                let generation = session.next_generation;
                // Consume the armed token one-shot so a later replacement beforeunload never inherits it.
                let owner_token = if is_beforeunload {
                    session.armed_token.take()
                } else {
                    session.armed_token = None;
                    None
                };
                let opening = WireDialogOpening {
                    generation,
                    dialog_type,
                    message,
                    owner_token,
                };
                session.active = Some(opening.clone());
                let wire_event = WireDialogEvent::Opening(opening);
                session.subscribers.retain(|sender| {
                    let _ = sender.try_send(wire_event.clone());
                    !sender.is_closed()
                });
            }
            "Page.javascriptDialogClosed" => {
                let mut sessions = self.sessions.lock();
                let session = sessions.entry(sid.to_string()).or_default();
                session.active = None;
                session.armed_token = None;
                session.subscribers.retain(|sender| {
                    let _ = sender.try_send(WireDialogEvent::Closed);
                    !sender.is_closed()
                });
            }
            "Page.frameNavigated" => {
                let is_main = event
                    .params
                    .get("frame")
                    .is_some_and(|frame| frame.get("parentId").is_none_or(Value::is_null));
                if is_main {
                    let mut sessions = self.sessions.lock();
                    if let Some(session) = sessions.get_mut(sid) {
                        session.armed_token = None;
                        if session.active.as_ref().is_some_and(|d| {
                            d.dialog_type.trim().eq_ignore_ascii_case("beforeunload")
                        }) {
                            session.active = None;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn disarms_navigation_on_write(method: &str) -> bool {
    method.starts_with("Input.")
        || matches!(
            method,
            "Page.navigate" | "Page.reload" | "Page.navigateToHistoryEntry"
        )
}
