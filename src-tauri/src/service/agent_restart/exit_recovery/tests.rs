use super::*;
use crate::session::lifecycle::{
    CommittedAuthority, LifecycleCheckpoint, LifecycleEntry, LifecycleEpoch, LifecycleKey,
};

fn key(tab_id: &str) -> LifecycleKey {
    LifecycleKey::new(tab_id, LifecycleEpoch::first(), "op-1")
}

fn bound(seq: u64, tab_id: &str, session_id: &str) -> JournalRecord {
    JournalRecord::Transition {
        entry: LifecycleEntry {
            seq,
            key: key(tab_id),
            transition: LifecycleTransition::SessionBound {
                session_id: session_id.into(),
                binding_nonce: format!("nonce-{seq}"),
                source: Default::default(),
            },
            recorded_at: String::new(),
        },
    }
}

fn launch_ready(seq: u64, tab_id: &str) -> JournalRecord {
    JournalRecord::Transition {
        entry: LifecycleEntry {
            seq,
            key: key(tab_id),
            transition: LifecycleTransition::LaunchReady,
            recorded_at: String::new(),
        },
    }
}

fn sealed(seq: u64, tab_id: &str, session_id: &str) -> JournalRecord {
    JournalRecord::Checkpoint {
        checkpoint: LifecycleCheckpoint {
            key: key(tab_id),
            committed_through: seq,
            authority: CommittedAuthority {
                provider: "codex".into(),
                session_id: session_id.into(),
                binding_nonce: format!("nonce-{seq}"),
                role: None,
                role_profile: None,
                team_id: None,
                parent_tab_id: None,
                can_spawn: false,
                depth: 0,
            },
            agent_type_write: None,
            prior_checkpoints: Vec::new(),
            sealed_at: String::new(),
        },
    }
}

fn lineage(entries: &[Option<&str>]) -> Vec<PriorBindingNonce> {
    entries
        .iter()
        .enumerate()
        .map(|(index, session_id)| PriorBindingNonce {
            session_id: session_id.map(str::to_string),
            nonce: format!("lineage-{index}"),
        })
        .collect()
}

fn refusal_message(result: Result<String, AgentRestartError>) -> String {
    match result {
        Err(AgentRestartError::BadRequest(message)) => message,
        other => panic!("expected a refusal before acceptance, got {other:?}"),
    }
}

fn verified(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| id.to_string()).collect()
}

#[test]
fn moved_on_tab_fails_the_atomic_pre_ready_recovery_guard() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.pty_id = Some("pty-old".into());
    tab.prior_session_binding_nonces = vec![PriorBindingNonce {
        session_id: Some("session-a".into()),
        nonce: "nonce-a".into(),
    }];
    let attempt = crate::service::launch_readiness::LaunchAttempt {
        tab_id: tab_id.clone(),
        pty_id: "pty-old".into(),
        binding_nonce: "nonce-a".into(),
    };
    assert!(launch_exit_tab_matches(tab, &attempt, false));

    tab.prior_session_binding_nonces.push(PriorBindingNonce {
        session_id: Some("session-b".into()),
        nonce: "nonce-b".into(),
    });
    assert!(!launch_exit_tab_matches(tab, &attempt, false));
    tab.prior_session_binding_nonces.pop();
    tab.pty_id = Some("pty-new".into());
    assert!(!launch_exit_tab_matches(tab, &attempt, false));
    tab.pty_id = Some("pty-old".into());
    tab.agent_booting = true;
    assert!(launch_exit_tab_matches(tab, &attempt, true));
    assert!(!launch_exit_tab_matches(tab, &attempt, false));
    tab.agent_booting = false;
    tab.agent_type = Some("codex".into());
    assert!(!launch_exit_tab_matches(tab, &attempt, false));
}

#[test]
fn journal_proves_only_this_tabs_verified_bindings() {
    let records = [
        bound(1, "tab-a", "s1"),
        bound(2, "tab-b", "foreign"),
        launch_ready(3, "tab-a"),
        sealed(4, "tab-a", "s2"),
    ];

    assert_eq!(
        verified_session_ids(&records, "tab-a"),
        verified(&["s1", "s2"])
    );
}

