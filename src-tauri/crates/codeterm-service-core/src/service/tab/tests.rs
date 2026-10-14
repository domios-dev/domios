use crate::multiplexer::types::AgentRole;
use crate::service::launch_readiness::{self, LaunchAttempt, LaunchExitProof};
use crate::service::process_tree::{ProcessIdentity, ProviderProcessTreeSnapshot, ProviderTreeKey};
use crate::service::test_fixture::ServiceFixture;
use crate::session::lifecycle::{JournalRecord, LifecycleTransition};
use codeterm_types::pty::{RemoteOwnership, RemoteOwnershipTeam};
use std::collections::HashSet;

#[test]
fn failed_restore_keeps_identity_session_and_exact_restore_data() {
    use codeterm_types::pty::BootStage;
    for stage in [
        BootStage::SpawnFailed,
        BootStage::LaunchDeadlineElapsed,
        BootStage::LaunchBindingUnconfirmed,
    ] {
        let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("fixture-provider".into());
        tab.agent_type_locked = true;
        tab.agent_session_id = Some("saved-session".into());
        tab.session_binding_nonce = Some("failed-nonce".into());
        tab.role = Some(AgentRole::Manager);
        tab.role_profile = Some("general_agent".into());
        tab.boot_stage = Some(stage);
        tab.boot_failure_reason = Some("opaque-launch-failure".into());
        tab.close_restore = Some(crate::multiplexer::types::CloseRestoreTabState {
            exact_session: true,
            session_cwd: Some("saved-cwd".into()),
            record_id: Some("saved-close-record".into()),
            ..Default::default()
        });
        let fixture = ServiceFixture::new(mux);
        for _ in 0..3 {
            assert!(!fixture
                .service
                .clear_tab_spawn_failed_identity_if_absent(&tab_id, "failed restore"));
            assert!(!fixture
                .service
                .clear_tab_agent_identity_after_confirmed_exit(&tab_id, "failed process exit"));
        }
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(tab.agent_type.as_deref(), Some("fixture-provider"));
        assert!(tab.agent_type_locked);
        assert_eq!(tab.agent_session_id.as_deref(), Some("saved-session"));
        assert_eq!(tab.role, Some(AgentRole::Manager));
        assert_eq!(tab.role_profile.as_deref(), Some("general_agent"));
        assert_eq!(tab.boot_stage, Some(stage));
        assert_eq!(
            tab.boot_failure_reason.as_deref(),
            Some("opaque-launch-failure")
        );
        let restore = tab.close_restore.as_ref().unwrap();
        assert!(restore.exact_session);
        assert_eq!(restore.session_cwd.as_deref(), Some("saved-cwd"));
        assert_eq!(restore.record_id.as_deref(), Some("saved-close-record"));
    }
}

fn restored_opencode_fixture() -> (
    ServiceFixture,
    String,
    String,
    LaunchAttempt,
    ProviderTreeKey,
) {
    restored_opencode_fixture_with_launch(true)
}

fn restored_opencode_fixture_with_launch(
    launch_in_flight: bool,
) -> (
    ServiceFixture,
    String,
    String,
    LaunchAttempt,
    ProviderTreeKey,
) {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let pty_id = "restored-opencode-tree-pty".to_string();
    mux.link_pty(&tab_id, pty_id.clone());
    {
        let tab = mux.get_tab_mut(&tab_id).expect("seeded tab");
        tab.agent_type = Some("opencode".into());
        tab.agent_session_id = Some("restored-opencode-session".into());
        tab.session_binding_nonce = Some("restored-opencode-nonce".into());
        tab.shell_integration_nonce = Some("restored-opencode-shell".into());
        tab.restored_from_snapshot = true;
        tab.agent_booting = launch_in_flight;
        tab.boot_stage = Some(if launch_in_flight {
            codeterm_types::pty::BootStage::LaunchWritten
        } else {
            codeterm_types::pty::BootStage::ShellReady
        });
    }
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("opencode").with_launch_binary("opencode"),
    ));
    fixture
        .service
        .pty
        .lock()
        .spawn_with_id(Some(pty_id.clone()), None, None, 80, 24, Default::default())
        .expect("spawn the tab's PTY shell");
    let attempt = LaunchAttempt {
        tab_id: tab_id.clone(),
        pty_id: pty_id.clone(),
        binding_nonce: "restored-opencode-nonce".into(),
    };
    if launch_in_flight {
        launch_readiness::arm(attempt.clone());
    }
    let key = fixture
        .service
        .provider_tree_key_for_tab(&tab_id, &pty_id)
        .expect("provider capability supplies the process name");
    (fixture, tab_id, pty_id, attempt, key)
}

