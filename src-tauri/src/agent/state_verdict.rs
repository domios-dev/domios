//! Applying a provider's own declared turn-state verdict to a tab.
//!
//! The verdict comes from the provider's `hookEventRoster` declaration, so core
//! resolves an incoming event name against data the provider published and
//! never against a provider id. An undeclared event yields nothing and changes
//! nothing.

use codeterm_types::pty::{AgentLiveState, TabVerdict, TimelineState};

use crate::agent::provider::{declared_state_verdict, ProviderStateVerdict};
use crate::multiplexer::commands::MuxState;
use crate::service::ServiceContext;

/// The live state a declared verdict commits to. Kept separate from the
/// screen-derived classifier: an authoritative provider signal must never have
/// to be inferred back out of terminal text.
pub fn live_state_for(verdict: ProviderStateVerdict) -> AgentLiveState {
    TabVerdict {
        state: match verdict {
            ProviderStateVerdict::Working => TimelineState::Working,
            ProviderStateVerdict::Idle => TimelineState::Idle,
        },
        subtype: None,
    }
    .into()
}

/// Commit a verdict onto the tab. Returns true only when the tab's state
/// actually moved, so a repeated report changes nothing and emits nothing.
pub fn apply(mux: &MuxState, tab_id: &str, verdict: ProviderStateVerdict) -> bool {
    let next = live_state_for(verdict);
    let mut mux = mux.write();
    match mux.get_tab_mut(tab_id) {
        Some(tab) if tab.agent_type.is_some() && tab.agent_state.as_ref() != Some(&next) => {
            tab.agent_state = Some(next);
            true
        }
        _ => false,
    }
}

/// Resolve one streamed event name against the tab provider's declared roster
/// and commit whatever it declares. Returns the applied verdict, if any.
pub fn observe_declared_event(
    svc: &ServiceContext,
    tab_id: &str,
    event: &str,
) -> Option<ProviderStateVerdict> {
    let event = event.trim();
    if event.is_empty() {
        return None;
    }
    let provider_id = svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.agent_type.clone())?;
    let roster = svc
        .agent_registry
        .with_provider(&provider_id, |provider| provider.hook_event_roster())
        .unwrap_or_default();
    let verdict = declared_state_verdict(&roster, event)?;
    apply(&svc.mux, tab_id, verdict).then(|| {
        svc.emit_state_changed();
        verdict
    })
}

#[cfg(test)]
mod tests;