#[test]
fn compacted_journal_with_restore_awaiting_recovers_via_agent_restart() {
    let dir = tempfile::tempdir().unwrap();
    let journal = TabLifecycleJournal::for_dir(dir.path().to_path_buf());
    let first = LifecycleKey::new("tab-a", LifecycleEpoch(1), "restore-1");
    for transition in [
        LifecycleTransition::EpochOpened,
        LifecycleTransition::ProviderIdentityCommitted {
            provider: "codex".into(),
        },
        LifecycleTransition::SessionBound {
            session_id: "session-a".into(),
            binding_nonce: "nonce-a".into(),
            source: Default::default(),
        },
        LifecycleTransition::RoleBinding {
            role: None,
            role_profile: None,
            can_spawn: false,
            depth: 0,
        },
        LifecycleTransition::TeamBinding {
            team_id: None,
            parent_tab_id: None,
        },
        LifecycleTransition::LaunchReady,
    ] {
        journal.append(first.clone(), transition).unwrap();
    }
    let cleared = LifecycleKey::new("tab-a", LifecycleEpoch(2), "restore-2");
    journal
        .append(cleared.clone(), LifecycleTransition::EpochOpened)
        .unwrap();
    journal
        .append(
            cleared.clone(),
            LifecycleTransition::RolledBack {
                reason: "restored without a live agent".into(),
            },
        )
        .unwrap();
    journal
        .append(
            cleared,
            LifecycleTransition::RestoreAwaitingAgentRestart {
                session_id: "session-a".into(),
            },
        )
        .unwrap();
    journal.compact().unwrap();
    let compacted = journal.load().records;
    assert!(!compacted.iter().any(|record| matches!(
        record,
        JournalRecord::Checkpoint { checkpoint }
            if checkpoint.authority.session_id == "session-a"
    )));
    let verified_ids = verified_session_ids(&compacted, "tab-a");

    assert!(verified_ids.iter().any(|id| id == "session-a"));
    assert_eq!(
        select_recovery_session(&lineage(&[Some("session-a")]), &verified_ids).unwrap(),
        "session-a"
    );
    assert_eq!(
        recovery_provider(
            &registry(&[("codex", Some("session-a"))]),
            Some("D:/repo"),
            "session-a"
        )
        .unwrap(),
        "codex"
    );
}

#[test]
fn unknown_or_foreign_session_still_refuses_with_guidance() {
    let records = [
        bound(1, "other-tab", "foreign-session"),
        JournalRecord::Transition {
            entry: LifecycleEntry {
                seq: 2,
                key: key("tab-a"),
                transition: LifecycleTransition::RestoreAwaitingAgentRestart {
                    session_id: "foreign-session".into(),
                },
                recorded_at: String::new(),
            },
        },
    ];
    let verified = verified_session_ids(&records, "tab-a");
    let message = refusal_message(select_recovery_session(
        &lineage(&[Some("foreign-session")]),
        &verified,
    ));

    assert!(message.starts_with(EXIT_RECOVERY_NO_VERIFIED_SESSION));
    assert!(message.contains("spawn a fresh agent"));
}

#[test]
fn newest_lineage_session_is_recovered_when_the_journal_verified_it() {
    let session = select_recovery_session(
        &lineage(&[Some("older"), Some("s2")]),
        &verified(&["older", "s2"]),
    );

    assert_eq!(session.unwrap(), "s2");
}

#[test]
fn lineage_session_without_journal_proof_is_refused() {
    let message = refusal_message(select_recovery_session(
        &lineage(&[Some("unverified")]),
        &verified(&["s1"]),
    ));

    assert!(message.starts_with(EXIT_RECOVERY_NO_VERIFIED_SESSION));
    assert!(message.contains("unverified"));
}

#[test]
fn exited_launch_that_never_bound_does_not_fall_back_to_an_older_session() {
    let message = refusal_message(select_recovery_session(
        &lineage(&[Some("s1"), None]),
        &verified(&["s1"]),
    ));

    assert!(message.starts_with(EXIT_RECOVERY_NO_VERIFIED_SESSION));
}

#[test]
fn tab_without_lineage_recovers_its_newest_journaled_binding() {
    let session = select_recovery_session(&[], &verified(&["s1", "s2"]));

    assert_eq!(session.unwrap(), "s2");
}

