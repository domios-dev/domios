use super::super::account_supervision;
use super::{LaunchGeneration, MultiplexerState, LAUNCH_REGISTRY};
use crate::multiplexer::types::AgentRole;
use crate::service::launch_diagnostics::AgentLaunchOrigin;
use crate::service::launch_readiness::LaunchAttempt;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn overlapping_manager_restores_keep_their_launch_authority_at_the_first_prompt() {
    let (mut mux, designer) = MultiplexerState::new();
    let workspace = mux.get_active_workspace().unwrap().id.clone();
    let coordinator = mux.create_tab(&workspace).unwrap();
    let parent = mux.create_tab(&workspace).unwrap();
    let managers = [
        (
            designer.as_str(),
            "design-pty",
            "design-session",
            "design_lead",
            "design-team",
            Some(parent.as_str()),
            r"C:\Projects\Design Desk",
            "file:///c:/Projects/Design%20Desk",
        ),
        (
            coordinator.as_str(),
            "coordinator-pty",
            "coordinator-session",
            "orchestrator",
            "coordinator-team",
            None,
            r"D:\Projects\Coordinator",
            "file:///D:/Projects/Coordinator",
        ),
    ];
    for (tab_id, pty, session, profile, team, parent, cwd, _) in managers {
        mux.link_pty(tab_id, pty.into());
        let tab = mux.get_tab_mut(tab_id).unwrap();
        tab.agent_type = Some("fixture-provider".into());
        tab.agent_session_id = Some(session.into());
        tab.restored_from_snapshot = true;
        tab.role = Some(AgentRole::Manager);
        tab.role_profile = Some(profile.into());
        tab.team_id = Some(team.into());
        tab.parent_tab_id = parent.map(str::to_owned);
        tab.can_spawn = parent.is_none();
        tab.depth = if parent.is_some() { 2 } else { 1 };
        tab.cwd = Some(cwd.into());
    }
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;

    let pending = managers.map(|(tab_id, pty, session, ..)| {
        let (account, nonce) =
            account_supervision::prepare(svc, tab_id, "fixture-provider", Some(session)).unwrap();
        let mut launch = LaunchGeneration::begin(tab_id, pty.into());
        launch
            .capture_binding_nonce(
                svc,
                None,
                None,
                AgentLaunchOrigin::RestoredSnapshot,
                Some(session),
                nonce,
            )
            .unwrap();
        (account, launch)
    });
    assert_ne!(pending[0].1.binding_nonce, pending[1].1.binding_nonce);

    for (_, pty, _, _, _, _, _, osc7) in managers.into_iter().rev() {
        svc.process_pty_output(pty, format!("\x1b]7;{osc7}\x07").as_bytes());
    }
    for ((tab_id, pty, session, profile, team, parent, cwd, _), (account, launch)) in
        managers.into_iter().zip(pending)
    {
        launch
            .write_if_current(|| account.validate(svc, tab_id))
            .expect("the restore still owns its launch")
            .expect("the first shell prompt preserves launch authority");
        assert_eq!(
            LaunchAttempt::current(&svc.mux, tab_id, pty),
            launch.attempt()
        );
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab_id).unwrap();
        assert_eq!(tab.cwd.as_deref(), Some(cwd));
        assert_eq!(tab.agent_session_id.as_deref(), Some(session));
        assert_eq!(tab.role, Some(AgentRole::Manager));
        assert_eq!(tab.role_profile.as_deref(), Some(profile));
        assert_eq!(tab.team_id.as_deref(), Some(team));
        assert_eq!(tab.parent_tab_id.as_deref(), parent);
        assert!(!tab.launch_session_authenticated());
        drop(mux);
        LAUNCH_REGISTRY.lock().finish(&launch);
    }
}

#[test]
fn fresh_fixture_orchestrator_launch_accepts_cwd_spelling_but_refuses_a_new_directory() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let pty = "fixture-orchestrator-pty";
    let provider = "fixture-orchestrator";
    mux.link_pty(&tab_id, pty.into());
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some(provider.into());
    tab.cwd = Some(r"C:\Projects\Fixture".into());
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    let (account, _) = account_supervision::prepare(svc, &tab_id, provider, None).unwrap();

    svc.process_pty_output(pty, b"\x1b]7;file:///c:/Projects/Fixture\x07");
    account.validate(svc, &tab_id).unwrap();

    for (marker, cwd) in [
        (
            b"\x1b]7;file:///C:/Projects/Fixture/child\x07".as_slice(),
            "C:/Projects/Fixture/child",
        ),
        (
            b"\x1b]7;file:///C:/Projects/Fixture-new\x07",
            "C:/Projects/Fixture-new",
        ),
        (
            b"\x1b]7;file:///C:/Projects/fixture\x07",
            "C:/Projects/fixture",
        ),
    ] {
        svc.process_pty_output(pty, marker);
        assert_eq!(
            svc.mux.read().get_tab(&tab_id).unwrap().cwd.as_deref(),
            Some(cwd)
        );
        assert!(account
            .validate(svc, &tab_id)
            .unwrap_err()
            .starts_with("launch_account_source_changed:"));
    }
}
