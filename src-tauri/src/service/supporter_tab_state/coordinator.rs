use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::LazyLock;

use super::super::ServiceContext;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub(crate) struct LedgerSnapshot {
    pub(crate) trust: u64,
    pub(crate) update: u64,
    pub(crate) permission: u64,
    pub(crate) supporter: u64,
    pub(crate) prompt: u64,
}

impl LedgerSnapshot {
    pub(crate) fn capture(service: &ServiceContext, tab: &str) -> Self {
        let prompt = {
            let detector = service.prompt_detector.lock();
            super::frame_hash(&serde_json::json!({
                "active": detector.active_prompt(tab),
                "explicit": detector.active_prompt_explicit(tab),
                "occurrence": format!("{:?}", detector.active_prompt_occurrence(tab)),
                "request": detector.pending_permission_request(tab).map(|(instance, _)| instance.id),
            }).to_string())
        };
        Self {
            trust: super::super::directory_trust::observation_revision(tab),
            update: super::super::output::manual_update_observation_revision(tab),
            permission: super::super::permission_auto_confirm::observation_revision(tab),
            supporter: super::state::observation_revision(tab),
            prompt,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObservationIdentity {
    tab: String,
    generation: String,
    pub(crate) occurrence: u64,
    frame: u64,
    revision: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct ActionTicket {
    observation: ObservationIdentity,
    ledgers: LedgerSnapshot,
}

#[derive(Default)]
struct TabOccurrence {
    generation: String,
    prompt: Option<u64>,
    occurrence: u64,
    frame: u64,
    revision: u64,
    claimed: bool,
}

#[derive(Default)]
struct FollowUpSlot {
    scope: String,
    established: bool,
    confirmation: bool,
}

#[derive(Default)]
pub(crate) struct ActionOwner {
    tabs: Mutex<HashMap<String, TabOccurrence>>,
    followups: Mutex<HashMap<String, FollowUpSlot>>,
}

pub(crate) fn global() -> &'static ActionOwner {
    static OWNER: LazyLock<ActionOwner> = LazyLock::new(ActionOwner::default);
    &OWNER
}

impl ActionOwner {
    pub(crate) fn claim_followup(&self, tab: &str, scope: &str, confirmation: bool) -> bool {
        let mut followups = self.followups.lock();
        let slot = followups.entry(tab.into()).or_default();
        if slot.scope != scope {
            *slot = FollowUpSlot {
                scope: scope.into(),
                ..Default::default()
            };
        }
        let claimed = if confirmation {
            &mut slot.confirmation
        } else {
            &mut slot.established
        };
        if *claimed {
            return false;
        }
        *claimed = true;
        true
    }

    pub(crate) fn observe(
        &self,
        tab: &str,
        generation: &str,
        prompt: Option<u64>,
        frame: u64,
    ) -> ObservationIdentity {
        let mut tabs = self.tabs.lock();
        let state = tabs.entry(tab.into()).or_default();
        if state.generation != generation
            || state.prompt != prompt
            || (prompt.is_none() && state.frame != frame)
        {
            state.generation = generation.into();
            state.prompt = prompt;
            state.occurrence = state.occurrence.saturating_add(1);
        }
        state.frame = frame;
        state.revision = state.revision.saturating_add(1);
        state.claimed = false;
        ObservationIdentity {
            tab: tab.into(),
            generation: generation.into(),
            occurrence: state.occurrence,
            frame,
            revision: state.revision,
        }
    }

    pub(crate) fn is_current(&self, observation: &ObservationIdentity) -> bool {
        self.tabs
            .lock()
            .get(&observation.tab)
            .is_some_and(|state| matches(state, observation))
    }

    pub(crate) fn claim(
        &self,
        observation: &ObservationIdentity,
        ledgers: LedgerSnapshot,
    ) -> Option<ActionTicket> {
        let mut tabs = self.tabs.lock();
        let state = tabs.get_mut(&observation.tab)?;
        if !matches(state, observation) || state.claimed {
            return None;
        }
        state.claimed = true;
        Some(ActionTicket {
            observation: observation.clone(),
            ledgers,
        })
    }

    pub(crate) fn ticket_is_current(
        &self,
        ticket: &ActionTicket,
        ledgers: &LedgerSnapshot,
    ) -> bool {
        self.is_current(&ticket.observation) && ticket.ledgers == *ledgers
    }

    #[cfg(test)]
    pub(crate) fn cancel(&self, tab: &str) {
        if let Some(state) = self.tabs.lock().get_mut(tab) {
            state.revision = state.revision.saturating_add(1);
            state.claimed = true;
        }
    }

    pub(crate) fn cancel_observation(&self, observation: &ObservationIdentity) {
        if let Some(state) = self.tabs.lock().get_mut(&observation.tab) {
            if matches(state, observation) {
                state.revision = state.revision.saturating_add(1);
            }
        }
    }
}

fn matches(state: &TabOccurrence, observation: &ObservationIdentity) -> bool {
    state.generation == observation.generation
        && state.occurrence == observation.occurrence
        && state.frame == observation.frame
        && state.revision == observation.revision
}