#[test]
fn restore_contract_restored_lineage_recovers_the_session_that_exited_before_shutdown() {
    let (mut state, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = state
        .create_workspace("exit-before-restore".into(), None)
        .id;
    let tab_id = state.create_tab(&workspace).unwrap();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("codex".into());
    tab.agent_session_id = Some("exited".into());
    tab.session_binding_nonce = Some("nonce-exited".into());
    crate::service::tab::clear_agent_identity_for_lifecycle_exit(tab);

    let raw = serde_json::to_string(&state.to_session_snapshot()).unwrap();
    let restored = crate::multiplexer::state::MultiplexerState::from_session_snapshot(
        &serde_json::from_str(&raw).unwrap(),
    );
    let lineage = &restored
        .get_tab(&tab_id)
        .unwrap()
        .prior_session_binding_nonces;
    let journal = verified(&["exited", "newer-foreign-binding"]);

    assert_eq!(
        select_recovery_session(lineage, &journal).unwrap(),
        "exited"
    );
    assert_eq!(
        select_recovery_session(&[], &journal).unwrap(),
        "newer-foreign-binding",
        "without restored lineage recovery would fall back to the newest journal binding"
    );
}

#[test]
fn plain_shell_without_session_history_is_refused() {
    let message = refusal_message(select_recovery_session(&[], &[]));

    assert!(message.starts_with(EXIT_RECOVERY_NO_VERIFIED_SESSION));
}

fn registry(
    sessions: &[(&'static str, Option<&'static str>)],
) -> crate::agent::registry::AgentRegistry {
    let registry = crate::agent::registry::AgentRegistry::new();
    for (provider_id, session_id) in sessions {
        let provider = match session_id {
            Some(session_id) => {
                crate::agent::test_provider::MockProvider::with_session(provider_id, session_id)
            }
            None => crate::agent::test_provider::MockProvider::new(provider_id),
        };
        registry.register_provider(Box::new(provider));
    }
    registry
}

#[test]
fn recovery_resolves_the_provider_owning_that_exact_session() {
    let registry = registry(&[("codex", Some("owned")), ("claude", None)]);

    assert_eq!(
        recovery_provider(&registry, Some("D:/repo"), "owned").unwrap(),
        "codex"
    );
}

#[test]
fn recovery_refuses_unowned_ambiguous_or_unlocatable_sessions() {
    let registry = registry(&[("codex", Some("shared")), ("claude", Some("shared"))]);

    for (cwd, session_id) in [
        (Some("D:/repo"), "shared"),
        (Some("D:/repo"), "missing"),
        (None, "shared"),
    ] {
        let message = refusal_message(recovery_provider(&registry, cwd, session_id));
        assert!(message.starts_with(EXIT_RECOVERY_SESSION_NOT_OWNED));
        assert!(message.contains(session_id));
    }
}

#[test]
fn in_place_readiness_is_the_relaunch_reaching_agent_ready() {
    let ready = crate::service::boot_stage::classify_restart_readiness(
        Some("pty-shell"),
        Some(BootStage::AgentReady),
        None,
        None,
    );
    assert_eq!(
        ready,
        Some(crate::service::boot_stage::RestartReadiness::Ready {
            pty_id: "pty-shell".into(),
        })
    );

    for retired in [BootStage::PtyStarting, BootStage::ShellReady] {
        assert_eq!(
            crate::service::boot_stage::classify_restart_readiness(
                Some("pty-shell"),
                Some(retired),
                None,
                None,
            ),
            None
        );
    }
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn plain_shell_without_a_live_pty_is_refused_before_acceptance() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let workspace = fixture
        .service
        .mux
        .write()
        .create_workspace("exit-recovery".into(), None)
        .id;
    let tab_id = fixture.service.mux.write().create_tab(&workspace).unwrap();

    let result = fixture.service.agent_restart_observed(&tab_id, false, 1);

    match result {
        Err(RestartAttemptError::Request(AgentRestartError::BadRequest(message))) => {
            assert!(message.starts_with(EXIT_RECOVERY_NO_LIVE_SHELL));
        }
        other => panic!("expected a refusal before acceptance, got {other:?}"),
    }
    assert!(
        fixture
            .service
            .restart_tracker
            .lock()
            .last_restart
            .get(&tab_id)
            .is_none(),
        "a refused recovery must not consume the restart rate limit"
    );
    let tab = fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .cloned()
        .unwrap();
    assert_eq!(tab.agent_type, None);
    assert!(!tab.agent_booting);
}

/// The common live order: readiness seals first, the hook binding lands after it.
fn journal_binding_after_readiness(
    journal: &TabLifecycleJournal,
    tab_id: &str,
    provider: &str,
    session_id: &str,
) {
    let key = LifecycleKey::new(tab_id, journal.next_epoch_for_tab(tab_id), "launch");
    for transition in [
        LifecycleTransition::EpochOpened,
        LifecycleTransition::ProviderIdentityCommitted {
            provider: provider.into(),
        },
        LifecycleTransition::LaunchReady,
        LifecycleTransition::SessionBound {
            session_id: session_id.into(),
            binding_nonce: format!("nonce-{session_id}"),
            source: Default::default(),
        },
    ] {
        journal.append(key.clone(), transition).unwrap();
    }
}

fn exited_tab(
    state: &mut crate::multiplexer::state::MultiplexerState,
    provider: &str,
    session_id: &str,
) -> String {
    let workspace = state.create_workspace("exit-recovery".into(), None).id;
    let tab_id = state.create_tab(&workspace).unwrap();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some(provider.into());
    tab.agent_session_id = Some(session_id.into());
    tab.session_binding_nonce = Some(format!("nonce-{session_id}"));
    tab.reported_session_binding_nonce = Some(format!("nonce-{session_id}"));
    tab_id
}

#[test]
fn exit_clear_keeps_verified_binding_in_lineage() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = crate::multiplexer::state::MultiplexerState::new();
    let tab_id = exited_tab(&mut state, "codex", "session-a");
    {
        let journal = TabLifecycleJournal::for_dir(dir.path().to_path_buf());
        journal_binding_after_readiness(&journal, &tab_id, "codex", "session-a");
        journal.compact().unwrap();
        journal.compact().unwrap();
    }

    let tab = state.get_tab_mut(&tab_id).unwrap();
    assert!(crate::service::tab::clear_agent_identity_for_lifecycle_exit(tab));
    assert_eq!(tab.session_binding_nonce, None);
    assert_eq!(
        crate::multiplexer::types::latest_prior_session(&tab.prior_session_binding_nonces),
        Some("session-a")
    );

    let reopened = TabLifecycleJournal::for_dir(dir.path().to_path_buf());
    reopened
        .append(
            LifecycleKey::new(&tab_id, LifecycleEpoch::first(), "launch"),
            LifecycleTransition::RolledBack {
                reason: "provider process exited to a surviving shell".into(),
            },
        )
        .unwrap();
    reopened.compact().unwrap();
    let load = reopened.load();
    assert!(load.tail.is_intact(), "compaction left an unreadable tail");
    let verified_ids = verified_session_ids(&load.records, &tab_id);

    assert_eq!(
        select_recovery_session(&tab.prior_session_binding_nonces, &verified_ids).unwrap(),
        "session-a"
    );
}

