//! Generic, provider-agnostic pane-state reactor (AS1).
//!
//! Gated on `PaneStatusTracker`: a pane is classified ONLY when it is parked
//! (`Idle` / `Waiting`), never while it streams — so the hot path pays nothing.
//! On that edge core asks the pane's OWN provider to `classify_pane_state`; if
//! the verdict clears [`GLOBAL_THRESHOLD`], is not [`PaneState::WaitingOnAuth`]
//! (the auth guard), and carries an action, core executes it through existing
//! seams: key injection (the `send --actions` path), the T7 delayed-send, and a
//! user-facing surface (chat card + notification).
//!
//! Core branches ONLY on the generic [`PaneState`] / [`PaneStateAction`] — never
//! on a provider id or a literal screen string.

use std::collections::HashSet;
use std::time::Duration;

use super::pane_state::{
    PaneState, PaneStateAction, PaneStateClassification, PaneStateSnapshot, Surface,
};
use super::status::PaneStatus;
use crate::service::ServiceState;

/// Minimum confidence an actionable verdict must clear. A single weak signal
/// (low confidence) yields no action — the no-false-positive gate.
pub const GLOBAL_THRESHOLD: f32 = 0.8;

/// How often the reactor sweeps tracked panes for the parked edge.
const POLL_INTERVAL: Duration = Duration::from_millis(1000);

/// Rows of screen tail handed to the classifier.
const SNAPSHOT_ROWS: usize = 60;

/// What the reactor decided to do with a classification. Pure output of
/// [`decide`] so the gate + threshold + auth-guard are unit-testable without a
/// live `ServiceContext`.
#[derive(Debug, Clone, PartialEq)]
pub enum ReactorDecision {
    /// Do nothing (not parked, below threshold, no action, or unknown).
    Ignore,
    /// Present to the user only — NEVER press a key. Used for the auth guard
    /// and for surface-only verdicts.
    Surface(Option<Surface>),
    /// High-confidence, non-auth, actionable verdict — execute it.
    Act(PaneStateAction),
}

/// Pure decision: gate on parked status, the confidence threshold, and the
/// auth guard. No side effects, no locks — the heart of the reactor's safety.
pub fn decide(
    status: PaneStatus,
    classification: Option<&PaneStateClassification>,
) -> ReactorDecision {
    // Gate: only act on a parked pane, never while it is Working/Dead.
    if !matches!(status, PaneStatus::Idle | PaneStatus::Waiting) {
        return ReactorDecision::Ignore;
    }
    let Some(c) = classification else {
        return ReactorDecision::Ignore;
    };
    // Threshold: a weak verdict never acts.
    if c.confidence < GLOBAL_THRESHOLD {
        return ReactorDecision::Ignore;
    }
    // Auth guard: an auth/login prompt is NEVER auto-advanced — surface only,
    // even if the provider attached keys.
    if c.state == PaneState::WaitingOnAuth {
        return ReactorDecision::Surface(c.action.as_ref().and_then(|a| a.surface.clone()));
    }
    match &c.action {
        Some(action) => ReactorDecision::Act(action.clone()),
        None => ReactorDecision::Ignore,
    }
}