fn live_opencode_process() -> ProviderProcessTreeSnapshot {
    ProviderProcessTreeSnapshot::Present(HashSet::from([ProcessIdentity {
        pid: 52_520,
        start_token: "live-opencode-child".into(),
    }]))
}

fn assert_opencode_identity(fixture: &ServiceFixture, tab_id: &str) {
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(tab_id)
        .is_some_and(|tab| tab.agent_type.as_deref() == Some("opencode")));
}

#[test]
fn release_remote_ownership_refuses_other_tabs_and_persists_the_release() {
    let (mut mux, orchestrator) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    mux.set_tab_role_kind(
        &orchestrator,
        AgentRole::Manager,
        Some("orchestrator"),
        None,
    )
    .unwrap();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&orchestrator)
        .unwrap()
        .team_id;
    let tab_id = mux.create_tab(&workspace_id).unwrap();
    let unrelated = mux.create_tab(&workspace_id).unwrap();
    {
        let target = mux.get_tab_mut(&tab_id).unwrap();
        target.team_id = Some(team_id.clone());
        target.ownership = Some(RemoteOwnership {
            controller_title: "Orchestrator".into(),
            controller_tab_id: Some("remote-controller".into()),
            peer_id: "peer-eight".into(),
            peer_name: "eight".into(),
            team: Some(RemoteOwnershipTeam {
                id: team_id,
                title: Some("Mesh team".into()),
            }),
        });
    }
    let fixture = ServiceFixture::new(mux);
    let refusal = fixture
        .service
        .release_remote_ownership(&tab_id, Some(&unrelated))
        .expect_err("an unrelated local tab cannot release ownership");
    assert!(refusal.contains("not authorized"));
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .ownership
        .is_some());

    let app = fixture._app.handle().clone();
    let storage = crate::storage::Storage::open(
        fixture.temp.path().join("ownership-release-storage"),
        crate::storage::StorageConfig::default(),
    )
    .unwrap();
    let adapter = crate::session::store::StorageDeskAdapter::from_storage(storage.clone());
    let snapshot = fixture.service.mux.read().to_session_snapshot();
    adapter
        .import_snapshot(&snapshot, Vec::new())
        .expect("seed the Storage desk before persistence starts");
    crate::service::session_persist::install_storage_desk_adapter(&app, adapter).unwrap();
    let released = fixture
        .service
        .release_remote_ownership(&tab_id, Some(&orchestrator))
        .expect("Team Orchestrator releases its tab")
        .expect("ownership existed");
    assert_eq!(released.peer_id, "peer-eight");
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .ownership
        .is_none());

    crate::service::session_persist::write_session_snapshot(&app, &fixture.service.mux)
        .expect("flush the queued desk snapshot");
    let persisted = crate::session::store::StorageDeskAdapter::from_storage(storage.clone())
        .snapshot()
        .expect("read persisted desk snapshot");
    let restored = crate::multiplexer::state::MultiplexerState::from_session_snapshot(&persisted);
    assert!(restored.get_tab(&tab_id).unwrap().ownership.is_none());

    let path = format!("/api/tabs/{tab_id}/send");
    assert!(!crate::api_support::access_policy::peer_access_allowed(
        &crate::mesh::types::AccessLevel::AgentIsolated,
        "peer-eight",
        &axum::http::Method::POST,
        &path,
        |id| fixture
            .service
            .mux
            .read()
            .get_tab(id)
            .and_then(|tab| tab.ownership.as_ref())
            .map(|owner| owner.peer_id.clone()),
    ));
    assert!(fixture
        .service
        .lifecycle_journal
        .load()
        .records
        .iter()
        .any(|record| {
            matches!(
                record,
                JournalRecord::Transition { entry }
                    if entry.key.tab_id == tab_id
                        && matches!(
                            &entry.transition,
                            LifecycleTransition::RemoteOwnershipReleased { peer_id, .. }
                                if peer_id == "peer-eight"
                        )
            )
        }));
    crate::service::session_persist::stop_worker(&app).unwrap();
    storage
        .shutdown()
        .wait(std::time::Duration::from_secs(30))
        .unwrap();
}

