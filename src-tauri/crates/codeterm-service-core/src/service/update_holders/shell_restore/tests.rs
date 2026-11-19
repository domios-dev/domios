use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{CloseRestoreTabState, ShellActivity};
use crate::service::test_fixture::ServiceFixture;
use codeterm_update::holders::ReserveHolders;

#[test]
fn update_snapshot_offers_the_busy_shell_command_once_and_excludes_the_caller() {
    let (mut mux, caller) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&caller).unwrap();
    let dev = mux.create_tab(&workspace).unwrap();
    let stale = mux.create_tab(&workspace).unwrap();
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    for (tab_id, pty_id, command) in [
        (&caller, "caller-pty", "update command"),
        (&dev, "dev-pty", "server command"),
        (&stale, "old-pty", "old command"),
    ] {
        {
            let mut mux = svc.mux.write();
            mux.link_pty(tab_id, pty_id.into());
            mux.get_tab_mut(tab_id)
                .unwrap()
                .shell_command_state
                .activity = ShellActivity::Running;
        }
        svc.utility_command_started(tab_id, pty_id, Some(command));
    }
    {
        let mut mux = svc.mux.write();
        mux.link_pty(&stale, "new-pty".into());
        mux.get_tab_mut(&dev).unwrap().close_restore = Some(CloseRestoreTabState {
            utility_owner_tab_id: Some(caller.clone()),
            ..Default::default()
        });
    }

    svc.reserve_update_shutdown(ReserveHolders {
        tabs: vec![],
        initiating_tab: Some(caller.clone()),
    })
    .unwrap();
    svc.capture_shutdown_shells();
    let mut restored =
        MultiplexerState::from_session_snapshot(&svc.mux.read().to_session_snapshot());

    assert!(restored.get_tab(&caller).unwrap().close_restore.is_none());
    assert!(restored.get_tab(&stale).unwrap().close_restore.is_none());
    let command = restored
        .get_tab_mut(&dev)
        .unwrap()
        .close_restore
        .as_mut()
        .unwrap();
    assert_eq!(
        command.utility_owner_tab_id.as_deref(),
        Some(caller.as_str())
    );
    assert_eq!(
        command.claim_rerun().unwrap().as_deref(),
        Some("server command")
    );
    assert!(command.claim_rerun().is_err());
}

#[test]
fn ordinary_shutdown_snapshot_preserves_running_shell_command_for_manual_rerun() {
    let (mut mux, caller) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&caller).unwrap();
    let shell = mux.create_tab(&workspace).unwrap();
    let fixture = ServiceFixture::new(mux);
    {
        let mut mux = fixture.service.mux.write();
        mux.link_pty(&shell, "ordinary-pty".into());
        mux.get_tab_mut(&shell)
            .unwrap()
            .shell_command_state
            .activity = ShellActivity::Running;
    }
    fixture
        .service
        .utility_command_started(&shell, "ordinary-pty", Some("python -m http.server"));

    fixture.service.capture_shutdown_shells();
    let restored =
        MultiplexerState::from_session_snapshot(&fixture.service.mux.read().to_session_snapshot());
    let restore = restored
        .get_tab(&shell)
        .unwrap()
        .close_restore
        .as_ref()
        .unwrap();

    assert!(restore.stopped);
    assert_eq!(restore.command.as_deref(), Some("python -m http.server"));
}
