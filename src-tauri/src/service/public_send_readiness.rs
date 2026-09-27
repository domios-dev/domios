use crate::multiplexer::types::Tab;
use crate::service::chat_send_transaction::{EvidenceWindow, SendEvidenceAnchor};
use crate::service::launch_readiness::LaunchAttempt;
use crate::service::ServiceContext;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::{Condvar, Mutex};
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

#[cfg(test)]
#[path = "public_send_readiness/tests.rs"]
mod tests;

struct LaunchSendGate {
    key: PublicSendGateKey,
    attempt: Option<LaunchAttempt>,
    expected_turn: String,
    anchor: Option<SendEvidenceAnchor>,
    bound_session: Option<String>,
    watcher: Option<RecommendedWatcher>,
    deadline: Option<Instant>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PublicSendGateKey(uuid::Uuid);

#[derive(Default)]
struct ReadinessState {
    gates: HashMap<String, LaunchSendGate>,
    generation: u64,
}

#[derive(Default)]
struct ReadinessRegistry {
    state: Mutex<ReadinessState>,
    changed: Condvar,
}

static READINESS: LazyLock<ReadinessRegistry> = LazyLock::new(ReadinessRegistry::default);

pub(crate) struct PublicSendLaunchGuard {
    svc: ServiceContext,
    tab_id: String,
    key: Option<PublicSendGateKey>,
    completed: bool,
}

impl PublicSendLaunchGuard {
    pub(crate) fn complete(mut self) {
        if let Some(key) = self.key {
            self.svc.complete_public_send_launch(&self.tab_id, key);
        }
        self.completed = true;
    }
}

impl Drop for PublicSendLaunchGuard {
    fn drop(&mut self) {
        if !self.completed {
            if let Some(key) = self.key {
                self.svc.retire_public_send_launch_key(&self.tab_id, key);
            }
        }
    }
}

fn dispose_gate(gate: Option<LaunchSendGate>) {
    if let Some(mut gate) = gate {
        if let Some(watcher) = gate.watcher.take() {
            std::thread::spawn(move || drop(watcher));
        }
    }
}

fn gate_tab_matches(tab: &Tab, gate: &LaunchSendGate) -> bool {
    gate.attempt.as_ref().is_some_and(|attempt| {
        tab.pty_id.as_deref() == Some(attempt.pty_id.as_str())
            && tab.session_binding_nonce.as_deref() == Some(attempt.binding_nonce.as_str())
    })
}

impl ReadinessRegistry {
    fn arm(
        &self,
        tab_id: String,
        expected_turn: Option<String>,
        anchor: Option<SendEvidenceAnchor>,
    ) -> Option<PublicSendGateKey> {
        let expected_turn = expected_turn.filter(|text| !text.trim().is_empty());
        let mut state = self.state.lock();
        let key = expected_turn
            .as_ref()
            .map(|_| PublicSendGateKey(uuid::Uuid::new_v4()));
        let previous = match (expected_turn, key) {
            (Some(expected_turn), Some(key)) => state.gates.insert(
                tab_id.clone(),
                LaunchSendGate {
                    key,
                    attempt: None,
                    expected_turn,
                    anchor,
                    bound_session: None,
                    watcher: None,
                    deadline: None,
                },
            ),
            _ => state.gates.remove(&tab_id),
        };
        state.generation = state.generation.wrapping_add(1);
        self.changed.notify_all();
        drop(state);
        dispose_gate(previous);
        key
    }

    fn retire_key(&self, tab_id: &str, key: PublicSendGateKey) {
        let mut state = self.state.lock();
        let matches_key = state.gates.get(tab_id).is_some_and(|gate| gate.key == key);
        let removed = matches_key.then(|| state.gates.remove(tab_id)).flatten();
        if removed.is_some() {
            state.generation = state.generation.wrapping_add(1);
            self.changed.notify_all();
        }
        drop(state);
        dispose_gate(removed);
    }

    fn retire_attempt(&self, attempt: &LaunchAttempt) {
        let mut state = self.state.lock();
        let matches_attempt = state
            .gates
            .get(&attempt.tab_id)
            .and_then(|gate| gate.attempt.as_ref())
            == Some(attempt);
        let removed = matches_attempt
            .then(|| state.gates.remove(&attempt.tab_id))
            .flatten();
        if removed.is_some() {
            state.generation = state.generation.wrapping_add(1);
            self.changed.notify_all();
        }
        drop(state);
        dispose_gate(removed);
    }

