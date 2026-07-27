//! Generic, provider-agnostic tab-state reactor. A parked tab (`Idle`/`Waiting`)
//! is classified by its own provider; a verdict that clears [`GLOBAL_THRESHOLD`],
//! isn't [`TabState::WaitingOnAuth`], and carries an action is executed via key
//! injection / delayed-send / a user-facing surface. Core branches only on the
//! generic [`TabState`] / [`TabStateAction`], never a provider id or screen string.

use std::collections::HashSet;
use std::time::Duration;

use super::tab_state::{
    TabState, TabStateAction, TabStateClassification, TabStateSnapshot, Surface,
};
use super::status::TabStatus;
use crate::service::ServiceState;

/// Minimum confidence an actionable verdict must clear — the no-false-positive gate.
pub const GLOBAL_THRESHOLD: f32 = 0.8;

/// How often the reactor sweeps tracked tabs for the parked edge.
const POLL_INTERVAL: Duration = Duration::from_millis(1000);

/// Rows of screen tail handed to the classifier.
const SNAPSHOT_ROWS: usize = 60;

/// What the reactor decided. Pure output of [`decide`] so the gate/threshold/auth-guard are unit-testable.
#[derive(Debug, Clone, PartialEq)]
pub enum ReactorDecision {
    /// Do nothing (not parked, below threshold, no action, or unknown).
    Ignore,
    /// Present to the user only — never press a key (auth guard + surface-only verdicts).
    Surface(Option<Surface>),
    /// High-confidence, non-auth, actionable verdict — execute it.
    Act(TabStateAction),
}

/// Pure decision: gate on parked status, confidence threshold, and the auth guard. No side effects, no locks.
pub fn decide(
    status: TabStatus,
    classification: Option<&TabStateClassification>,
) -> ReactorDecision {
    // Gate: only act on a parked tab, never while it is Working/Dead.
    if !matches!(status, TabStatus::Idle | TabStatus::Waiting) {
        return ReactorDecision::Ignore;
    }
    let Some(c) = classification else {
        return ReactorDecision::Ignore;
    };
    // Threshold: a weak verdict never acts.
    if c.confidence < GLOBAL_THRESHOLD {
        return ReactorDecision::Ignore;
    }
    // Auth guard: a login prompt is never auto-advanced — surface only, even if keys were attached.
    if c.state == TabState::WaitingOnAuth {
        return ReactorDecision::Surface(c.action.as_ref().and_then(|a| a.surface.clone()));
    }
    match &c.action {
        Some(action) => ReactorDecision::Act(action.clone()),
        None => ReactorDecision::Ignore,
    }
}

/// Translate an action's `keys` into ordered PTY byte writes; unknown keys are skipped (never the wrong byte).
pub fn action_to_writes(action: &TabStateAction) -> Vec<Vec<u8>> {
    action
        .keys
        .as_ref()
        .map(|keys| {
            keys.iter()
                .filter_map(|k| super::keys::action_key_bytes(k))
                .collect()
        })
        .unwrap_or_default()
}

/// Start the background reactor loop on its own thread: sweeps tracked tabs,
/// dedupes per parked episode, and executes verdicts off the async runtime.
pub fn start(svc: ServiceState) {
    std::thread::spawn(move || {
        // Tabs already reacted-to this parked episode — prevents re-firing every tick at the same prompt.
        let mut reacted: HashSet<String> = HashSet::new();
        loop {
            std::thread::sleep(POLL_INTERVAL);
            // Snapshot ids + statuses under one lock acquisition per tick (not N+1).
            let snapshot: Vec<(String, TabStatus)> = {
                let tracker = svc.agent_tracker.lock();
                tracker
                    .all_ids()
                    .into_iter()
                    .map(|id| {
                        let status = tracker.get_status(&id);
                        (id, status)
                    })
                    .collect()
            };
            for (pty_id, status) in &snapshot {
                match status {
                    TabStatus::Idle | TabStatus::Waiting => {
                        if reacted.insert(pty_id.clone()) {
                            react(&svc, pty_id, *status);
                        }
                    }
                    _ => {
                        reacted.remove(pty_id);
                    }
                }
            }
            reacted.retain(|id| snapshot.iter().any(|(sid, _)| sid == id));
        }
    });
}

