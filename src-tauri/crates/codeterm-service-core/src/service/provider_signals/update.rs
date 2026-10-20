use super::*;
use crate::multiplexer::{commands::MuxState, types::Tab};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct SignalSnapshot {
    signals: ProviderSignals,
    provider: Option<String>,
    task: Option<String>,
    worker: String,
}

impl SignalSnapshot {
    pub(super) fn capture(tab: &Tab) -> Self {
        Self {
            signals: tab.provider_signals.clone(),
            provider: tab.agent_type.clone(),
            task: tab
                .task_binding
                .as_ref()
                .and_then(|binding| binding.task_id.clone()),
            worker: tab.id.clone(),
        }
    }

    fn matches(&self, tab: &Tab) -> bool {
        tab.provider_signals == self.signals
            && tab.agent_type == self.provider
            && tab
                .task_binding
                .as_ref()
                .and_then(|binding| binding.task_id.as_ref())
                == self.task.as_ref()
            && tab.id == self.worker
    }
}

pub(super) struct ComputedSignals {
    pub(super) signals: ProviderSignals,
    pub(super) changed: bool,
    pub(super) changed_error: Option<ApiErrorKind>,
    pub(super) refresh_needed: bool,
    pub(super) transition: IncidentTransition,
    pub(super) fresh: Vec<codeterm_provider_signals::ProviderTurnIncident>,
}

fn matches_binding(tab: &Tab, session: &str, nonce: &str, provider: &Option<String>) -> bool {
    tab.agent_session_id.as_deref() == Some(session)
        && tab.session_binding_nonce.as_deref() == Some(nonce)
        && &tab.agent_type == provider
}

pub(super) fn update(
    mux: &MuxState,
    tab_id: &str,
    session: &str,
    nonce: &str,
    provider: &Option<String>,
    records: &[crate::agent::chat_types::RichMessage],
    structured: bool,
) -> Option<ComputedSignals> {
    update_with(mux, tab_id, session, nonce, provider, |snapshot| {
        compute(snapshot, session, records, structured)
    })
}

fn update_with(
    mux: &MuxState,
    tab_id: &str,
    session: &str,
    nonce: &str,
    provider: &Option<String>,
    mut compute: impl FnMut(&SignalSnapshot) -> ComputedSignals,
) -> Option<ComputedSignals> {
    for _ in 0..2 {
        let snapshot = {
            let state = mux.read();
            let tab = state.get_tab(tab_id)?;
            if !matches_binding(tab, session, nonce, provider) {
                return None;
            }
            SignalSnapshot::capture(tab)
        };
        let mut computed = compute(&snapshot);
        let committed = {
            let mut state = mux.write();
            let tab = state.get_tab_mut(tab_id)?;
            if matches_binding(tab, session, nonce, provider) && snapshot.matches(tab) {
                std::mem::swap(&mut tab.provider_signals, &mut computed.signals);
                true
            } else {
                false
            }
        };
        if committed {
            return Some(computed);
        }
    }
    log::debug!("[provider-access] discarded contended update tab={tab_id}");
    None
}

fn compute(
    snapshot: &SignalSnapshot,
    session: &str,
    records: &[crate::agent::chat_types::RichMessage],
    structured: bool,
) -> ComputedSignals {
    let previous = &snapshot.signals;
    let mut signals = previous.clone();
    observe_records(&mut signals, session, records, structured);
    let refresh_needed = previous.auth.as_ref().is_some_and(|auth| !auth.logged_in)
        && records.iter().any(|record| {
            if record
                .provider_session_id
                .as_deref()
                .is_some_and(|id| id != session)
            {
                return false;
            }
            record
                .provider_observations
                .as_ref()
                .is_some_and(|observation| observation.clears_api_error)
        });
    let fresh = fresh_turn_incidents(&mut signals, snapshot, session, records);
    ComputedSignals {
        changed: *previous != signals,
        changed_error: (previous.api_error != signals.api_error)
            .then_some(signals.api_error)
            .flatten(),
        refresh_needed,
        transition: match incident_transition(previous.access_lost(), signals.access_lost()) {
            IncidentTransition::Unchanged
                if !signals.access_lost()
                    && records.iter().any(|record| {
                        record
                            .provider_session_id
                            .as_deref()
                            .is_none_or(|id| id == session)
                            && record
                                .provider_observations
                                .as_ref()
                                .is_some_and(|observation| observation.clears_api_error)
                    }) =>
            {
                IncidentTransition::Recover
            }
            transition => transition,
        },
        fresh,
        signals,
    }
}

pub(super) fn fresh_turn_incidents(
    signals: &mut ProviderSignals,
    snapshot: &SignalSnapshot,
    session: &str,
    records: &[crate::agent::chat_types::RichMessage],
) -> Vec<codeterm_provider_signals::ProviderTurnIncident> {
    use codeterm_provider_signals::{admit_key, ProviderTurnIncident};
    let task = &snapshot.task;
    let provider = snapshot.provider.as_deref().unwrap_or_default();
    let worker = &snapshot.worker;
    let mut fresh = Vec::new();
    for record in records {
        if record
            .provider_session_id
            .as_deref()
            .is_some_and(|id| id != session)
        {
            continue;
        }
        let Some(observation) = record.provider_observations.as_ref() else {
            continue;
        };
        let Some(turn) = ProviderTurnIncident::from_observation(
            worker,
            task.as_deref(),
            provider,
            observation,
            record.uuid.as_deref(),
        ) else {
            continue;
        };
        if admit_key(
            &mut signals.admitted_provider_incidents,
            &turn.idempotency_key,
        ) {
            fresh.push(turn);
        }
    }
    fresh
}

#[cfg(test)]
mod tests;
