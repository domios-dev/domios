use super::ServiceContext;
use crate::agent::provider_identity::{
    apply_provider_identity, clear_unproven_provider_identity, ProviderIdentityEvidence,
    ProviderIdentityWrite,
};
use crate::session::lifecycle::{AgentTypeWriteOrigin, LifecycleTransition};

struct WriteEvidence {
    caller_tab_id: Option<String>,
    identity: ProviderIdentityEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TabAgentTypeState {
    pub(crate) provider: Option<String>,
    pub(crate) locked: bool,
}

impl TabAgentTypeState {
    fn capture(tab: &crate::multiplexer::types::Tab) -> Self {
        Self {
            provider: tab.agent_type.clone(),
            locked: tab.agent_type_locked,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentTypeWriteOutcome {
    Applied {
        previous: TabAgentTypeState,
        current: TabAgentTypeState,
    },
    Kept {
        current: TabAgentTypeState,
    },
    EvidenceChanged {
        expected: ProviderIdentityEvidence,
        current_evidence: ProviderIdentityEvidence,
        current: TabAgentTypeState,
    },
    RejectedClaimedSession {
        current: TabAgentTypeState,
    },
}

#[derive(Debug)]
pub(crate) enum ConditionalAgentTypeWrite<T> {
    Committed {
        value: T,
        identity_write: AgentTypeWriteOutcome,
    },
    EvidenceChanged {
        identity_write: AgentTypeWriteOutcome,
    },
}

struct IdentityMutation {
    outcome: AgentTypeWriteOutcome,
    previous_provider: Option<String>,
    new_provider: Option<String>,
    applied: bool,
}

fn apply_guarded_identity_mutation(
    tab: &mut crate::multiplexer::types::Tab,
    agent_type: String,
    lock: bool,
    origin: AgentTypeWriteOrigin,
    expected_evidence: Option<&ProviderIdentityEvidence>,
) -> IdentityMutation {
    let previous = TabAgentTypeState::capture(tab);
    let current_evidence = ProviderIdentityEvidence::capture(tab);
    if expected_evidence.is_some_and(|expected| expected != &current_evidence) {
        return IdentityMutation {
            outcome: AgentTypeWriteOutcome::EvidenceChanged {
                expected: expected_evidence
                    .cloned()
                    .unwrap_or(current_evidence.clone()),
                current_evidence,
                current: previous.clone(),
            },
            previous_provider: previous.provider.clone(),
            new_provider: previous.provider,
            applied: false,
        };
    }

    let transition = apply_provider_identity(tab, agent_type, lock, origin);
    let current = TabAgentTypeState::capture(tab);
    let applied = matches!(transition, ProviderIdentityWrite::Applied { .. });
    let outcome = match transition {
        ProviderIdentityWrite::Applied { .. } if previous == current => {
            AgentTypeWriteOutcome::Kept {
                current: current.clone(),
            }
        }
        ProviderIdentityWrite::Applied { .. } => AgentTypeWriteOutcome::Applied {
            previous: previous.clone(),
            current: current.clone(),
        },
        ProviderIdentityWrite::RejectedClaimedSession { .. } => {
            AgentTypeWriteOutcome::RejectedClaimedSession {
                current: current.clone(),
            }
        }
    };
    IdentityMutation {
        outcome,
        previous_provider: previous.provider,
        new_provider: current.provider,
        applied,
    }
}

fn write_evidence(
    tab: &crate::multiplexer::types::Tab,
    origin: AgentTypeWriteOrigin,
    caller_tab_id: Option<&str>,
) -> WriteEvidence {
    WriteEvidence {
        caller_tab_id: caller_tab_id.map(str::to_string).or_else(|| {
            (origin == AgentTypeWriteOrigin::Spawn)
                .then(|| tab.spawned_by_tab_id.clone())
                .flatten()
        }),
        identity: ProviderIdentityEvidence::capture(tab),
    }
}

fn agent_type_write_transition(
    origin: AgentTypeWriteOrigin,
    previous_provider: Option<String>,
    new_provider: Option<String>,
    attempted_provider: Option<String>,
    evidence: WriteEvidence,
    applied: bool,
) -> LifecycleTransition {
    let nonce_verified = evidence.identity.nonce_verified();
    LifecycleTransition::AgentTypeWrite {
        origin,
        previous_provider,
        new_provider,
        attempted_provider,
        caller_tab_id: evidence.caller_tab_id,
        session_id: evidence.identity.session_id,
        nonce_verified,
        applied,
    }
}

impl ServiceContext {
    pub fn set_tab_agent_type(&self, tab_id: &str, agent_type: Option<String>) {
        match agent_type {
            Some(agent_type) => {
                self.write_tab_agent_type_with_origin(
                    tab_id,
                    agent_type,
                    false,
                    AgentTypeWriteOrigin::Ui,
                    None,
                );
            }
            None => {
                self.clear_tab_agent_identity_for_lifecycle_exit(tab_id);
            }
        }
    }

    pub fn set_and_lock_tab_agent_type(&self, tab_id: &str, agent_type: String) {
        self.write_tab_agent_type_with_origin(
            tab_id,
            agent_type,
            true,
            AgentTypeWriteOrigin::Parser,
            None,
        );
    }

    pub(crate) fn set_and_lock_tab_agent_type_from(
        &self,
        tab_id: &str,
        agent_type: String,
        origin: AgentTypeWriteOrigin,
        caller_tab_id: Option<&str>,
    ) {
        self.write_tab_agent_type_with_origin(tab_id, agent_type, true, origin, caller_tab_id);
    }

    pub(crate) fn write_tab_agent_type_with_origin(
        &self,
        tab_id: &str,
        agent_type: String,
        lock: bool,
        origin: AgentTypeWriteOrigin,
        caller_tab_id: Option<&str>,
    ) -> Option<AgentTypeWriteOutcome> {
        self.write_tab_agent_type_guarded(tab_id, agent_type, lock, origin, caller_tab_id, None)
    }

    pub(crate) fn write_verified_tab_agent_type_with_origin(
        &self,
        tab_id: &str,
        agent_type: String,
        origin: AgentTypeWriteOrigin,
        caller_tab_id: Option<&str>,
        expected_evidence: &ProviderIdentityEvidence,
    ) -> Option<AgentTypeWriteOutcome> {
        if !matches!(
            origin,
            AgentTypeWriteOrigin::Rebind | AgentTypeWriteOrigin::Repair
        ) {
            return None;
        }
        let outcome = self.write_tab_agent_type_guarded(
            tab_id,
            agent_type.clone(),
            false,
            origin,
            caller_tab_id,
            Some(expected_evidence),
        )?;
        if matches!(
            &outcome,
            AgentTypeWriteOutcome::Applied { .. } | AgentTypeWriteOutcome::Kept { .. }
        ) {
            self.record_provider_authority_from_evidence(tab_id, &agent_type, expected_evidence);
        }
        Some(outcome)
    }

    pub(crate) fn write_hook_tab_agent_type_from_claim(
        &self,
        tab_id: &str,
        agent_type: String,
        caller_tab_id: Option<&str>,
        expected_evidence: &ProviderIdentityEvidence,
    ) -> Option<AgentTypeWriteOutcome> {
        self.write_tab_agent_type_guarded(
            tab_id,
            agent_type,
            true,
            AgentTypeWriteOrigin::Hook,
            caller_tab_id,
            Some(expected_evidence),
        )
    }

    pub(crate) fn commit_verified_tab_agent_type_with_origin<T, E>(
        &self,
        tab_id: &str,
        agent_type: String,
        origin: AgentTypeWriteOrigin,
        caller_tab_id: Option<&str>,
        expected_evidence: &ProviderIdentityEvidence,
        commit: impl FnOnce(&mut crate::multiplexer::state::MultiplexerState) -> Result<T, E>,
    ) -> Result<Option<ConditionalAgentTypeWrite<T>>, E> {
        if !matches!(
            origin,
            AgentTypeWriteOrigin::Rebind | AgentTypeWriteOrigin::Repair
        ) {
            return Ok(None);
        }
        let attempted_provider = agent_type.clone();
        let (result, evidence, mutation) = {
            let mut mux = self.mux.write();
            let Some(tab) = mux.get_tab(tab_id) else {
                return Ok(None);
            };
            let current_evidence = ProviderIdentityEvidence::capture(tab);
            if &current_evidence != expected_evidence {
                let current = TabAgentTypeState::capture(tab);
                let mutation = IdentityMutation {
                    outcome: AgentTypeWriteOutcome::EvidenceChanged {
                        expected: expected_evidence.clone(),
                        current_evidence,
                        current: current.clone(),
                    },
                    previous_provider: current.provider.clone(),
                    new_provider: current.provider,
                    applied: false,
                };
                let evidence = write_evidence(tab, origin, caller_tab_id);
                (None, evidence, mutation)
            } else {
                let value = commit(&mut mux)?;
                let tab = mux
                    .get_tab_mut(tab_id)
                    .expect("verified identity commit must preserve its target tab");
                let evidence = write_evidence(tab, origin, caller_tab_id);
                let mutation =
                    apply_guarded_identity_mutation(tab, agent_type, false, origin, None);
                (Some(value), evidence, mutation)
            }
        };
        let authority_evidence = evidence.identity.clone();
        let outcome = self.finish_agent_type_write(
            tab_id,
            origin,
            attempted_provider.clone(),
            evidence,
            mutation,
        );
        if result.is_some()
            && matches!(
                &outcome,
                AgentTypeWriteOutcome::Applied { .. } | AgentTypeWriteOutcome::Kept { .. }
            )
        {
            self.record_provider_authority_from_evidence(
                tab_id,
                &attempted_provider,
                &authority_evidence,
            );
        }
        Ok(Some(match result {
            Some(value) => ConditionalAgentTypeWrite::Committed {
                value,
                identity_write: outcome,
            },
            None => ConditionalAgentTypeWrite::EvidenceChanged {
                identity_write: outcome,
            },
        }))
    }

    fn write_tab_agent_type_guarded(
        &self,
        tab_id: &str,
        agent_type: String,
        lock: bool,
        origin: AgentTypeWriteOrigin,
        caller_tab_id: Option<&str>,
        expected_evidence: Option<&ProviderIdentityEvidence>,
    ) -> Option<AgentTypeWriteOutcome> {
        let attempted_provider = agent_type.clone();
        let (outcome, evidence, previous_provider, new_provider, applied) = {
            let mut mux = self.mux.write();
            let tab = mux.get_tab_mut(tab_id)?;
            let evidence = write_evidence(tab, origin, caller_tab_id);
            let mutation =
                apply_guarded_identity_mutation(tab, agent_type, lock, origin, expected_evidence);
            (
                mutation.outcome,
                evidence,
                mutation.previous_provider,
                mutation.new_provider,
                mutation.applied,
            )
        };
        Some(self.finish_agent_type_write(
            tab_id,
            origin,
            attempted_provider,
            evidence,
            IdentityMutation {
                outcome,
                previous_provider,
                new_provider,
                applied,
            },
        ))
    }

    fn finish_agent_type_write(
        &self,
        tab_id: &str,
        origin: AgentTypeWriteOrigin,
        attempted_provider: String,
        evidence: WriteEvidence,
        mutation: IdentityMutation,
    ) -> AgentTypeWriteOutcome {
        let IdentityMutation {
            outcome,
            previous_provider,
            new_provider,
            applied,
        } = mutation;
        if applied {
            log::info!(
                "[set_tab_agent_type] tab={} origin={:?} prev={:?} new={:?}",
                tab_id,
                origin,
                previous_provider,
                new_provider,
            );
            if previous_provider.is_none() {
                if let Some(current) = new_provider.as_deref() {
                    self.replay_inbound_buffer(tab_id, current);
                }
            }
            if previous_provider != new_provider {
                if let Some(current) = new_provider.as_deref() {
                    self.kick_off_session_name_detection(tab_id, current);
                    self.refresh_session_model_once(tab_id, current);
                }
            }
        } else {
            match &outcome {
                AgentTypeWriteOutcome::EvidenceChanged { .. } => {
                    log::warn!(
                        "[set_tab_agent_type] tab={} origin={:?} rejected={}; session evidence changed",
                        tab_id,
                        origin,
                        attempted_provider,
                    );
                }
                AgentTypeWriteOutcome::RejectedClaimedSession { .. } => {
                    log::warn!(
                        "[set_tab_agent_type] tab={} origin={:?} preserved={:?}; rejected={}",
                        tab_id,
                        origin,
                        new_provider,
                        attempted_provider,
                    );
                }
                AgentTypeWriteOutcome::Applied { .. } | AgentTypeWriteOutcome::Kept { .. } => {}
            }
        }
        self.record_agent_type_write(
            tab_id,
            agent_type_write_transition(
                origin,
                previous_provider,
                new_provider,
                Some(attempted_provider),
                evidence,
                applied,
            ),
        );
        outcome
    }

    pub fn unlock_tab_agent_type(&self, tab_id: &str) {
        if let Some(tab) = self.mux.write().get_tab_mut(tab_id) {
            tab.agent_type_locked = false;
        }
    }

    fn record_provider_authority_from_evidence(
        &self,
        tab_id: &str,
        provider: &str,
        evidence: &ProviderIdentityEvidence,
    ) {
        if !evidence.nonce_verified() {
            return;
        }
        let Some(session_id) = evidence.session_id.clone() else {
            return;
        };
        let Some(binding_nonce) = evidence.configured_nonce.clone() else {
            return;
        };
        self.record_lifecycle(
            tab_id,
            LifecycleTransition::ProviderIdentityCommitted {
                provider: provider.to_string(),
            },
        );
        self.record_lifecycle(
            tab_id,
            LifecycleTransition::SessionBound {
                session_id,
                binding_nonce,
            },
        );
    }

    pub(crate) fn clear_unproven_restored_provider(&self, tab_id: &str) {
        let (previous_provider, evidence) = {
            let mut mux = self.mux.write();
            let Some(tab) = mux.get_tab_mut(tab_id) else {
                return;
            };
            let evidence = write_evidence(tab, AgentTypeWriteOrigin::Restore, None);
            (clear_unproven_provider_identity(tab), evidence)
        };
        if previous_provider.is_none() {
            return;
        }
        let nonce_verified = evidence.identity.nonce_verified();
        self.record_agent_type_write(
            tab_id,
            LifecycleTransition::AgentTypeWrite {
                origin: AgentTypeWriteOrigin::Restore,
                previous_provider,
                new_provider: None,
                attempted_provider: None,
                caller_tab_id: evidence.caller_tab_id,
                session_id: evidence.identity.session_id,
                nonce_verified,
                applied: true,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::provider_identity::provider_for_claimed_session;
    use crate::service::test_fixture::ServiceFixture;

    fn claimed_tab(provider: &str) -> crate::multiplexer::types::Tab {
        let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("D:/repo".into());
        tab.agent_type = Some(provider.into());
        tab.agent_type_locked = true;
        tab.agent_session_id = Some("owned-session".into());
        tab.session_binding_nonce = Some("binding-nonce".into());
        tab.reported_session_binding_nonce = Some("binding-nonce".into());
        tab.clone()
    }

    #[test]
    fn verified_write_rejects_a_changed_evidence_generation() {
        let mut tab = claimed_tab("claude");
        let expected = ProviderIdentityEvidence::capture(&tab);
        tab.cwd = Some("D:/other".into());

        let mutation = apply_guarded_identity_mutation(
            &mut tab,
            "codex".into(),
            false,
            AgentTypeWriteOrigin::Repair,
            Some(&expected),
        );

        assert!(matches!(
            mutation.outcome,
            AgentTypeWriteOutcome::EvidenceChanged { .. }
        ));
        assert_eq!(tab.agent_type.as_deref(), Some("claude"));
        assert!(tab.agent_type_locked);
    }

    #[test]
    fn verified_repair_reports_applied_then_kept() {
        let mut tab = claimed_tab("claude");
        let evidence = ProviderIdentityEvidence::capture(&tab);

        let applied = apply_guarded_identity_mutation(
            &mut tab,
            "codex".into(),
            false,
            AgentTypeWriteOrigin::Repair,
            Some(&evidence),
        );
        assert!(matches!(
            applied.outcome,
            AgentTypeWriteOutcome::Applied { .. }
        ));
        assert_eq!(tab.agent_type.as_deref(), Some("codex"));
        assert!(!tab.agent_type_locked);

        let evidence = ProviderIdentityEvidence::capture(&tab);
        let kept = apply_guarded_identity_mutation(
            &mut tab,
            "codex".into(),
            false,
            AgentTypeWriteOrigin::Repair,
            Some(&evidence),
        );
        assert!(matches!(kept.outcome, AgentTypeWriteOutcome::Kept { .. }));
    }

    #[test]
    #[cfg_attr(
        not(windows),
        ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
    )]
    fn hook_identity_write_applies_verified_claim() {
        let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let evidence = {
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.cwd = Some("D:/repo".into());
            tab.agent_session_id = Some("claimed-session".into());
            tab.session_binding_nonce = Some("binding-nonce".into());
            tab.reported_session_binding_nonce = Some("binding-nonce".into());
            ProviderIdentityEvidence::capture(tab)
        };
        let fixture = ServiceFixture::new(mux);
        assert!(evidence.nonce_verified());

        let outcome = fixture.service.write_hook_tab_agent_type_from_claim(
            &tab_id,
            "claude".into(),
            Some(&tab_id),
            &evidence,
        );

        assert!(matches!(
            outcome,
            Some(AgentTypeWriteOutcome::Applied {
                previous: TabAgentTypeState {
                    provider: None,
                    locked: false,
                },
                current: TabAgentTypeState {
                    provider: Some(ref provider),
                    locked: true,
                },
            }) if provider == "claude"
        ));
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(tab.agent_type.as_deref(), Some("claude"));
        assert!(tab.agent_type_locked);
    }

    #[test]
    #[cfg_attr(
        not(windows),
        ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
    )]
    fn hook_identity_write_refuses_stale_claim_evidence() {
        let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let evidence = {
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.cwd = Some("D:/repo".into());
            tab.agent_type = Some("claude".into());
            tab.agent_type_locked = true;
            tab.agent_session_id = Some("claimed-session".into());
            tab.session_binding_nonce = Some("binding-nonce".into());
            tab.reported_session_binding_nonce = Some("binding-nonce".into());
            ProviderIdentityEvidence::capture(tab)
        };
        let fixture = ServiceFixture::new(mux);
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&tab_id)
            .unwrap()
            .agent_session_id = Some("replacement-session".into());

        let outcome = fixture.service.write_hook_tab_agent_type_from_claim(
            &tab_id,
            "codex".into(),
            Some(&tab_id),
            &evidence,
        );

        assert!(matches!(
            outcome,
            Some(AgentTypeWriteOutcome::EvidenceChanged { .. })
        ));
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(tab.agent_type.as_deref(), Some("claude"));
        assert!(tab.agent_type_locked);
        assert_eq!(tab.agent_session_id.as_deref(), Some("replacement-session"));
    }

    #[test]
    fn verified_repair_boundary_records_complete_journal_provenance() {
        let mut tab = claimed_tab("claude");
        let evidence = write_evidence(&tab, AgentTypeWriteOrigin::Repair, Some("repair-tab"));
        let expected = evidence.identity.clone();
        let mutation = apply_guarded_identity_mutation(
            &mut tab,
            "codex".into(),
            false,
            AgentTypeWriteOrigin::Repair,
            Some(&expected),
        );
        let transition = agent_type_write_transition(
            AgentTypeWriteOrigin::Repair,
            mutation.previous_provider,
            mutation.new_provider,
            Some("codex".into()),
            evidence,
            mutation.applied,
        );
        let directory = tempfile::tempdir().unwrap();
        let journal =
            crate::session::lifecycle::TabLifecycleJournal::for_dir(directory.path().to_path_buf());
        journal
            .append(
                crate::session::lifecycle::LifecycleKey::new(
                    "target-tab",
                    crate::session::lifecycle::LifecycleEpoch::first(),
                    "repair-operation",
                ),
                transition,
            )
            .unwrap();

        assert!(journal.load().records.iter().any(|record| matches!(
            record,
            crate::session::lifecycle::JournalRecord::Transition { entry }
                if matches!(
                    &entry.transition,
                    LifecycleTransition::AgentTypeWrite {
                        origin: AgentTypeWriteOrigin::Repair,
                        previous_provider: Some(previous),
                        new_provider: Some(new),
                        attempted_provider: Some(attempted),
                        caller_tab_id: Some(caller),
                        session_id: Some(session),
                        nonce_verified: true,
                        applied: true,
                        ..
                    } if previous == "claude"
                        && new == "codex"
                        && attempted == "codex"
                        && caller == "repair-tab"
                        && session == "owned-session"
                )
        )));
    }
}