/// Translate an action's `keys` into the ordered PTY byte writes used by the
/// `send --actions` path. Unknown keys are skipped (a misnamed key never
/// silently injects the wrong byte).
pub fn action_to_writes(action: &PaneStateAction) -> Vec<Vec<u8>> {
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

/// Start the background reactor loop. Self-contained: reads the status tracker
/// from `svc`, dedupes per parked episode, and executes verdicts off the async
/// runtime (its own thread; key writes spawn their own short-lived thread).
pub fn start(svc: ServiceState) {
    std::thread::spawn(move || {
        // Panes already reacted-to since they last became parked — prevents
        // re-firing every tick while a pane sits at the same prompt.
        let mut reacted: HashSet<String> = HashSet::new();
        loop {
            std::thread::sleep(POLL_INTERVAL);
            let ids = svc.agent_tracker.lock().all_ids();
            for pty_id in &ids {
                let status = svc.agent_tracker.lock().get_status(pty_id);
                match status {
                    PaneStatus::Idle | PaneStatus::Waiting => {
                        if reacted.insert(pty_id.clone()) {
                            react(&svc, pty_id, status);
                        }
                    }
                    _ => {
                        reacted.remove(pty_id);
                    }
                }
            }
            reacted.retain(|id| ids.contains(id));
        }
    });
}

/// Classify one parked pane and execute the decided action. Acquires locks in
/// CLAUDE.md order (mux → screen_state → … → pty), each in its own scope, and
/// never holds a lock across the provider call or a thread spawn.
fn react(svc: &ServiceState, pty_id: &str, status: PaneStatus) {
    // mux (1): resolve the pane's OWN identity + provider + session/cwd.
    let (pane_id, agent_type, session_id, cwd) = {
        let mux = svc.mux.read();
        let Some(pane_id) = mux.pane_id_for_pty(pty_id) else {
            return;
        };
        let Some(pane) = mux.get_pane(&pane_id) else {
            return;
        };
        let Some(agent_type) = pane.agent_type.clone() else {
            return; // not an agent pane — nothing to classify
        };
        (
            pane_id,
            agent_type,
            pane.agent_session_id.clone(),
            pane.cwd.clone(),
        )
    };

    // screen_state (3): the rendered tail the classifier fingerprints against.
    let screen_text = svc
        .screen_state
        .lock()
        .tail_text(pty_id, SNAPSHOT_ROWS)
        .unwrap_or_default();

    let snapshot = PaneStateSnapshot {
        pane_id: pane_id.clone(),
        agent_type: agent_type.clone(),
        session_id,
        cwd,
        screen_text,
    };

    // Ask the pane's OWN provider. No lock held across this call.
    let classification: Option<PaneStateClassification> = svc
        .agent_registry
        .with_provider(&agent_type, |p| p.classify_pane_state(&snapshot))
        .flatten();

    match decide(status, classification.as_ref()) {
        ReactorDecision::Ignore => {}
        ReactorDecision::Surface(surface) => surface_to_user(svc, &pane_id, surface),
        ReactorDecision::Act(action) => execute_action(svc, &pane_id, action),
    }
}

fn execute_action(svc: &ServiceState, pane_id: &str, action: PaneStateAction) {
    let writes = action_to_writes(&action);
    if !writes.is_empty() {
        inject_keys(svc, pane_id, writes);
    }
    if let Some(resume) = &action.delayed_resume {
        schedule_resume(svc, pane_id, resume);
    }
    if let Some(surface) = action.surface {
        surface_to_user(svc, pane_id, Some(surface));
    }
}

/// Inject ordered key bytes into the pane's PTY — the same write path as
/// `send --actions`. Spawns a short-lived thread so the poll loop never blocks
/// and so the pty lock is taken outside the loop's cadence.
fn inject_keys(svc: &ServiceState, pane_id: &str, writes: Vec<Vec<u8>>) {
    if svc.inject_raw_keys_async(pane_id, writes).is_err() {
        return;
    }
    log::info!("[state-reactor] injected keys into pane {}", pane_id);
}

/// Schedule a resume via the T7 delayed-send machinery at the parsed ISO time.
/// A reset-time we can't parse is dropped (the classifier should have surfaced
/// instead) rather than fired at a wrong moment.
fn schedule_resume(svc: &ServiceState, pane_id: &str, resume: &super::pane_state::DelayedResume) {
    let Some(at_ms) = parse_iso_to_ms(&resume.at) else {
        log::warn!(
            "[state-reactor] unparseable resume time {:?} for pane {} — skipping",
            resume.at,
            pane_id
        );
        return;
    };
    match crate::delayed_send::create_message(
        svc,
        pane_id.to_string(),
        resume.text.clone(),
        Some(at_ms),
        None,
    ) {
        Ok(_) => log::info!(
            "[state-reactor] scheduled resume for pane {} at {}",
            pane_id,
            resume.at
        ),
        Err(e) => log::warn!("[state-reactor] schedule resume failed: {e}"),
    }
}

/// Surface a verdict to the user: a notification plus a generic chat-card event
/// the frontend renders. Used for clarifying questions, limit notices, and the
/// auth guard.
fn surface_to_user(svc: &ServiceState, pane_id: &str, surface: Option<Surface>) {
    let (kind, payload) = match &surface {
        Some(s) => (s.kind.clone(), s.payload.clone()),
        None => ("pane_state".to_string(), serde_json::Value::Null),
    };
    svc.create_notification(
        format!("Agent needs attention: {kind}"),
        Some(format!("Pane {pane_id}")),
        Some("warning"),
    );
    crate::api::events::emit_everywhere(
        &svc.app_handle,
        "pane-state-surface",
        serde_json::json!({ "pane_id": pane_id, "kind": kind, "payload": payload }),
    );
    log::info!("[state-reactor] surfaced {} for pane {}", kind, pane_id);
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