    fn retire(&self, tab_id: &str) {
        let mut state = self.state.lock();
        let removed = state.gates.remove(tab_id);
        if removed.is_some() {
            state.generation = state.generation.wrapping_add(1);
            self.changed.notify_all();
        }
        drop(state);
        dispose_gate(removed);
    }

    fn bind_attempt(&self, attempt: LaunchAttempt, key: Option<PublicSendGateKey>) {
        let Some(key) = key else { return };
        let mut state = self.state.lock();
        if let Some(gate) = state.gates.get_mut(&attempt.tab_id) {
            if gate.key == key {
                gate.attempt = Some(attempt);
                state.generation = state.generation.wrapping_add(1);
                self.changed.notify_all();
            }
        }
    }

    fn complete(&self, tab_id: &str, key: PublicSendGateKey) {
        let tab_id = tab_id.to_string();
        let mut state = self.state.lock();
        let Some(gate) = state.gates.get_mut(&tab_id) else {
            return;
        };
        if gate.key != key {
            return;
        }
        let Some(attempt) = gate.attempt.clone() else {
            return;
        };
        let deadline = *gate
            .deadline
            .get_or_insert_with(|| Instant::now() + Duration::from_secs(30));
        state.generation = state.generation.wrapping_add(1);
        self.changed.notify_all();
        drop(state);
        std::thread::spawn(move || {
            let mut state = READINESS.state.lock();
            loop {
                let Some(gate) = state.gates.get(&tab_id) else {
                    return;
                };
                if gate.key != key
                    || gate.attempt.as_ref() != Some(&attempt)
                    || gate.deadline != Some(deadline)
                {
                    return;
                }
                let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                    let removed = state.gates.remove(&tab_id);
                    state.generation = state.generation.wrapping_add(1);
                    READINESS.changed.notify_all();
                    drop(state);
                    dispose_gate(removed);
                    return;
                };
                if READINESS
                    .changed
                    .wait_for(&mut state, remaining)
                    .timed_out()
                {
                    if state.gates.get(&tab_id).is_some_and(|gate| {
                        gate.key == key
                            && gate.attempt.as_ref() == Some(&attempt)
                            && gate.deadline == Some(deadline)
                    }) {
                        let removed = state.gates.remove(&tab_id);
                        state.generation = state.generation.wrapping_add(1);
                        READINESS.changed.notify_all();
                        drop(state);
                        dispose_gate(removed);
                    }
                    return;
                }
            }
        });
    }

    fn retire_changed_session(&self, mux: &crate::MuxState, tab_id: &str, event_nonce: &str) {
        let current = mux.read().get_tab(tab_id).map(|tab| {
            (
                tab.pty_id.clone(),
                tab.session_binding_nonce.clone(),
                tab.agent_session_id.clone(),
            )
        });
        let mut state = self.state.lock();
        let Some(gate) = state.gates.get(tab_id) else {
            return;
        };
        let changed = current.is_some_and(|(pty_id, nonce, session_id)| {
            if nonce.as_deref() != Some(event_nonce) {
                return false;
            }
            gate.attempt.as_ref().is_some_and(|attempt| {
                pty_id.as_deref() != Some(attempt.pty_id.as_str())
                    || nonce.as_deref() != Some(attempt.binding_nonce.as_str())
            }) || gate
                .bound_session
                .as_ref()
                .is_some_and(|bound| session_id.as_ref() != Some(bound))
        });
        if changed {
            let removed = state.gates.remove(tab_id);
            state.generation = state.generation.wrapping_add(1);
            self.changed.notify_all();
            drop(state);
            dispose_gate(removed);
        }
    }

