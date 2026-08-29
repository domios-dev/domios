use serde::Serialize;

use super::session_binding_deferral::{DeferredSessionBinding, RefusedSessionBinding};
use super::ServiceContext;
use crate::multiplexer::types::TabBacking;
use codeterm_types::pty::BootStage;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SessionBindingStateKind {
    Healthy,
    DeferredPending,
    Refused,
    Unbound,
    StalePending,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct SessionBindingState {
    pub tab_id: String,
    pub kind: SessionBindingStateKind,
    pub current_session_id: Option<String>,
    pub requested_session_id: Option<String>,
    pub reason: Option<String>,
}

fn classify_session_binding_state(
    tab_id: &str,
    current_session_id: Option<&str>,
    agent_alive: bool,
    pending: Option<DeferredSessionBinding>,
    refused: Option<RefusedSessionBinding>,
) -> SessionBindingState {
    if let Some(binding) = pending {
        let kind = if current_session_id == Some(binding.previous_session_id.as_str()) {
            SessionBindingStateKind::DeferredPending
        } else {
            SessionBindingStateKind::StalePending
        };
        return SessionBindingState {
            tab_id: tab_id.to_string(),
            kind,
            current_session_id: current_session_id.map(str::to_string),
            requested_session_id: Some(binding.session_id),
            reason: None,
        };
    }
    if let Some(refusal) =
        refused.filter(|refusal| refusal.previous_session_id.as_deref() == current_session_id)
    {
        return SessionBindingState {
            tab_id: tab_id.to_string(),
            kind: SessionBindingStateKind::Refused,
            current_session_id: current_session_id.map(str::to_string),
            requested_session_id: Some(refusal.session_id),
            reason: Some(refusal.reason),
        };
    }
    SessionBindingState {
        tab_id: tab_id.to_string(),
        kind: if agent_alive && current_session_id.is_none() {
            SessionBindingStateKind::Unbound
        } else {
            SessionBindingStateKind::Healthy
        },
        current_session_id: current_session_id.map(str::to_string),
        requested_session_id: None,
        reason: None,
    }
}

impl ServiceContext {
    pub(crate) fn session_binding_state(&self, tab_id: &str) -> Option<SessionBindingState> {
        let (current_session_id, agent_alive) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id)?;
            (
                tab.agent_session_id.clone(),
                tab.agent_type.is_some()
                    && matches!(&tab.backing, TabBacking::Pty)
                    && matches!(
                        tab.boot_stage,
                        Some(BootStage::AgentReady | BootStage::PtyReattached)
                    ),
            )
        };
        Some(classify_session_binding_state(
            tab_id,
            current_session_id.as_deref(),
            agent_alive,
            self.deferred_session_bindings.pending(tab_id),
            self.deferred_session_bindings.refused(tab_id),
        ))
    }

    pub(crate) fn emit_session_binding_state(&self, tab_id: &str) {
        if let Some(state) = self.session_binding_state(tab_id) {
            crate::api::events::emit_everywhere(
                &self.app_handle,
                "session-binding-state-changed",
                state,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn pending(previous_session_id: &str) -> DeferredSessionBinding {
        DeferredSessionBinding {
            generation: 1,
            tab_id: "tab-a".into(),
            provider_id: "provider".into(),
            session_id: "requested".into(),
            previous_session_id: previous_session_id.into(),
            transcript_path: PathBuf::from("transcript.jsonl"),
        }
    }

    #[test]
    fn session_binding_state_distinguishes_pending_from_stale_pending() {
        let deferred = classify_session_binding_state(
            "tab-a",
            Some("previous"),
            true,
            Some(pending("previous")),
            None,
        );
        let stale = classify_session_binding_state(
            "tab-a",
            Some("replacement"),
            true,
            Some(pending("previous")),
            None,
        );
        assert_eq!(deferred.kind, SessionBindingStateKind::DeferredPending);
        assert_eq!(stale.kind, SessionBindingStateKind::StalePending);
    }

    #[test]
    fn session_binding_state_preserves_typed_refusal() {
        let refusal = RefusedSessionBinding {
            session_id: "requested".into(),
            previous_session_id: Some("previous".into()),
            reason: "refused".into(),
        };
        let state = classify_session_binding_state(
            "tab-a",
            Some("previous"),
            true,
            None,
            Some(refusal.clone()),
        );
        assert_eq!(state.kind, SessionBindingStateKind::Refused);
        assert_eq!(state.requested_session_id.as_deref(), Some("requested"));
        let superseded =
            classify_session_binding_state("tab-a", Some("replacement"), true, None, Some(refusal));
        assert_eq!(superseded.kind, SessionBindingStateKind::Healthy);
    }

    #[test]
    fn session_binding_state_exposes_live_agent_without_session() {
        let state = classify_session_binding_state("tab-a", None, true, None, None);
        assert_eq!(state.kind, SessionBindingStateKind::Unbound);
    }

    #[test]
    fn session_binding_state_keeps_bound_tab_healthy() {
        let state = classify_session_binding_state("tab-a", Some("current"), true, None, None);
        assert_eq!(state.kind, SessionBindingStateKind::Healthy);
        assert_eq!(state.current_session_id.as_deref(), Some("current"));
    }
}
