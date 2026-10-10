use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, CloseRestoreTabState, RestorableLaunchParameters};
use crate::session::lifecycle::{LifecycleEpoch, LifecycleKey};

#[test]
fn queued_unwritten_turns_are_not_conversations_but_claimed_turns_are_unknown() {
    use crate::service::pending_delivery_payloads::user_sends::UserSendState;
    assert!(!unresolved_user_turn([]));
    assert!(!unresolved_user_turn([UserSendState::Queued]));
    for state in [
        UserSendState::Claimed,
        UserSendState::Uncertain,
        UserSendState::BodyLease,
    ] {
        assert!(unresolved_user_turn([UserSendState::Queued, state]));
    }
}

#[test]
fn missing_session_classifier_requires_definite_empty_evidence() {
    assert_eq!(
        classify_missing_session(Some(false), Some(false), Some(false), true),
        MissingSessionAction::Fresh
    );
    assert_eq!(
        classify_missing_session(Some(false), Some(true), Some(false), true),
        MissingSessionAction::ConversationMissing
    );
    assert_eq!(
        classify_missing_session(Some(false), Some(false), Some(true), true),
        MissingSessionAction::ConversationMissing
    );
    for (exists, journal, provider, captured) in [
        (None, Some(false), Some(false), true),
        (Some(true), Some(false), Some(false), true),
        (Some(false), None, Some(false), true),
        (Some(false), Some(false), None, true),
        (Some(false), Some(false), Some(false), false),
    ] {
        assert_eq!(
            classify_missing_session(exists, journal, provider, captured),
            MissingSessionAction::EvidenceUnknown
        );
    }
}

#[test]
fn first_delivered_turn_is_durable_once_and_survives_compaction_and_close() {
    let temp = tempfile::tempdir().unwrap();
    let journal = TabLifecycleJournal::for_dir(temp.path().into());
    let key = LifecycleKey::new("tab", LifecycleEpoch(1), "launch");
    journal
        .append(key.clone(), LifecycleTransition::EpochOpened)
        .unwrap();
    assert_eq!(
        journal_conversation(&journal.load(), "tab", "saved"),
        Some(false)
    );
    journal
        .note_conversation_started(key.clone(), "saved")
        .unwrap();
    journal
        .note_conversation_started(key.clone(), "saved")
        .unwrap();
    assert_eq!(journal.load().records.len(), 2);
    journal
        .append(
            key,
            LifecycleTransition::Closed {
                reason: "fixture".into(),
            },
        )
        .unwrap();
    journal.compact().unwrap();
    let reopened = TabLifecycleJournal::for_dir(temp.path().into());
    assert_eq!(
        journal_conversation(&reopened.load(), "tab", "saved"),
        Some(true)
    );
    assert_eq!(
        journal_conversation(&reopened.load(), "foreign", "saved"),
        Some(false)
    );
    assert_eq!(
        journal_conversation(&JournalLoad::empty(), "tab", "saved"),
        Some(false)
    );
}

#[test]
fn empty_restore_relaunch_spec_equals_previous_except_session_binding() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some("saved".into());
    tab.title = "Volta".into();
    tab.cwd = Some("/recorded-cwd".into());
    tab.role = Some(AgentRole::Manager);
    tab.role_profile = Some("orchestrator".into());
    tab.team_id = Some("recorded-team".into());
    tab.task_binding = Some(crate::multiplexer::types::TaskBinding {
        plan_id: Some("recorded-plan".into()),
        task_id: Some("recorded-task".into()),
        ..Default::default()
    });
    tab.restored_from_snapshot = true;
    tab.close_restore = Some(CloseRestoreTabState {
        exact_session: true,
        ..Default::default()
    });
    tab.restorable_launch = RestorableLaunchParameters {
        captured: true,
        model: Some("recorded-model".into()),
        requested_model: Some("recorded-model".into()),
        requested_reasoning_effort: Some("recorded-effort".into()),
        resolved_reasoning_effort: Some("recorded-effort".into()),
        structured_runtime_mode: Some("recorded-mode".into()),
        identity_instructions: Some("opaque-launch-prompt".into()),
        args: Some(vec!["recorded-argument".into()]),
        ..Default::default()
    };
    let mut expected = tab.clone();
    expected.agent_session_id = None;
    expected.close_restore.as_mut().unwrap().exact_session = false;
    clear_empty_session(tab);
    assert_eq!(
        serde_json::to_value(&*tab).unwrap(),
        serde_json::to_value(expected).unwrap()
    );

    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("fixture-provider")
            .with_session_config_delivery(),
    ));
    let resume = super::super::restored_snapshot_launch_input(
        &registry,
        super::super::RestoredLaunch {
            provider_id: tab.agent_type.clone().unwrap(),
            session_id: Some("saved".into()),
            cwd: tab.cwd.clone(),
            model: tab.restorable_launch.model.clone(),
            launch_parameters: tab.restorable_launch.clone(),
            role: Some("Manager".into()),
            role_profile: tab.role_profile.clone(),
        },
        None,
        None,
    );
    let fresh = super::super::restored_snapshot_launch_input(
        &registry,
        super::super::restored_launch(tab, "fixture-provider".into()),
        None,
        None,
    );
    assert_eq!(fresh.launch_parameters, tab.restorable_launch);
    assert_eq!(fresh.launch_parameters, resume.launch_parameters);
    let mut previous_params = resume.params.clone();
    previous_params.resume_session_id = None;
    assert_eq!(
        format!("{:?}", fresh.params),
        format!("{:?}", previous_params)
    );
    assert_eq!(fresh.params.agent, resume.params.agent);
    assert_eq!(fresh.params.cwd, resume.params.cwd);
    assert_eq!(fresh.params.model, resume.params.model);
    assert_eq!(
        fresh.params.reasoning_effort,
        resume.params.reasoning_effort
    );
    assert_eq!(
        fresh.params.skip_permissions,
        resume.params.skip_permissions
    );
    assert_eq!(fresh.params.role, resume.params.role);
    assert_eq!(fresh.params.role_profile, resume.params.role_profile);
    assert_eq!(fresh.params.args, resume.params.args);
    assert_eq!(fresh.params.system_prompt, resume.params.system_prompt);
    assert_eq!(fresh.launch_task, resume.launch_task);
    assert_eq!(fresh.params.resume_session_id, None);
    assert_eq!(resume.params.resume_session_id.as_deref(), Some("saved"));
}

