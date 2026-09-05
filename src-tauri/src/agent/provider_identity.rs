use crate::multiplexer::types::Tab;
use crate::session::lifecycle::AgentTypeWriteOrigin;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderIdentityEvidence {
    pub(crate) cwd: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) configured_nonce: Option<String>,
    pub(crate) reported_nonce: Option<String>,
}

impl ProviderIdentityEvidence {
    pub(crate) fn capture(tab: &Tab) -> Self {
        Self {
            cwd: tab.cwd.clone(),
            session_id: tab.agent_session_id.clone(),
            configured_nonce: tab.session_binding_nonce.clone(),
            reported_nonce: tab.reported_session_binding_nonce.clone(),
        }
    }

    pub(crate) fn nonce_verified(&self) -> bool {
        self.session_id.is_some()
            && self.configured_nonce.is_some()
            && self.configured_nonce == self.reported_nonce
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ProviderIdentityWrite {
    Applied {
        previous: Option<String>,
        current: String,
    },
    RejectedClaimedSession {
        current: Option<String>,
        attempted: String,
        session_id: String,
    },
}

pub(crate) fn apply_provider_identity(
    tab: &mut Tab,
    provider: String,
    lock: bool,
    origin: AgentTypeWriteOrigin,
) -> ProviderIdentityWrite {
    let conflicting_claim = match (&tab.agent_type, &tab.agent_session_id) {
        (current, Some(session_id))
            if origin == AgentTypeWriteOrigin::Parser
                && current.as_deref() != Some(provider.as_str()) =>
        {
            Some((current.clone(), session_id.clone()))
        }
        _ => None,
    };
    if let Some((current, session_id)) = conflicting_claim {
        return ProviderIdentityWrite::RejectedClaimedSession {
            current,
            attempted: provider,
            session_id,
        };
    }

    let previous = tab.agent_type.clone();
    tab.agent_set_at = Some(std::time::Instant::now());
    tab.exit_suspected_at = None;
    tab.agent_type = Some(provider.clone());
    if lock {
        tab.agent_type_locked = true;
    } else if matches!(
        origin,
        AgentTypeWriteOrigin::Rebind | AgentTypeWriteOrigin::Repair
    ) {
        tab.agent_type_locked = false;
    }
    ProviderIdentityWrite::Applied {
        previous,
        current: provider,
    }
}

pub(crate) fn clear_unproven_provider_identity(tab: &mut Tab) -> Option<String> {
    let previous = tab.agent_type.take();
    tab.agent_type_locked = false;
    tab.agent_set_at = None;
    tab.exit_suspected_at = None;
    previous
}

pub(crate) fn provider_for_claimed_session(
    registry: &crate::agent::registry::AgentRegistry,
    cwd: &str,
    session_id: &str,
) -> Option<String> {
    let mut owners = registry.provider_ids().into_iter().filter(|provider_id| {
        registry
            .with_provider(provider_id, |provider| {
                provider.session_exists(cwd, session_id)
            })
            .unwrap_or(false)
    });
    let owner = owners.next()?;
    owners.next().is_none().then_some(owner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multiplexer::state::MultiplexerState;

    fn tab_with_claimed_session(provider: &str) -> Tab {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some(provider.into());
        tab.agent_type_locked = false;
        tab.agent_session_id = Some("owned-session".into());
        tab.session_binding_nonce = Some("launch-binding".into());
        tab.reported_session_binding_nonce = Some("launch-binding".into());
        tab.clone()
    }

    #[test]
    fn claimed_codex_session_refuses_conflicting_claude_guess() {
        let mut tab = tab_with_claimed_session("codex");

        let outcome = apply_provider_identity(
            &mut tab,
            "claude".into(),
            true,
            AgentTypeWriteOrigin::Parser,
        );

        assert!(matches!(
            outcome,
            ProviderIdentityWrite::RejectedClaimedSession { .. }
        ));
        assert_eq!(tab.agent_type.as_deref(), Some("codex"));
        assert!(!tab.agent_type_locked);
    }

    #[test]
    fn claimed_claude_session_keeps_claude_identity() {
        let mut tab = tab_with_claimed_session("claude");

        let outcome = apply_provider_identity(
            &mut tab,
            "claude".into(),
            true,
            AgentTypeWriteOrigin::Parser,
        );

        assert!(matches!(outcome, ProviderIdentityWrite::Applied { .. }));
        assert_eq!(tab.agent_type.as_deref(), Some("claude"));
        assert!(tab.agent_type_locked);
    }

    #[test]
    fn claimed_session_without_provider_refuses_a_guess() {
        let mut tab = tab_with_claimed_session("codex");
        tab.agent_type = None;

        let outcome = apply_provider_identity(
            &mut tab,
            "claude".into(),
            true,
            AgentTypeWriteOrigin::Parser,
        );

        assert!(matches!(
            outcome,
            ProviderIdentityWrite::RejectedClaimedSession { current: None, .. }
        ));
        assert_eq!(tab.agent_type, None);
        assert!(!tab.agent_type_locked);
    }

    #[test]
    fn fresh_explicit_provider_spawn_accepts_provider() {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.session_binding_nonce = Some("fresh-launch".into());

        let outcome =
            apply_provider_identity(tab, "claude".into(), true, AgentTypeWriteOrigin::Spawn);

        assert!(matches!(outcome, ProviderIdentityWrite::Applied { .. }));
        assert_eq!(tab.agent_type.as_deref(), Some("claude"));
        assert!(tab.agent_type_locked);
    }

    #[test]
    fn send_provider_and_claimed_tab_provider_stay_on_one_record() {
        let mut tab = tab_with_claimed_session("codex");
        let send_provider_before = tab.agent_type.clone();

        apply_provider_identity(
            &mut tab,
            "claude".into(),
            true,
            AgentTypeWriteOrigin::Parser,
        );

        assert_eq!(send_provider_before, tab.agent_type);
        assert_eq!(tab.agent_session_id.as_deref(), Some("owned-session"));
    }

    #[test]
    fn verified_rebind_reasserts_provider_and_clears_parser_lock() {
        let mut tab = tab_with_claimed_session("claude");
        tab.agent_type_locked = true;

        let outcome = apply_provider_identity(
            &mut tab,
            "codex".into(),
            false,
            AgentTypeWriteOrigin::Rebind,
        );

        assert!(matches!(outcome, ProviderIdentityWrite::Applied { .. }));
        assert_eq!(tab.agent_type.as_deref(), Some("codex"));
        assert!(!tab.agent_type_locked);
    }

    #[test]
    fn restore_uses_the_unique_provider_owning_the_claimed_session() {
        let registry = crate::agent::registry::AgentRegistry::new();
        registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::with_session("codex", "owned-session"),
        ));
        registry.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
            "claude",
        )));

        let provider = provider_for_claimed_session(&registry, "D:/repo", "owned-session").unwrap();
        let mut tab = tab_with_claimed_session("claude");

        apply_provider_identity(&mut tab, provider, true, AgentTypeWriteOrigin::Restore);

        assert_eq!(tab.agent_type.as_deref(), Some("codex"));
        assert!(tab.agent_type_locked);
    }

    #[test]
    fn ambiguous_session_ownership_returns_null() {
        let registry = crate::agent::registry::AgentRegistry::new();
        for provider_id in ["codex", "claude"] {
            registry.register_provider(Box::new(
                crate::agent::test_provider::MockProvider::with_session(
                    provider_id,
                    "shared-session",
                ),
            ));
        }

        assert_eq!(
            provider_for_claimed_session(&registry, "D:/repo", "shared-session"),
            None
        );
    }

    #[test]
    fn restore_without_unique_session_evidence_clears_the_provider_guess() {
        let mut tab = tab_with_claimed_session("claude");

        let previous = clear_unproven_provider_identity(&mut tab);

        assert_eq!(previous.as_deref(), Some("claude"));
        assert!(tab.agent_type.is_none());
        assert_eq!(tab.agent_session_id.as_deref(), Some("owned-session"));
        assert!(!tab.agent_type_locked);
    }
}
