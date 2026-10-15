use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

use parking_lot::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObservationEvent {
    SpawnReady,
    SpawnDeadline,
    StartupEscalation,
    PromptCandidate,
    WorkerIdle,
    BusyNoOutput,
    TabExit,
}

pub(crate) use codeterm_supporter::policy::{confidence_band, ConfidenceBand};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SignalAction {
    SpawnOk,
    StrongAction,
    WeakHint,
    None,
}

pub(crate) fn launch_episode_key(
    tab_id: &str,
    pty_id: Option<&str>,
    agent_set_at: Option<Instant>,
) -> String {
    let launch_identity = pty_id
        .map(|pty_id| format!("pty:{pty_id}"))
        .or_else(|| agent_set_at.map(|started| format!("agent-set:{started:?}")))
        .unwrap_or_else(|| "tab-lifetime".to_string());
    format!("{tab_id}:{launch_identity}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObservationPermit {
    pub(crate) episode: u64,
    pub(crate) observation_id: u64,
    pub(crate) spawn_claim: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PermitError {
    SpawnAlreadyObserved,
    BusyAlreadyObserved,
}

#[derive(Default)]
struct EpisodeState {
    session_key: String,
    episode: u64,
    output_sequence: u64,
    output_epoch_seconds: u64,
    spawn_observed: bool,
    spawn_claim: Option<u64>,
    last_busy_output_sequence: Option<u64>,
    previous_strong_label: Option<String>,
    applied_observation_ids: BTreeSet<u64>,
    acted_labels: BTreeSet<String>,
    weak_emitted: bool,
    signaled: bool,
    previous_screen_hash: Option<u64>,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EpisodeSnapshot {
    pub(crate) session_key: String,
    pub(crate) episode: u64,
    pub(crate) output_sequence: u64,
    pub(crate) signaled: bool,
    pub(crate) spawn_observed: bool,
}

static EPISODES: OnceLock<Mutex<HashMap<String, EpisodeState>>> = OnceLock::new();
static NEXT_SPAWN_CLAIM: AtomicU64 = AtomicU64::new(1);

pub(crate) fn next_observation_id() -> u64 {
    NEXT_SPAWN_CLAIM.fetch_add(1, Ordering::Relaxed)
}

fn episodes() -> &'static Mutex<HashMap<String, EpisodeState>> {
    EPISODES.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(test)]
pub(crate) fn snapshot_episode(tab_id: &str) -> EpisodeSnapshot {
    let all = episodes().lock();
    all.get(tab_id)
        .map(|state| EpisodeSnapshot {
            session_key: state.session_key.clone(),
            episode: state.episode,
            output_sequence: state.output_sequence,
            signaled: state.signaled,
            spawn_observed: state.spawn_observed,
        })
        .unwrap_or_else(|| EpisodeSnapshot {
            session_key: String::new(),
            episode: 0,
            output_sequence: 0,
            signaled: false,
            spawn_observed: false,
        })
}

fn state_for_session<'a>(
    all: &'a mut HashMap<String, EpisodeState>,
    tab_id: &str,
    session_key: &str,
) -> &'a mut EpisodeState {
    let state = all.entry(tab_id.to_owned()).or_default();
    if state.session_key != session_key {
        let next_episode = state.episode.saturating_add(1);
        let previous_screen_hash = state.previous_screen_hash;
        let output_sequence = state.output_sequence;
        let output_epoch_seconds = state.output_epoch_seconds;
        *state = EpisodeState {
            session_key: session_key.to_owned(),
            episode: next_episode,
            output_sequence,
            output_epoch_seconds,
            previous_screen_hash,
            ..EpisodeState::default()
        };
    }
    state
}

pub(crate) fn episode(tab: &str, session: &str) -> u64 {
    state_for_session(&mut episodes().lock(), tab, session).episode
}

pub(crate) fn observation_revision(tab_id: &str) -> u64 {
    episodes().lock().get(tab_id).map_or(0, |state| {
        super::frame_hash(&format!(
            "{}:{:?}:{}:{}",
            state.episode, state.acted_labels, state.weak_emitted, state.signaled
        ))
    })
}

pub(crate) fn claim_observation(
    tab_id: &str,
    session_key: &str,
    event: ObservationEvent,
) -> Result<ObservationPermit, PermitError> {
    let mut all = episodes().lock();
    let state = state_for_session(&mut all, tab_id, session_key);
    if matches!(
        event,
        ObservationEvent::SpawnReady | ObservationEvent::SpawnDeadline
    ) {
        if state.spawn_observed {
            return Err(PermitError::SpawnAlreadyObserved);
        }
        state.spawn_observed = true;
        state.spawn_claim = Some(NEXT_SPAWN_CLAIM.fetch_add(1, Ordering::Relaxed));
    }
    if event == ObservationEvent::BusyNoOutput {
        if state.last_busy_output_sequence == Some(state.output_sequence) {
            return Err(PermitError::BusyAlreadyObserved);
        }
        state.last_busy_output_sequence = Some(state.output_sequence);
    }
    let spawn_claim = if matches!(
        event,
        ObservationEvent::SpawnReady | ObservationEvent::SpawnDeadline
    ) {
        state.spawn_claim
    } else {
        None
    };
    Ok(ObservationPermit {
        observation_id: next_observation_id(),
        episode: state.episode,
        spawn_claim,
    })
}