#[test]
fn secondary_evidence_reads_only_the_exact_transcript_and_rejects_read_errors() {
    let temp = tempfile::tempdir().unwrap();
    let provider = crate::plugin::agent_claude::ClaudeProvider::new();
    let path = temp.path().join("own-session.jsonl");
    assert_eq!(file_conversation(&provider, &path), Some(false));
    std::fs::write(
        &path,
        r#"{"type":"user","message":{"role":"user","content":"opaque-turn"}}"#,
    )
    .unwrap();
    assert_eq!(file_conversation(&provider, &path), Some(true));
    std::fs::write(&path, "malformed-jsonl").unwrap();
    assert_eq!(file_conversation(&provider, &path), None);
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert_eq!(file_conversation(&provider, &path), None);
    std::fs::remove_dir(&path).unwrap();
}

#[test]
fn unreadable_or_malformed_journal_is_unknown() {
    let temp = tempfile::tempdir().unwrap();
    let journal = TabLifecycleJournal::for_dir(temp.path().into());
    std::fs::create_dir(journal.path()).unwrap();
    assert_eq!(journal_conversation(&journal.load(), "tab", "saved"), None);
    std::fs::remove_dir(journal.path()).unwrap();
    std::fs::write(journal.path(), "malformed-journal").unwrap();
    assert_eq!(journal_conversation(&journal.load(), "tab", "saved"), None);
}

#[test]
fn quarantine_evidence_is_exact_session_scoped_and_malformed_data_is_unknown() {
    let temp = tempfile::tempdir().unwrap();
    let journal = TabLifecycleJournal::for_dir(temp.path().into());
    let sidecar = crate::session::lifecycle::refused_tail_path(journal.path());
    assert_eq!(
        quarantined_conversation(&sidecar, "own", "saved"),
        Some(false)
    );
    journal
        .note_conversation_started(
            LifecycleKey::new("foreign", LifecycleEpoch(1), "launch"),
            "saved",
        )
        .unwrap();
    std::fs::copy(journal.path(), &sidecar).unwrap();
    assert_eq!(
        quarantined_conversation(&sidecar, "own", "saved"),
        Some(false)
    );
    journal
        .note_conversation_started(
            LifecycleKey::new("own", LifecycleEpoch(1), "launch"),
            "saved",
        )
        .unwrap();
    std::fs::copy(journal.path(), &sidecar).unwrap();
    assert_eq!(
        quarantined_conversation(&sidecar, "own", "saved"),
        Some(true)
    );
    std::fs::write(&sidecar, "malformed-journal").unwrap();
    assert_eq!(quarantined_conversation(&sidecar, "own", "saved"), None);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn restore_service_claims_fresh_only_for_an_empty_missing_session() {
    for evidence in [Some(false), Some(true), None] {
        let (mut state, id) = MultiplexerState::new();
        let session = uuid::Uuid::new_v4().to_string();
        let tab = state.get_tab_mut(&id).unwrap();
        tab.agent_type = Some("claude".into());
        tab.agent_session_id = Some(session.clone());
        tab.restored_from_snapshot = true;
        tab.restorable_launch.captured = evidence.is_some();
        tab.close_restore = Some(CloseRestoreTabState {
            exact_session: true,
            ..Default::default()
        });
        let fixture = crate::service::test_fixture::ServiceFixture::new(state);
        let svc = &fixture.service;
        let journal = TabLifecycleJournal::for_dir(fixture.temp.path().join("journal"));
        let key = LifecycleKey::new(&id, LifecycleEpoch(1), "fixture-launch");
        assert!(svc
            .mux
            .write()
            .stamp_lifecycle(&id, key.epoch, &key.operation_id));
        svc.mux.write().get_tab_mut(&id).unwrap().cwd =
            Some(fixture.temp.path().to_string_lossy().into_owned());
        svc.agent_registry
            .register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
        if let Some(conversation) = evidence {
            journal
                .append(key.clone(), LifecycleTransition::EpochOpened)
                .unwrap();
            if conversation {
                journal
                    .note_conversation_started(key.clone(), &session)
                    .unwrap();
            }
        }
        let prepared = svc.prepare_empty_session_restore_in_journal(
            &id,
            "claude",
            Some(&session),
            Some(false),
            &journal,
        );
        assert_eq!(prepared, evidence == Some(false));
        let claim = super::super::claim_restored_launch(
            &svc.mux,
            &id,
            "claude",
            if prepared { None } else { Some(&session) },
            Some(fixture.temp.path().to_str().unwrap()),
            false,
        );
        if prepared {
            assert_eq!(claim.unwrap().unwrap().session_id, None);
        } else {
            assert!(claim.is_err());
            assert_eq!(
                svc.mux
                    .read()
                    .get_tab(&id)
                    .unwrap()
                    .agent_session_id
                    .as_deref(),
                Some(session.as_str())
            );
        }
    }
}
