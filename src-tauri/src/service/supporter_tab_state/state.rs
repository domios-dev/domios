use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

use parking_lot::Mutex;

pub(crate) const CALL_CAP_PER_EPISODE: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObservationEvent {
    SpawnReady,
    SpawnDeadline,
    WorkerIdle,
    BusyNoOutput,
    TabExit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfidenceBand {
    Strong,
    Weak,
    Ignored,
}

pub(crate) fn confidence_band(confidence: f64) -> ConfidenceBand {
    if !(0.0..=1.0).contains(&confidence) {
        return ConfidenceBand::Ignored;
    }
    if confidence >= 0.80 {
        ConfidenceBand::Strong
    } else if confidence >= 0.60 {
        ConfidenceBand::Weak
    } else {
        ConfidenceBand::Ignored
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SignalAction {
    SpawnOk,
    StrongAction,
    WeakHint,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObservationPermit {
    pub(crate) episode: u64,
    pub(crate) output_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PermitError {
    SpawnAlreadyObserved,
    BusyAlreadyObserved,
    CallCapReached,
}

#[derive(Default)]
struct EpisodeState {
    session_key: String,
    episode: u64,
    output_sequence: u64,
    output_epoch_seconds: u64,
    calls: usize,
    spawn_observed: bool,
    last_busy_output_sequence: Option<u64>,
    previous_strong_label: Option<String>,
    acted_labels: BTreeSet<String>,
    weak_emitted: bool,
    signaled: bool,
    previous_screen_hash: Option<u64>,
}

static EPISODES: OnceLock<Mutex<HashMap<String, EpisodeState>>> = OnceLock::new();

fn episodes() -> &'static Mutex<HashMap<String, EpisodeState>> {
    EPISODES.get_or_init(|| Mutex::new(HashMap::new()))
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
    }
    if event == ObservationEvent::BusyNoOutput {
        if state.last_busy_output_sequence == Some(state.output_sequence) {
            return Err(PermitError::BusyAlreadyObserved);
        }
        state.last_busy_output_sequence = Some(state.output_sequence);
    }
    if state.calls >= CALL_CAP_PER_EPISODE {
        return Err(PermitError::CallCapReached);
    }
    state.calls += 1;
    Ok(ObservationPermit {
        episode: state.episode,
        output_sequence: state.output_sequence,
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
    if state.signaled {
        let session_key = state.session_key.clone();
        let previous_screen_hash = state.previous_screen_hash;
        let spawn_observed = state.spawn_observed;
        let episode = state.episode.saturating_add(1);
        let output_sequence = state.output_sequence;
        let output_epoch_seconds = state.output_epoch_seconds;
        *state = EpisodeState {
            session_key,
            episode,
            output_sequence,
            output_epoch_seconds,
            spawn_observed,
            previous_screen_hash,
            ..EpisodeState::default()
        };
    }
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
            state.calls = 0;
            state.last_busy_output_sequence = None;
            state.previous_strong_label = None;
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
    strong_actions: bool,
    weak_hints: bool,
) -> SignalAction {
    let mut all = episodes().lock();
    let state = state_for_session(&mut all, tab_id, session_key);
    apply_verdict_to_state(state, label, confidence, strong_actions, weak_hints)
}

pub(crate) fn apply_observed_verdict(
    tab_id: &str,
    session_key: &str,
    permit: &ObservationPermit,
    label: &str,
    confidence: f64,
    strong_actions: bool,
    weak_hints: bool,
) -> Option<SignalAction> {
    let mut all = episodes().lock();
    let state = state_for_session(&mut all, tab_id, session_key);
    if state.episode != permit.episode || state.output_sequence != permit.output_sequence {
        return None;
    }
    Some(apply_verdict_to_state(
        state,
        label,
        confidence,
        strong_actions,
        weak_hints,
    ))
}

pub(crate) fn permit_is_current(
    tab_id: &str,
    session_key: &str,
    permit: &ObservationPermit,
) -> bool {
    let all = episodes().lock();
    all.get(tab_id).is_some_and(|state| {
        state.session_key == session_key
            && state.episode == permit.episode
            && state.output_sequence == permit.output_sequence
    })
}

fn apply_verdict_to_state(
    state: &mut EpisodeState,
    label: &str,
    confidence: f64,
    strong_actions: bool,
    weak_hints: bool,
) -> SignalAction {
    let action = match confidence_band(confidence) {
        ConfidenceBand::Ignored => {
            state.previous_strong_label = None;
            SignalAction::None
        }
        ConfidenceBand::Weak => {
            state.previous_strong_label = None;
            if weak_hints && !state.weak_emitted {
                state.weak_emitted = true;
                SignalAction::WeakHint
            } else {
                SignalAction::None
            }
        }
        ConfidenceBand::Strong => {
            let consecutive = state.previous_strong_label.as_deref() == Some(label);
            state.previous_strong_label = Some(label.to_owned());
            if consecutive && strong_actions && state.acted_labels.insert(label.to_owned()) {
                SignalAction::StrongAction
            } else if weak_hints && !state.weak_emitted {
                state.weak_emitted = true;
                SignalAction::WeakHint
            } else {
                SignalAction::None
            }
        }
    };
    if action != SignalAction::None {
        state.signaled = true;
    }
    action
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