pub(crate) fn note_output(tab_id: &str) {
    let mut all = episodes().lock();
    let state = all.entry(tab_id.to_owned()).or_default();
    state.output_sequence = state.output_sequence.saturating_add(1);
    state.output_epoch_seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
}

pub(crate) fn last_output_epoch_seconds(tab_id: &str) -> u64 {
    episodes()
        .lock()
        .get(tab_id)
        .map(|state| state.output_epoch_seconds)
        .unwrap_or_default()
}

pub(crate) fn screen_changed(tab_id: &str, session_key: &str, hash: u64) -> bool {
    let mut all = episodes().lock();
    let state = state_for_session(&mut all, tab_id, session_key);
    let changed = state
        .previous_screen_hash
        .is_some_and(|previous| previous != hash);
    if changed {
        state.output_sequence = state.output_sequence.saturating_add(1);
        state.output_epoch_seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if state.signaled {
            state.episode = state.episode.saturating_add(1);
            state.last_busy_output_sequence = None;
            state.previous_strong_label = None;
            state.applied_observation_ids.clear();
            state.acted_labels.clear();
            state.weak_emitted = false;
            state.signaled = false;
        }
    }
    state.previous_screen_hash = Some(hash);
    changed
}

#[cfg(test)]
pub(crate) fn apply_verdict(
    tab_id: &str,
    session_key: &str,
    label: &str,
    confidence: f64,
) -> SignalAction {
    let mut all = episodes().lock();
    let state = state_for_session(&mut all, tab_id, session_key);
    apply_verdict_to_state(state, label, confidence)
}

pub(crate) fn apply_observed_verdict(
    tab_id: &str,
    session_key: &str,
    permit: &ObservationPermit,
    label: &str,
    confidence: f64,
) -> Option<SignalAction> {
    let mut all = episodes().lock();
    let state = all.get_mut(tab_id)?;
    if state.session_key != session_key || state.episode != permit.episode {
        return None;
    }
    if !state.applied_observation_ids.insert(permit.observation_id) {
        return None;
    }
    Some(apply_verdict_to_state(state, label, confidence))
}

pub(crate) fn permit_is_current(
    tab_id: &str,
    session_key: &str,
    permit: &ObservationPermit,
) -> bool {
    let all = episodes().lock();
    all.get(tab_id)
        .is_some_and(|state| state.session_key == session_key && state.episode == permit.episode)
}

pub(crate) fn audit_permit(
    tab: &str,
    session: &str,
    observation_id: u64,
) -> Option<ObservationPermit> {
    let all = episodes().lock();
    let state = all.get(tab).filter(|state| state.session_key == session)?;
    Some(ObservationPermit {
        episode: state.episode,
        observation_id,
        spawn_claim: None,
    })
}

/// Spawn-ok belongs to its launch claim and survives signal-episode churn.
pub(crate) fn permit_is_current_for_launch(
    tab_id: &str,
    session_key: &str,
    permit: &ObservationPermit,
) -> bool {
    let all = episodes().lock();
    permit.spawn_claim.is_some_and(|spawn_claim| {
        all.get(tab_id).is_some_and(|state| {
            state.session_key == session_key
                && state.spawn_observed
                && state.spawn_claim == Some(spawn_claim)
        })
    })
}

fn apply_verdict_to_state(state: &mut EpisodeState, label: &str, confidence: f64) -> SignalAction {
    if label == super::OTHER {
        state.previous_strong_label = None;
        return SignalAction::None;
    }
    let action = match confidence_band(confidence) {
        ConfidenceBand::Ignored => {
            state.previous_strong_label = None;
            SignalAction::None
        }
        ConfidenceBand::Weak => {
            state.previous_strong_label = None;
            if !state.weak_emitted {
                state.weak_emitted = true;
                SignalAction::WeakHint
            } else {
                SignalAction::None
            }
        }
        ConfidenceBand::Strong => {
            let consecutive = state.previous_strong_label.as_deref() == Some(label);
            state.previous_strong_label = Some(label.to_owned());
            if consecutive && state.acted_labels.insert(label.to_owned()) {
                SignalAction::StrongAction
            } else if !state.weak_emitted {
                state.weak_emitted = true;
                SignalAction::WeakHint
            } else {
                SignalAction::None
            }
        }
    };
    if action == SignalAction::StrongAction {
        state.signaled = true;
    }
    action
}

#[cfg(test)]
mod tests;
