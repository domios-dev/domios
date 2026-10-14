//! Applying a provider's own declared turn-state verdict to a tab.
//!
//! The verdict comes from the provider's `hookEventRoster` declaration, so core
//! resolves an incoming event name against data the provider published and
//! never against a provider id. An undeclared event yields nothing and changes
//! nothing.

use codeterm_plugin::agent_runtime::{RuntimeTurnDetail, RuntimeTurnState};
use codeterm_types::pty::{AgentLiveState, QuestionSubtype, TabVerdict, TimelineState};

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
            ProviderStateVerdict::Waiting => TimelineState::Question,
        },
        subtype: None,
    }
    .into()
}

const RUNTIME_STATUS_LINE_CHARS: usize = 160;

/// The live state a structured runtime turn report commits to. The detail names
/// a phase or question subtype; the UI owns the words shown for it.
pub fn live_state_for_runtime(
    state: &RuntimeTurnState,
    detail: Option<RuntimeTurnDetail>,
    status_line: Option<&str>,
) -> AgentLiveState {
    let (timeline, subtype, phase) = match (state, detail) {
        (RuntimeTurnState::Working, Some(RuntimeTurnDetail::Compacting)) => {
            (TimelineState::Working, None, Some("compacting"))
        }
        (RuntimeTurnState::Working, _) => (TimelineState::Working, None, None),
        (RuntimeTurnState::NeedsInput, Some(RuntimeTurnDetail::AwaitingApproval)) => (
            TimelineState::Question,
            Some(QuestionSubtype::Permission),
            None,
        ),
        (RuntimeTurnState::NeedsInput, Some(RuntimeTurnDetail::SystemError)) => {
            (TimelineState::Question, Some(QuestionSubtype::Error), None)
        }
        (RuntimeTurnState::NeedsInput, _) => (TimelineState::Question, None, None),
        (RuntimeTurnState::Idle, Some(RuntimeTurnDetail::Interrupted)) => {
            (TimelineState::Idle, None, Some("interrupted"))
        }
        (RuntimeTurnState::Idle, Some(RuntimeTurnDetail::Failed)) => {
            (TimelineState::Idle, None, Some("failed"))
        }
        (RuntimeTurnState::Idle, _) => (TimelineState::Idle, None, None),
    };
    let mut live: AgentLiveState = TabVerdict {
        state: timeline,
        subtype,
    }
    .into();
    live.phase = phase.map(str::to_string);
    live.status_line = status_line
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.chars().take(RUNTIME_STATUS_LINE_CHARS).collect());
    live
}

/// Commit a verdict onto the tab. Returns true only when the tab's state
/// actually moved, so a repeated report changes nothing and emits nothing.
pub fn apply(
    mux: &MuxState,
    tab_id: &str,
    session_id: &str,
    verdict: ProviderStateVerdict,
) -> bool {
    apply_live(mux, tab_id, session_id, live_state_for(verdict))
}

/// Commit a complete live state onto the tab bound to `session_id`.
pub fn apply_live(mux: &MuxState, tab_id: &str, session_id: &str, next: AgentLiveState) -> bool {
    let mut mux = mux.write();
    match mux.get_tab_mut(tab_id) {
        Some(tab)
            if tab.agent_type.is_some()
                && tab.agent_session_id.as_deref() == Some(session_id)
                && tab.agent_state.as_ref() != Some(&next) =>
        {
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
    session_id: &str,
    event: &str,
) -> Option<ProviderStateVerdict> {
    let event = event.trim();
    if event.is_empty() || session_id.is_empty() {
        return None;
    }
    let (provider_id, bound_session_id) = {
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab_id)?;
        (tab.agent_type.clone()?, tab.agent_session_id.clone()?)
    };
    if bound_session_id != session_id {
        return None;
    }
    let roster = svc
        .agent_registry
        .with_provider(&provider_id, |provider| provider.hook_event_roster())
        .unwrap_or_default();
    let verdict = declared_state_verdict(&roster, event);
    log::debug!(
        "[agent-state-event] tab={tab_id} session={session_id} event={event} declared={verdict:?}"
    );
    let verdict = verdict?;
    let changed = apply(&svc.mux, tab_id, session_id, verdict);
    log::debug!(
        "[agent-state-event] tab={tab_id} session={session_id} event={event} changed={changed}"
    );
    changed.then(|| {
        svc.emit_state_changed();
        svc.record_turn_resume_edge(tab_id);
        verdict
    })
}

#[cfg(test)]
mod tests;