    fn confirm(
        &self,
        mux: &crate::MuxState,
        tab_id: &str,
        session_id: &str,
        nonce: &str,
        key: PublicSendGateKey,
    ) {
        let authenticated = mux.read().get_tab(tab_id).is_some_and(|tab| {
            tab.launch_session_authenticated()
                && tab.agent_session_id.as_deref() == Some(session_id)
                && tab.session_binding_nonce.as_deref() == Some(nonce)
                && tab.reported_session_binding_nonce.as_deref() == Some(nonce)
        });
        if !authenticated {
            return;
        }
        let mut state = self.state.lock();
        let Some(gate) = state.gates.get(tab_id) else {
            return;
        };
        if gate.key != key
            || gate
                .attempt
                .as_ref()
                .is_none_or(|attempt| attempt.binding_nonce != nonce)
            || gate.bound_session.as_deref() != Some(session_id)
            || gate
                .deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return;
        }
        let removed = state.gates.remove(tab_id);
        state.generation = state.generation.wrapping_add(1);
        self.changed.notify_all();
        drop(state);
        dispose_gate(removed);
        log::info!(
            "[public-send-readiness] event=LaunchBootstrapTurnConfirmed tab={tab_id} session={session_id}"
        );
    }

    fn inspect(&self, mux: &crate::MuxState, tab_id: &str) -> Option<Result<(), String>> {
        let tab = mux.read().get_tab(tab_id).cloned()?;
        let mut state = self.state.lock();
        let Some(gate) = state.gates.get(tab_id) else {
            return Some(Ok(()));
        };
        if gate.attempt.is_some()
            && (!gate_tab_matches(&tab, gate)
                || gate
                    .bound_session
                    .as_ref()
                    .is_some_and(|session| tab.agent_session_id.as_ref() != Some(session))
                || gate
                    .deadline
                    .is_some_and(|deadline| Instant::now() >= deadline))
        {
            let removed = state.gates.remove(tab_id);
            state.generation = state.generation.wrapping_add(1);
            self.changed.notify_all();
            drop(state);
            dispose_gate(removed);
            return Some(Ok(()));
        }
        if tab.launch_session_authenticated()
            && gate.bound_session.as_deref() == tab.agent_session_id.as_deref()
            && gate.bound_session.is_some()
        {
            return None;
        }
        None
    }

    fn await_ready(
        &self,
        mux: &crate::MuxState,
        tab_id: &str,
        timeout: Duration,
    ) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        let mut state = self.state.lock();
        loop {
            let generation = state.generation;
            drop(state);
            if let Some(observed) = self.inspect(mux, tab_id) {
                return observed;
            }
            state = self.state.lock();
            if state.generation != generation {
                continue;
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                drop(state);
                return Err(format!(
                    "Tab {tab_id} public input readiness deadline elapsed; no input was written"
                ));
            };
            if self.changed.wait_for(&mut state, remaining).timed_out() {
                drop(state);
                if let Some(observed) = self.inspect(mux, tab_id) {
                    return observed;
                }
                return Err(format!(
                    "Tab {tab_id} public input readiness deadline elapsed; no input was written"
                ));
            }
        }
    }

    fn expected_turn(
        &self,
        tab_id: &str,
        nonce: &str,
    ) -> Option<(String, Option<SendEvidenceAnchor>, PublicSendGateKey)> {
        self.state.lock().gates.get(tab_id).and_then(|gate| {
            (gate
                .attempt
                .as_ref()
                .is_some_and(|attempt| attempt.binding_nonce == nonce)
                && gate
                    .deadline
                    .is_none_or(|deadline| Instant::now() < deadline))
            .then(|| (gate.expected_turn.clone(), gate.anchor.clone(), gate.key))
        })
    }

    fn bound(&self, tab_id: &str, nonce: &str, session_id: &str) -> bool {
        let mut state = self.state.lock();
        let Some(gate) = state.gates.get_mut(tab_id) else {
            return false;
        };
        if gate
            .attempt
            .as_ref()
            .is_none_or(|attempt| attempt.binding_nonce != nonce)
            || gate
                .deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return false;
        }
        if gate
            .bound_session
            .as_deref()
            .is_some_and(|previous| previous != session_id)
        {
            let removed = state.gates.remove(tab_id);
            state.generation = state.generation.wrapping_add(1);
            self.changed.notify_all();
            drop(state);
            dispose_gate(removed);
            return false;
        }
        gate.bound_session = Some(session_id.to_string());
        state.generation = state.generation.wrapping_add(1);
        self.changed.notify_all();
        true
    }

    fn set_watcher(
        &self,
        tab_id: &str,
        nonce: &str,
        key: PublicSendGateKey,
        watcher: RecommendedWatcher,
    ) {
        let mut state = self.state.lock();
        let Some(gate) = state.gates.get_mut(tab_id) else {
            return;
        };
        let previous = if gate.key == key
            && gate
                .attempt
                .as_ref()
                .is_some_and(|attempt| attempt.binding_nonce == nonce)
        {
            gate.watcher.replace(watcher)
        } else {
            Some(watcher)
        };
        drop(state);
        if let Some(watcher) = previous {
            std::thread::spawn(move || drop(watcher));
        }
    }

    fn expected_text(
        &self,
        tab_id: &str,
        nonce: &str,
        session_id: &str,
    ) -> Option<(String, PublicSendGateKey)> {
        self.state.lock().gates.get(tab_id).and_then(|gate| {
            (gate
                .attempt
                .as_ref()
                .is_some_and(|attempt| attempt.binding_nonce == nonce)
                && gate
                    .deadline
                    .is_none_or(|deadline| Instant::now() < deadline)
                && gate.bound_session.as_deref() == Some(session_id))
            .then(|| (gate.expected_turn.clone(), gate.key))
        })
    }

    fn note_turn(
        &self,
        mux: &crate::MuxState,
        tab_id: &str,
        session_id: &str,
        binding_nonce: &str,
        messages: &[crate::agent::chat_types::ChatMsg],
    ) -> bool {
        let Some((expected, key)) = self.expected_text(tab_id, binding_nonce, session_id) else {
            return false;
        };
        if !messages.iter().any(|message| {
            message.kind == "user"
                && crate::service::chat_send_transaction::normalize_turn_evidence_with(
                    &message.content,
                    Default::default(),
                ) == crate::service::chat_send_transaction::normalize_turn_evidence_with(
                    &expected,
                    Default::default(),
                )
        }) {
            return false;
        }
        self.confirm(mux, tab_id, session_id, binding_nonce, key);
        true
    }
}

