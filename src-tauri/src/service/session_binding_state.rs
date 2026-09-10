use serde::Serialize;

use super::session_binding_deferral::{DeferredSessionBinding, RefusedSessionBinding};
use super::ServiceContext;
use crate::multiplexer::types::{PriorBindingNonce, TabBacking};
use codeterm_types::pty::BootStage;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SessionBindingStateKind {
    Healthy,
    DeferredPending,
    Refused,
    NeverBound,
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

/// The deferral still describes this tab when the session it was protecting is
/// the one the tab is on; otherwise the tab has moved past it.
fn deferral_holds_current_session(
    current_session_id: Option<&str>,
    binding: &DeferredSessionBinding,
) -> bool {
    current_session_id == Some(binding.previous_session_id.as_str())
}

fn has_prior_session_binding(lineage: &[PriorBindingNonce]) -> bool {
    lineage.iter().any(|entry| entry.session_id.is_some())
}

fn classify_session_binding_state(
    tab_id: &str,
    current_session_id: Option<&str>,
    agent_alive: bool,
    prior_session_binding_nonces: &[PriorBindingNonce],
    pending: Option<DeferredSessionBinding>,
    refused: Option<RefusedSessionBinding>,
) -> SessionBindingState {
    if let Some(binding) = pending {
        let holds_current = deferral_holds_current_session(current_session_id, &binding);
        // A live agent answering on its own session is working, not recovering:
        // the deferral is a rebind queued behind it, not a broken binding.
        let outranked_by_liveness = holds_current && agent_alive;
        if !outranked_by_liveness {
            return SessionBindingState {
                tab_id: tab_id.to_string(),
                kind: if holds_current {
                    SessionBindingStateKind::DeferredPending
                } else {
                    SessionBindingStateKind::StalePending
                },
                current_session_id: current_session_id.map(str::to_string),
                requested_session_id: Some(binding.session_id),
                reason: None,
            };
        }
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
            if has_prior_session_binding(prior_session_binding_nonces) {
                SessionBindingStateKind::Unbound
            } else {
                SessionBindingStateKind::NeverBound
            }
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
        let (current_session_id, agent_alive, prior_session_binding_nonces) = {
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
                tab.prior_session_binding_nonces.clone(),
            )
        };
        Some(classify_session_binding_state(
            tab_id,
            current_session_id.as_deref(),
            agent_alive,
            &prior_session_binding_nonces,
            self.deferred_session_bindings.pending(tab_id),
            self.deferred_session_bindings.refused(tab_id),
        ))
    }

    /// Emission is the transition boundary, so a deferral the tab has already
    /// moved past is retired here instead of left for a later read to trust.
    fn retire_resolved_session_binding(&self, tab_id: &str) {
        let current_session_id = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .and_then(|tab| tab.agent_session_id.clone())
        };
        if self
            .deferred_session_bindings
            .retire_resolved(tab_id, current_session_id.as_deref())
        {
            log::info!(
                "[session-binding-deferral] retired resolved deferral for Tab {}",
                tab_id
            );
        }
    }

    pub(crate) fn emit_session_binding_state(&self, tab_id: &str) {
        self.retire_resolved_session_binding(tab_id);
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

    fn prior_session(session_id: &str) -> Vec<PriorBindingNonce> {
        vec![PriorBindingNonce {
            session_id: Some(session_id.into()),
            nonce: "prior-nonce".into(),
        }]
    }

    #[test]
    fn session_binding_state_distinguishes_pending_from_stale_pending() {
        let deferred = classify_session_binding_state(
            "tab-a",
            Some("previous"),
            false,
            &[],
            Some(pending("previous")),
            None,
        );
        let stale = classify_session_binding_state(
            "tab-a",
            Some("replacement"),
            true,
            &prior_session("previous"),
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
            &[],
            None,
            Some(refusal.clone()),
        );
        assert_eq!(state.kind, SessionBindingStateKind::Refused);
        assert_eq!(state.requested_session_id.as_deref(), Some("requested"));
        let superseded = classify_session_binding_state(
            "tab-a",
            Some("replacement"),
            true,
            &[],
            None,
            Some(refusal),
        );
        assert_eq!(superseded.kind, SessionBindingStateKind::Healthy);
    }

    #[test]
    fn session_binding_state_exposes_live_agent_without_session() {
        let state = classify_session_binding_state("tab-a", None, true, &[], None, None);
        assert_eq!(state.kind, SessionBindingStateKind::NeverBound);
    }

    #[test]
    fn session_binding_state_distinguishes_never_bound_from_lost() {
        let never_bound = classify_session_binding_state("tab-a", None, true, &[], None, None);
        let prior = prior_session("previous");
        let lost = classify_session_binding_state("tab-a", None, true, &prior, None, None);

        assert_eq!(never_bound.kind, SessionBindingStateKind::NeverBound);
        assert_eq!(lost.kind, SessionBindingStateKind::Unbound);
        assert_eq!(never_bound.current_session_id, None);
        assert_eq!(lost.current_session_id, None);
    }

    #[test]
    fn session_binding_state_keeps_bound_tab_healthy() {
        let state = classify_session_binding_state("tab-a", Some("current"), true, &[], None, None);
        assert_eq!(state.kind, SessionBindingStateKind::Healthy);
        assert_eq!(state.current_session_id.as_deref(), Some("current"));
    }

    /// rollacode/codeterm#381: the deferral record outranked liveness, so a
    /// bound agent that was answering still reported "recovering".
    #[test]
    fn live_bound_agent_is_not_classified_deferred() {
        let state = classify_session_binding_state(
            "tab-a",
            Some("previous"),
            true,
            &prior_session("previous"),
            Some(pending("previous")),
            None,
        );
        assert_ne!(state.kind, SessionBindingStateKind::DeferredPending);
        assert_eq!(state.kind, SessionBindingStateKind::Healthy);
        assert_eq!(state.current_session_id.as_deref(), Some("previous"));
    }

    /// The guard on the fix above: a true deferral is still a deferral, so the
    /// false positive is narrowed rather than the branch deleted.
    #[test]
    fn genuinely_deferred_binding_still_classifies_deferred() {
        let not_yet_live = classify_session_binding_state(
            "tab-a",
            Some("previous"),
            false,
            &prior_session("previous"),
            Some(pending("previous")),
            None,
        );
        assert_eq!(not_yet_live.kind, SessionBindingStateKind::DeferredPending);
        assert_eq!(
            not_yet_live.requested_session_id.as_deref(),
            Some("requested")
        );
    }

    /// The other three kinds, each pinned on its own case, so the change cannot
    /// be satisfied by collapsing the state machine into two states.
    #[test]
    fn binding_state_kinds_are_unchanged() {
        let stale = classify_session_binding_state(
            "tab-a",
            Some("replacement"),
            true,
            &[],
            Some(pending("previous")),
            None,
        );
        assert_eq!(stale.kind, SessionBindingStateKind::StalePending);

        let refused = classify_session_binding_state(
            "tab-a",
            Some("previous"),
            true,
            &[],
            None,
            Some(RefusedSessionBinding {
                session_id: "requested".into(),
                previous_session_id: Some("previous".into()),
                reason: "refused".into(),
            }),
        );
        assert_eq!(refused.kind, SessionBindingStateKind::Refused);
        assert_eq!(refused.reason.as_deref(), Some("refused"));

        let unbound = classify_session_binding_state(
            "tab-a",
            None,
            true,
            &prior_session("previous"),
            None,
            None,
        );
        assert_eq!(unbound.kind, SessionBindingStateKind::Unbound);
    }

    /// The second half of rollacode/codeterm#381: masking a resolved record in
    /// classification still leaves it for the next reader, so emission retires it.
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn resolved_binding_retires_its_deferral_record() {
        let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let workspace = fixture
            .service
            .mux
            .write()
            .create_workspace("binding".into(), None)
            .id;
        let tab_id = fixture.service.mux.write().create_tab(&workspace).unwrap();
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&tab_id)
            .unwrap()
            .agent_session_id = Some("previous".into());
        fixture
            .service
            .deferred_session_bindings
            .defer(
                &tab_id,
                "provider",
                "requested",
                "previous",
                fixture.temp.path().join("requested.jsonl"),
                std::sync::Arc::new(|_, _| {}),
            )
            .unwrap();

        fixture.service.emit_session_binding_state(&tab_id);
        assert!(
            fixture
                .service
                .deferred_session_bindings
                .pending(&tab_id)
                .is_some(),
            "an unresolved deferral is still armed and must survive emission"
        );

        // The binding resolves exactly as `commit_reported_session` commits it.
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&tab_id)
            .unwrap()
            .agent_session_id = Some("requested".into());
        fixture.service.emit_session_binding_state(&tab_id);

        assert!(
            fixture
                .service
                .deferred_session_bindings
                .pending(&tab_id)
                .is_none(),
            "a resolved binding leaves no deferral record for a later reader"
        );
    }
}
