use crate::multiplexer::types::Tab;
use crate::session::lifecycle::AgentTypeWriteOrigin;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderIdentityEvidence {
    pub cwd: Option<String>,
    pub session_id: Option<String>,
    pub configured_nonce: Option<String>,
    pub reported_nonce: Option<String>,
}

impl ProviderIdentityEvidence {
    pub fn capture(tab: &Tab) -> Self {
        Self {
            cwd: tab.cwd.clone(),
            session_id: tab.agent_session_id.clone(),
            configured_nonce: tab.session_binding_nonce.clone(),
            reported_nonce: tab.reported_session_binding_nonce.clone(),
        }
    }

    pub fn nonce_verified(&self) -> bool {
        self.session_id.is_some()
            && self.configured_nonce.is_some()
            && self.configured_nonce == self.reported_nonce
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProviderIdentityWrite {
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

pub fn apply_provider_identity(
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

pub fn provider_for_claimed_session(
    registry: &crate::agent::registry::AgentRegistry,
    cwd: &str,
    session_id: &str,
) -> Option<String> {
    let mut owners = registry.provider_ids().into_iter().filter(|provider_id| {
        registry
            .with_provider(provider_id, |provider| {
                provider.session_exists(cwd, session_id) == Some(true)
            })
            .unwrap_or(false)
    });
    let owner = owners.next()?;
    owners.next().is_none().then_some(owner)
}

pub fn provider_for_restored_session(
    journal: &crate::session::lifecycle::JournalLoad,
    tab_id: &str,
    evidence: &ProviderIdentityEvidence,
) -> Option<String> {
    use crate::session::lifecycle::checkpoint_history;

    if !journal.tail.is_intact() {
        return None;
    }
    let session_id = evidence.session_id.as_deref()?;
    let binding_nonce = match (
        evidence.configured_nonce.as_deref(),
        evidence.reported_nonce.as_deref(),
    ) {
        (None, None) => None,
        (Some(configured), Some(reported)) if configured == reported => Some(configured),
        _ => return None,
    };
    let mut providers = checkpoint_history(journal, tab_id)
        .into_iter()
        .filter(|checkpoint| checkpoint.authority.session_id == session_id)
        .filter(|checkpoint| {
            binding_nonce.is_none_or(|nonce| checkpoint.authority.binding_nonce == nonce)
        })
        .map(|checkpoint| checkpoint.authority.provider);
    let provider = providers.next()?;
    providers
        .all(|candidate| candidate == provider)
        .then_some(provider)
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

    fn journal_checkpoint(
        tab_id: &str,
        provider: &str,
        session_id: &str,
        nonce: &str,
    ) -> crate::session::lifecycle::JournalRecord {
        use crate::session::lifecycle::{
            CommittedAuthority, JournalRecord, LifecycleCheckpoint, LifecycleEpoch, LifecycleKey,
        };

        JournalRecord::Checkpoint {
            checkpoint: LifecycleCheckpoint {
                key: LifecycleKey::new(tab_id, LifecycleEpoch(1), "launch"),
                committed_through: 4,
                authority: CommittedAuthority {
                    provider: provider.into(),
                    session_id: session_id.into(),
                    binding_nonce: nonce.into(),
                    role: Some("Manager".into()),
                    role_profile: Some("orchestrator".into()),
                    team_id: Some("team-pair".into()),
                    parent_tab_id: None,
                    can_spawn: true,
                    depth: 1,
                },
                agent_type_write: None,
                prior_checkpoints: Vec::new(),
                sealed_at: "2026-10-06T00:00:00Z".into(),
            },
        }
    }

    #[test]
    fn restored_provider_comes_from_the_tab_session_and_binding_checkpoint() {
        let mut tab = tab_with_claimed_session("stale-provider");
        tab.agent_session_id = Some("session-a".into());
        tab.session_binding_nonce = Some("nonce-a".into());
        tab.reported_session_binding_nonce = Some("nonce-a".into());
        let evidence = ProviderIdentityEvidence::capture(&tab);
        let journal = crate::session::lifecycle::JournalLoad {
            records: vec![journal_checkpoint("tab-a", "codex", "session-a", "nonce-a")],
            tail: crate::session::lifecycle::TailStatus::Intact,
            duplicate_seqs: 0,
        };

        assert_eq!(
            provider_for_restored_session(&journal, "tab-a", &evidence).as_deref(),
            Some("codex")
        );

        let mut legacy_tab = tab_with_claimed_session("stale-provider");
        legacy_tab.agent_session_id = Some("session-a".into());
        legacy_tab.session_binding_nonce = None;
        legacy_tab.reported_session_binding_nonce = None;
        let legacy_evidence = ProviderIdentityEvidence::capture(&legacy_tab);
        assert_eq!(
            provider_for_restored_session(&journal, "tab-a", &legacy_evidence).as_deref(),
            Some("codex")
        );
    }

    #[test]
    fn restored_provider_rejects_other_tabs_sessions_nonces_and_ambiguous_history() {
        let mut tab = tab_with_claimed_session("stale-provider");
        tab.agent_session_id = Some("session-a".into());
        tab.session_binding_nonce = Some("nonce-a".into());
        tab.reported_session_binding_nonce = Some("nonce-a".into());
        let evidence = ProviderIdentityEvidence::capture(&tab);
        let make_journal = |records| crate::session::lifecycle::JournalLoad {
            records,
            tail: crate::session::lifecycle::TailStatus::Intact,
            duplicate_seqs: 0,
        };

        let other_tab = make_journal(vec![journal_checkpoint(
            "tab-b",
            "codex",
            "session-a",
            "nonce-a",
        )]);
        let other_session = make_journal(vec![journal_checkpoint(
            "tab-a",
            "codex",
            "session-b",
            "nonce-a",
        )]);
        let other_nonce = make_journal(vec![journal_checkpoint(
            "tab-a",
            "codex",
            "session-a",
            "nonce-b",
        )]);
        let ambiguous = make_journal(vec![
            journal_checkpoint("tab-a", "codex", "session-a", "nonce-a"),
            journal_checkpoint("tab-a", "claude", "session-a", "nonce-a"),
        ]);

        for journal in [other_tab, other_session, other_nonce, ambiguous] {
            assert_eq!(
                provider_for_restored_session(&journal, "tab-a", &evidence),
                None
            );
        }

        let refused = crate::session::lifecycle::JournalLoad {
            records: vec![journal_checkpoint("tab-a", "codex", "session-a", "nonce-a")],
            tail: crate::session::lifecycle::TailStatus::Refused {
                line: 2,
                reason: "invalid record".into(),
            },
            duplicate_seqs: 0,
        };
        assert_eq!(
            provider_for_restored_session(&refused, "tab-a", &evidence),
            None
        );
    }
}