/// Classify one parked tab and execute the verdict. Locks in CLAUDE.md order, each
/// scoped, never held across the provider call or a thread spawn.
fn react(svc: &ServiceState, pty_id: &str, status: TabStatus) {
    // mux (1): resolve the tab's OWN identity + provider + session/cwd.
    let (tab_id, agent_type, session_id, cwd) = {
        let mux = svc.mux.read();
        let Some(tab_id) = mux.tab_id_for_pty(pty_id) else {
            return;
        };
        let Some(tab) = mux.get_tab(&tab_id) else {
            return;
        };
        let Some(agent_type) = tab.agent_type.clone() else {
            return; // not an agent tab — nothing to classify
        };
        (
            tab_id,
            agent_type,
            tab.agent_session_id.clone(),
            tab.cwd.clone(),
        )
    };

    // screen_state (3): the rendered tail the classifier fingerprints against.
    let screen_text = svc
        .screen_state
        .lock()
        .tail_text(pty_id, SNAPSHOT_ROWS)
        .unwrap_or_default();

    let snapshot = TabStateSnapshot {
        tab_id: tab_id.clone(),
        agent_type: agent_type.clone(),
        session_id,
        cwd,
        screen_text,
    };

    // Ask the tab's OWN provider. No lock held across this call.
    let classification: Option<TabStateClassification> = svc
        .agent_registry
        .with_provider(&agent_type, |p| p.classify_tab_state(&snapshot))
        .flatten();

    match decide(status, classification.as_ref()) {
        ReactorDecision::Ignore => {}
        ReactorDecision::Surface(surface) => surface_to_user(svc, &tab_id, surface),
        ReactorDecision::Act(action) => execute_action(svc, &tab_id, action),
    }
}

fn execute_action(svc: &ServiceState, tab_id: &str, action: TabStateAction) {
    let writes = action_to_writes(&action);
    if !writes.is_empty() {
        inject_keys(svc, tab_id, writes);
    }
    if let Some(resume) = &action.delayed_resume {
        schedule_resume(svc, tab_id, resume);
    }
    if let Some(surface) = action.surface {
        surface_to_user(svc, tab_id, Some(surface));
    }
}

/// Inject ordered key bytes into the tab's PTY (the `send --actions` path), off-thread so the poll loop never blocks.
fn inject_keys(svc: &ServiceState, tab_id: &str, writes: Vec<Vec<u8>>) {
    if svc.inject_raw_keys_async(tab_id, writes).is_err() {
        return;
    }
    log::info!("[state-reactor] injected keys into Tab {}", tab_id);
}

/// Schedule a resume via delayed-send at the parsed ISO time; an unparseable time is dropped, never fired at a guessed moment.
fn schedule_resume(svc: &ServiceState, tab_id: &str, resume: &super::tab_state::DelayedResume) {
    let Some(at_ms) = parse_iso_to_ms(&resume.at) else {
        log::warn!(
            "[state-reactor] unparseable resume time {:?} for Tab {} — skipping",
            resume.at,
            tab_id
        );
        return;
    };
    match crate::delayed_send::create_message(
        svc,
        tab_id.to_string(),
        resume.text.clone(),
        Some(at_ms),
        None,
    ) {
        Ok(_) => log::info!(
            "[state-reactor] scheduled resume for Tab {} at {}",
            tab_id,
            resume.at
        ),
        Err(e) => log::warn!("[state-reactor] schedule resume failed: {e}"),
    }
}

/// Surface a verdict to the user as a chat-card event.
fn surface_to_user(svc: &ServiceState, tab_id: &str, surface: Option<Surface>) {
    let (kind, payload) = match &surface {
        Some(s) => (s.kind.clone(), s.payload.clone()),
        None => ("tab_state".to_string(), serde_json::Value::Null),
    };
    crate::api::events::emit_everywhere(
        &svc.app_handle,
        "tab-state-surface",
        serde_json::json!({ "tab_id": tab_id, "kind": kind, "payload": payload }),
    );
    log::info!("[state-reactor] surfaced {} for Tab {}", kind, tab_id);
}

fn parse_iso_to_ms(iso: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(iso)
        .ok()
        .map(|dt| dt.timestamp_millis())
        .filter(|ms| *ms >= 0)
        .map(|ms| ms as u64)
}

#[cfg(test)]
mod tests;