#[test]
fn restored_opencode_with_live_child_is_never_reported_exited() {
    let (fixture, tab_id, pty_id, attempt, key) = restored_opencode_fixture();
    let cause = "restored without a live agent";
    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, ProviderProcessTreeSnapshot::Absent);
    assert!(fixture
        .service
        .observe_provider_process_tree(&tab_id, &pty_id)
        .is_some());
    fixture
        .service
        .process_pty_output(&pty_id, b"PS C:\\workspace> opencode");
    assert!(launch_readiness::take_published_failure(&attempt).is_none());
    assert_opencode_identity(&fixture, &tab_id);

    fixture
        .service
        .process_pty_output(&pty_id, b"OpenCode startup output");
    assert!(!fixture.service.provider_tree_witnesses.has_seen(&key));

    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).expect("restored tab");
        tab.reported_session_binding_nonce = Some("restored-opencode-nonce".into());
        tab.agent_booting = false;
        tab.boot_stage = Some(codeterm_types::pty::BootStage::ShellReady);
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    fixture
        .service
        .provider_tree_witnesses
        .report_next_observation(&key, sender);
    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, live_opencode_process());
    fixture
        .service
        .reconcile_boot_stage_with_session_binding(&tab_id);
    let observed = receiver
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("provider readiness tree snapshot completes off the runtime");
    assert_eq!(
        observed.state(),
        crate::service::process_tree::ProviderTreeState::Present
    );
    assert!(fixture.service.provider_tree_witnesses.has_seen(&key));

    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, live_opencode_process());
    fixture.service.process_pty_output(
        &pty_id,
        b"\x1b]633;E;;restored-opencode-shell\x07\x1b]633;D\x07",
    );
    assert!(launch_readiness::take_published_failure(&attempt).is_none());
    assert!(fixture.service.provider_tree_witnesses.has_seen(&key));
    assert_opencode_identity(&fixture, &tab_id);

    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, live_opencode_process());
    assert!(!fixture
        .service
        .clear_tab_agent_identity_for_lifecycle_exit_during(&tab_id, cause, None));
    assert_opencode_identity(&fixture, &tab_id);
    assert!(launch_readiness::take_published_failure(&attempt).is_none());
    fixture
        .service
        .pty
        .lock()
        .kill(&pty_id)
        .expect("stop the tab's PTY shell");
}

#[test]
fn restore_relaunch_in_flight_with_unseen_provider_keeps_identity() {
    let (fixture, tab_id, pty_id, attempt, key) = restored_opencode_fixture();
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .is_some_and(|tab| tab.restored_from_snapshot));
    assert!(launch_readiness::attempt_is_armed(&attempt));
    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, ProviderProcessTreeSnapshot::Absent);

    assert!(!fixture
        .service
        .clear_tab_restored_identity_if_absent(&tab_id, "restored without a live agent"));

    assert!(!fixture.service.provider_tree_witnesses.has_seen(&key));
    assert_opencode_identity(&fixture, &tab_id);
    assert!(launch_readiness::take_published_failure(&attempt).is_none());

    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).expect("restored tab");
        tab.agent_booting = false;
        tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
    }
    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, ProviderProcessTreeSnapshot::Absent);
    assert!(!fixture
        .service
        .clear_tab_restored_identity_if_absent(&tab_id, "restored without a live agent"));
    assert_opencode_identity(&fixture, &tab_id);
    assert!(launch_readiness::take_published_failure(&attempt).is_none());

    fixture
        .service
        .pty
        .lock()
        .kill(&pty_id)
        .expect("stop the tab's PTY shell");
}