#[test]
fn exit_recovery_resumes_roleless_and_team_tabs() {
    let dir = tempfile::tempdir().unwrap();
    let journal = TabLifecycleJournal::for_dir(dir.path().to_path_buf());
    let (mut state, _) = crate::multiplexer::state::MultiplexerState::new();
    let cases = [
        ("provider-a", "roleless-a", false),
        ("provider-a", "member-a", true),
        ("provider-b", "roleless-b", false),
        ("provider-b", "member-b", true),
    ];
    let mut tabs = Vec::new();
    for (provider, session_id, team_member) in cases {
        let tab_id = exited_tab(&mut state, provider, session_id);
        if team_member {
            let tab = state.get_tab_mut(&tab_id).unwrap();
            tab.role = Some(crate::multiplexer::types::AgentRole::Worker);
            tab.team_id = Some("team-exit".into());
        }
        journal_binding_after_readiness(&journal, &tab_id, provider, session_id);
        tabs.push((tab_id, provider, session_id));
    }
    journal.compact().unwrap();
    let registry = registry(&[
        ("provider-a", Some("roleless-a")),
        ("provider-b", Some("roleless-b")),
    ]);

    for (tab_id, provider, session_id) in &tabs {
        let tab = state.get_tab_mut(tab_id).unwrap();
        assert!(crate::service::tab::clear_agent_identity_for_lifecycle_exit(tab));
        let verified_ids = verified_session_ids(&journal.load().records, tab_id);
        let recovered =
            select_recovery_session(&tab.prior_session_binding_nonces, &verified_ids).unwrap();
        assert_eq!(
            recovered, *session_id,
            "tab {tab_id} lost its verified session"
        );
        if session_id.starts_with("roleless") {
            assert_eq!(
                recovery_provider(&registry, Some("D:/repo"), &recovered).unwrap(),
                *provider
            );
        }
    }
}