pub(crate) fn observe_session_binding(
    svc: &ServiceContext,
    tab_id: &str,
    event_session_id: &str,
    event_nonce: &str,
) {
    let authenticated_event_matches = svc.mux.read().get_tab(tab_id).is_some_and(|tab| {
        tab.launch_session_authenticated()
            && tab.agent_session_id.as_deref() == Some(event_session_id)
            && tab.session_binding_nonce.as_deref() == Some(event_nonce)
            && tab.reported_session_binding_nonce.as_deref() == Some(event_nonce)
    });
    if !authenticated_event_matches {
        return;
    }
    READINESS.retire_changed_session(&svc.mux, tab_id, event_nonce);
    let bound = svc.mux.read().get_tab(tab_id).and_then(|tab| {
        (tab.launch_session_authenticated()).then(|| {
            (
                tab.agent_type.clone(),
                tab.cwd.clone().unwrap_or_default(),
                tab.agent_session_id.clone(),
                tab.session_binding_nonce.clone(),
            )
        })
    });
    let Some((Some(provider_id), cwd, Some(session_id), Some(nonce))) = bound else {
        return;
    };
    if session_id != event_session_id || nonce != event_nonce {
        return;
    }
    let Some((expected_turn, previous_anchor, gate_key)) = READINESS.expected_turn(tab_id, &nonce)
    else {
        return;
    };
    if !READINESS.bound(tab_id, &nonce, &session_id) {
        return;
    }
    let structured = svc
        .agent_registry
        .with_provider(&provider_id, |provider| provider.uses_structured_chat())
        .unwrap_or(false);
    let turn_normalization = svc
        .agent_registry
        .with_provider(&provider_id, |provider| provider.turn_evidence_normalization())
        .unwrap_or_default();
    let anchor = previous_anchor
        .filter(|anchor| anchor.session_id == session_id)
        .or_else(|| {
            Some(SendEvidenceAnchor {
                provider_id: provider_id.clone(),
                cwd: cwd.clone(),
                session_id: session_id.clone(),
                turn_normalization,
                window: if structured {
                    EvidenceWindow::Structured { cursor: None }
                } else {
                    EvidenceWindow::Jsonl {
                        transcript_offset: 0,
                    }
                },
            })
        });
    let Some(anchor) = anchor else { return };
    let evidence_path = svc
        .agent_registry
        .with_provider(&provider_id, |provider| {
            provider
                .session_jsonl_path(&cwd, &session_id)
                .or_else(|| provider.session_file_path(&cwd, &session_id))
        })
        .flatten();
    if let Some(path) = evidence_path {
        if let Some(parent) = path.parent().map(std::path::Path::to_path_buf) {
            let file_name = path.file_name().map(std::ffi::OsStr::to_os_string);
            let registry = svc.agent_registry.clone();
            let mux = svc.mux.clone();
            let tab = tab_id.to_string();
            let session = session_id.clone();
            let binding_nonce = nonce.clone();
            let watch_anchor = anchor.clone();
            let watch_key = gate_key;
            let mut cursor = crate::service::chat_send_evidence::ExactTurnCursor::for_anchor(Some(
                &watch_anchor,
            ));
            if let Ok(mut watcher) =
                notify::recommended_watcher(move |event: notify::Result<Event>| {
                    let Ok(event) = event else { return };
                    if !event.paths.iter().any(|changed| {
                        changed.parent() == Some(parent.as_path())
                            && changed.file_name() == file_name.as_deref()
                    }) {
                        return;
                    }
                    let Some((expected, current_key)) =
                        READINESS.expected_text(&tab, &binding_nonce, &session)
                    else {
                        return;
                    };
                    if current_key != watch_key {
                        return;
                    }
                    if crate::service::chat_send_evidence::contains_exact_user_turn_since(
                        &registry,
                        &watch_anchor,
                        &expected,
                        &mut cursor,
                    ) {
                        READINESS.confirm(&mux, &tab, &session, &binding_nonce, watch_key);
                    }
                })
            {
                if watcher
                    .watch(path.parent().unwrap(), RecursiveMode::NonRecursive)
                    .is_ok()
                {
                    READINESS.set_watcher(tab_id, &nonce, gate_key, watcher);
                }
            }
        }
    }
    if crate::service::chat_send_evidence::contains_exact_user_turn_since(
        &svc.agent_registry,
        &anchor,
        &expected_turn,
        &mut crate::service::chat_send_evidence::ExactTurnCursor::for_anchor(Some(&anchor)),
    ) {
        READINESS.confirm(&svc.mux, tab_id, &session_id, &nonce, gate_key);
    }
}