#[test]
fn restored_without_relaunch_absent_provider_keeps_identity_without_exit_marker() {
    let (fixture, tab_id, pty_id, _, key) = restored_opencode_fixture_with_launch(false);
    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, ProviderProcessTreeSnapshot::Absent);

    assert!(!fixture
        .service
        .clear_tab_restored_identity_if_absent(&tab_id, "restored without a live agent"));
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .is_some_and(|tab| tab.agent_type.as_deref() == Some("opencode")));
    fixture
        .service
        .pty
        .lock()
        .kill(&pty_id)
        .expect("stop the tab's PTY shell");
}

#[test]
fn expired_provisional_launch_stages_keep_identity_without_exit_marker() {
    for stage in [
        codeterm_types::pty::BootStage::LaunchDeadlineElapsed,
        codeterm_types::pty::BootStage::LaunchBindingUnconfirmed,
    ] {
        let (fixture, tab_id, pty_id, attempt, key) = restored_opencode_fixture();
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).expect("launching tab");
            tab.restored_from_snapshot = false;
            tab.agent_booting = false;
            tab.boot_stage = Some(stage);
        }

        assert!(launch_readiness::attempt_is_armed(&attempt));
        assert_eq!(
            launch_readiness::observe(&attempt, std::time::Instant::now()),
            launch_readiness::LaunchObservation::Deadline
        );
        assert!(!launch_readiness::attempt_is_armed(&attempt));

        fixture
            .service
            .provider_tree_witnesses
            .script_snapshot(&key, ProviderProcessTreeSnapshot::Absent);
        assert!(!fixture
            .service
            .clear_tab_restored_identity_if_absent(&tab_id, "restored without a live agent"));
        assert!(fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .is_some_and(|tab| tab.agent_type.as_deref() == Some("opencode")));
        assert!(fixture.service.pty.lock().get_session(&pty_id).is_some());
    }
}

#[test]
fn lifecycle_identity_clear_refuses_process_tree_absence_without_exit_marker() {
    let (fixture, tab_id, pty_id, attempt, key) = restored_opencode_fixture();
    let cause = "restored without a live agent";

    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, live_opencode_process());
    assert!(!fixture
        .service
        .clear_tab_agent_identity_for_lifecycle_exit_during(&tab_id, cause, None));
    assert_opencode_identity(&fixture, &tab_id);
    assert!(launch_readiness::take_published_failure(&attempt).is_none());

    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, ProviderProcessTreeSnapshot::Unknown);
    assert!(!fixture
        .service
        .clear_tab_agent_identity_for_lifecycle_exit_during(&tab_id, cause, None));
    assert_opencode_identity(&fixture, &tab_id);
    assert!(launch_readiness::take_published_failure(&attempt).is_none());

    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, ProviderProcessTreeSnapshot::Absent);
    assert!(!fixture
        .service
        .clear_tab_agent_identity_for_lifecycle_exit_during(&tab_id, cause, None));
    assert_opencode_identity(&fixture, &tab_id);
    assert!(launch_readiness::take_published_failure(&attempt).is_none());
    fixture
        .service
        .pty
        .lock()
        .kill(&pty_id)
        .expect("stop the tab's PTY shell");
}

#[test]
fn finished_marker_cannot_clear_identity_after_the_pty_child_dies() {
    let (fixture, tab_id, pty_id, attempt, key) = restored_opencode_fixture();
    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, live_opencode_process());
    let _ = fixture
        .service
        .observe_provider_process_tree(&tab_id, &pty_id)
        .expect("observe the live provider tree");
    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, ProviderProcessTreeSnapshot::Absent);
    let gone = fixture
        .service
        .observe_provider_process_tree(&tab_id, &pty_id)
        .expect("observe the provider exit marker evidence");
    let proof = LaunchExitProof::from_finished_marker(&attempt, &gone, None, false, None)
        .expect("provider tree exit evidence");
    fixture
        .service
        .pty
        .lock()
        .kill(&pty_id)
        .expect("kill the PTY child before marker handling");

    assert!(!fixture
        .service
        .clear_tab_agent_identity_after_exit_with_proof(
            &tab_id,
            "provider exit marker after PTY death",
            &proof,
        ));
    assert_opencode_identity(&fixture, &tab_id);
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .is_some_and(|tab| tab.agent_session_id.as_deref() == Some("restored-opencode-session")));
}

