use crate::service::launch_readiness::{self, LaunchAttempt, LaunchSettlement};
use crate::service::process_tree::{ProcessIdentity, ProviderProcessTreeSnapshot, ProviderTreeKey};
use crate::service::test_fixture::ServiceFixture;
use std::collections::HashSet;

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
        tab.restored_from_snapshot = !launch_in_flight;
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
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
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
    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, live_opencode_process());
    fixture
        .service
        .reconcile_boot_stage_with_session_binding(&tab_id);
    assert!(fixture.service.provider_tree_witnesses.has_seen(&key));

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
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn restore_relaunch_in_flight_with_unseen_provider_keeps_identity() {
    let (fixture, tab_id, pty_id, attempt, key) = restored_opencode_fixture();
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
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn restored_without_relaunch_absent_provider_clears_identity() {
    let (fixture, tab_id, pty_id, _, key) = restored_opencode_fixture_with_launch(false);
    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, ProviderProcessTreeSnapshot::Absent);

    assert!(fixture
        .service
        .clear_tab_restored_identity_if_absent(&tab_id, "restored without a live agent"));
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .is_some_and(|tab| tab.agent_type.is_none()));
    fixture
        .service
        .pty
        .lock()
        .kill(&pty_id)
        .expect("stop the tab's PTY shell");
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn lifecycle_identity_clear_requires_provider_gone_from_tab_tree() {
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
    assert!(fixture
        .service
        .clear_tab_agent_identity_for_lifecycle_exit_during(&tab_id, cause, None));
    let settlement = fixture
        .service
        .observe_launch_readiness(attempt.clone(), std::time::Duration::ZERO, false)
        .recv()
        .expect("published exit settles the waiter");
    let LaunchSettlement::Failed { reason, .. } = settlement else {
        panic!("one absent snapshot after seeing the provider proves the exit");
    };
    assert!(reason.contains("exit_code=unknown"));
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .is_some_and(|tab| tab.agent_type.is_none()));
    fixture
        .service
        .pty
        .lock()
        .kill(&pty_id)
        .expect("stop the tab's PTY shell");
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
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
        .is_some_and(|tab| tab.agent_type.is_none()));
    fixture
        .service
        .pty
        .lock()
        .kill(&pty_id)
        .expect("stop the tab's PTY shell");
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
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
        .is_some_and(|tab| tab.agent_type.is_none()));
    fixture
        .service
        .pty
        .lock()
        .kill(&pty_id)
        .expect("stop the tab's PTY shell");
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn confirmed_pty_exit_finalises_launch_before_provider_is_seen() {
    let (fixture, tab_id, pty_id, attempt, _) = restored_opencode_fixture();
    fixture
        .service
        .pty
        .lock()
        .kill(&pty_id)
        .expect("reap the tab's PTY shell");

    assert!(fixture
        .service
        .clear_tab_agent_identity_after_confirmed_exit(&tab_id, "PTY process reaped"));
    assert!(launch_readiness::take_published_failure(&attempt)
        .is_some_and(|reason| reason.contains("exit_code=unknown")));
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .is_some_and(|tab| tab.agent_type.is_none()));
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
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