pub(crate) fn note_bootstrap_turn(
    mux: &crate::MuxState,
    tab_id: &str,
    session_id: &str,
    binding_nonce: &str,
    messages: &[crate::agent::chat_types::ChatMsg],
) -> bool {
    READINESS.note_turn(mux, tab_id, session_id, binding_nonce, messages)
}

impl ServiceContext {
    pub(crate) fn public_send_launch_guard_for_key(
        &self,
        tab_id: &str,
        key: Option<PublicSendGateKey>,
    ) -> PublicSendLaunchGuard {
        PublicSendLaunchGuard {
            svc: self.clone(),
            tab_id: tab_id.to_string(),
            key,
            completed: false,
        }
    }

    pub(crate) fn arm_public_send_launch(
        &self,
        tab_id: &str,
        expected_turn: Option<String>,
        anchor: Option<SendEvidenceAnchor>,
    ) -> Option<PublicSendGateKey> {
        READINESS.arm(tab_id.to_string(), expected_turn, anchor)
    }

    pub(crate) fn bind_public_send_launch_attempt(
        &self,
        attempt: LaunchAttempt,
        key: Option<PublicSendGateKey>,
    ) {
        READINESS.bind_attempt(attempt, key);
    }

    pub(crate) fn has_public_send_launch_gate(&self, tab_id: &str) -> bool {
        READINESS.state.lock().gates.contains_key(tab_id)
    }

    pub(crate) fn complete_public_send_launch(&self, tab_id: &str, key: PublicSendGateKey) {
        READINESS.complete(tab_id, key);
    }

    pub(crate) fn await_public_send_readiness(
        &self,
        tab_id: &str,
        timeout: Duration,
    ) -> Result<(), String> {
        READINESS.await_ready(&self.mux, tab_id, timeout)
    }

    pub(crate) fn retire_public_send_launch(&self, tab_id: &str) {
        READINESS.retire(tab_id);
    }

    pub(crate) fn retire_public_send_launch_key(&self, tab_id: &str, key: PublicSendGateKey) {
        READINESS.retire_key(tab_id, key);
    }

    pub(crate) fn retire_public_send_launch_attempt(&self, attempt: &LaunchAttempt) {
        READINESS.retire_attempt(attempt);
    }
}
