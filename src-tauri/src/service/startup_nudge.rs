//! Whether a startup reminder may be written into a tab, and what the host owes
//! that tab instead.
//!
//! A startup reminder is agent-directed prose delivered by pasting into the
//! tab's terminal. Until the agent reaches its composer that terminal belongs to
//! something else — a bare shell, or the provider's own modal prompt — and the
//! paste arrives there as literal keystrokes. That is how a failed launch typed
//! CodeTerm's own routing markers into the user's shell, and how a paste into an
//! unanswered directory-trust menu killed the provider mid-launch.

use super::directory_trust::TrustDecision;
use super::ServiceContext;
use crate::multiplexer::types::TabBacking;

/// What a startup deadline may do with a tab that has not acknowledged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartupNudge {
    /// An agent is attached and nothing modal owns its terminal.
    Deliver,
    /// A directory-trust dialog this tab's provenance entitles CodeTerm to
    /// answer. Answering it is the nudge; the agent was never unresponsive.
    AnsweredTrustDialog,
    /// A modal prompt CodeTerm may not answer. It stays with the user.
    BlockedOnPrompt,
    /// No agent process to read it.
    NoAgent,
}

impl StartupNudge {
    pub(crate) fn delivers(self) -> bool {
        matches!(self, Self::Deliver)
    }

    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::Deliver => "the agent terminal is free",
            Self::AnsweredTrustDialog => "answered its directory-trust dialog instead",
            Self::BlockedOnPrompt => "a provider prompt owns the terminal",
            Self::NoAgent => "no agent process would receive it",
        }
    }
}

/// What the tab's own terminal currently shows. `None` for a tab backed by a
/// chat transport, which has no terminal for a paste to collide with.
pub(crate) struct AgentTerminal<'a> {
    pub(crate) trust: &'a TrustDecision,
    pub(crate) modal_on_screen: bool,
}

/// The whole rule, as a pure decision over the tab's own state.
pub(crate) fn nudge_for(attached: bool, terminal: Option<AgentTerminal<'_>>) -> StartupNudge {
    if !attached {
        return StartupNudge::NoAgent;
    }
    let Some(terminal) = terminal else {
        return StartupNudge::Deliver;
    };
    match terminal.trust {
        TrustDecision::Answer { .. } | TrustDecision::Waiting => StartupNudge::AnsweredTrustDialog,
        TrustDecision::Held => StartupNudge::BlockedOnPrompt,
        TrustDecision::NotApplicable | TrustDecision::NoDialog if terminal.modal_on_screen => {
            StartupNudge::BlockedOnPrompt
        }
        _ => StartupNudge::Deliver,
    }
}

impl ServiceContext {
    /// Classify a tab before writing agent-directed prose into it, answering its
    /// directory-trust dialog on the way when this tab's provenance allows it.
    pub(crate) fn classify_startup_nudge(&self, tab_id: &str) -> StartupNudge {
        let attached = self.mux.read().get_tab(tab_id).map(|tab| {
            let pty_backed = matches!(tab.backing, TabBacking::Pty);
            let attached = tab.agent_type.is_some()
                && tab.exit_suspected_at.is_none()
                && (!pty_backed || tab.pty_id.is_some());
            (attached, pty_backed.then(|| tab.pty_id.clone()).flatten())
        });
        let nudge = match attached {
            None => StartupNudge::NoAgent,
            Some((attached, None)) => nudge_for(attached, None),
            Some((false, Some(_))) => StartupNudge::NoAgent,
            Some((true, Some(pty_id))) => {
                let trust = self.respond_to_directory_trust(tab_id, None);
                let modal_on_screen = !trust.owns_prompt() && self.modal_owns_terminal(&pty_id);
                nudge_for(
                    true,
                    Some(AgentTerminal {
                        trust: &trust,
                        modal_on_screen,
                    }),
                )
            }
        };
        if !nudge.delivers() {
            log::warn!(
                "[startup-nudge] Tab {tab_id}: withheld the startup reminder; {}",
                nudge.reason()
            );
        }
        nudge
    }

    fn modal_owns_terminal(&self, pty_id: &str) -> bool {
        let screen = self
            .screen_state
            .lock()
            .get_screen_text(pty_id)
            .unwrap_or_default();
        crate::agent::detector::detect_interactive_prompt(&screen)
    }
}

#[cfg(test)]
#[path = "startup_nudge/tests.rs"]
mod tests;