#[test]
fn powershell_finished_marker_uses_a_distinct_launch_written_absence() {
    let (fixture, tab_id, pty_id, attempt, key) = restored_opencode_fixture();
    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, ProviderProcessTreeSnapshot::Absent);
    assert!(fixture
        .service
        .observe_provider_process_tree(&tab_id, &pty_id)
        .is_some());
    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, ProviderProcessTreeSnapshot::Absent);
    fixture.service.process_pty_output(
        &pty_id,
        b"\x1b]633;E;;restored-opencode-shell\x07\x1b]633;D\x07",
    );

    assert!(launch_readiness::take_published_failure(&attempt)
        .is_some_and(|reason| reason.contains("exit_code=unknown")));
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .is_some_and(|tab| tab.agent_type.as_deref() == Some("opencode")));
    fixture
        .service
        .pty
        .lock()
        .kill(&pty_id)
        .expect("stop the tab's PTY shell");
}

#[test]
fn bash_finished_marker_exit_code_finalises_launch_before_provider_seen() {
    let (fixture, tab_id, pty_id, attempt, key) = restored_opencode_fixture();
    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, ProviderProcessTreeSnapshot::Absent);

    fixture.service.process_pty_output(
        &pty_id,
        b"\x1b]633;C\x07\x1b]633;E;opencode;restored-opencode-shell\x07\x1b]633;D;7\x07",
    );

    assert!(launch_readiness::take_published_failure(&attempt)
        .is_some_and(|reason| reason.contains("exit_code=7")));
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .is_some_and(|tab| tab.agent_type.as_deref() == Some("opencode")));
    fixture
        .service
        .pty
        .lock()
        .kill(&pty_id)
        .expect("stop the tab's PTY shell");
}

#[test]
fn confirmed_pty_exit_finalises_launch_before_provider_is_seen() {
    let (fixture, tab_id, pty_id, attempt, _) = restored_opencode_fixture();
    fixture
        .service
        .pty
        .lock()
        .kill(&pty_id)
        .expect("reap the tab's PTY shell");

    assert!(!fixture
        .service
        .clear_tab_agent_identity_after_confirmed_exit(&tab_id, "PTY process reaped"));
    assert!(launch_readiness::take_published_failure(&attempt)
        .is_some_and(|reason| reason.contains("exit_code=unknown")));
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .is_some_and(|tab| tab.agent_type.as_deref() == Some("opencode")));
}

#[test]
fn explicit_ui_clear_does_not_depend_on_process_tree_absence() {
    let (fixture, tab_id, pty_id, attempt, key) = restored_opencode_fixture();
    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, live_opencode_process());

    fixture.service.set_tab_agent_type(&tab_id, None);

    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .is_some_and(|tab| tab.agent_type.is_none()));
    assert!(launch_readiness::take_published_failure(&attempt).is_none());
    fixture
        .service
        .pty
        .lock()
        .kill(&pty_id)
        .expect("stop the tab's PTY shell");
}

#[test]
fn confirmed_provider_exit_resets_terminal_modes_for_shell() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let pty_id = "exited-provider-shell-pty";
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("fixture-provider".into());
        tab.agent_session_id = Some("verified-session".into());
        tab.session_binding_nonce = Some("verified-nonce".into());
        tab.reported_session_binding_nonce = Some("verified-nonce".into());
        tab.pty_id = Some(pty_id.into());
    }
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .pty
        .lock()
        .spawn_with_id(Some(pty_id.into()), None, None, 80, 24, Default::default())
        .expect("spawn the tab's surviving shell PTY");
    fixture
        .service
        .screen_state
        .lock()
        .process(pty_id, b"\x1b[?2004h\x1b[?1h\x1b=");

    assert!(fixture
        .service
        .clear_tab_agent_identity_after_confirmed_exit(&tab_id, "provider process exited"));

    assert_eq!(
        fixture
            .service
            .screen_state
            .lock()
            .bracketed_paste_mode(pty_id),
        Some(false)
    );
    fixture
        .service
        .pty
        .lock()
        .kill(pty_id)
        .expect("stop the tab's surviving shell PTY");
}
