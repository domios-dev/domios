use super::{close, started};
use crate::multiplexer::state::MultiplexerState;
use crate::service::{TeamMutationCaller, TeamPlanCaller};
use crate::session::store::StorageDeskAdapter;

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri service fixture requires Windows"
)]
fn workspace_create_round_trip_expected_red() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let created = instance
        .fixture
        .service
        .create_workspace("runtime workspace".into(), None, false)
        .expect("create workspace through ServiceContext");

    let restarted = instance.restart(root.path());
    let restored = restarted
        .fixture
        .service
        .mux
        .read()
        .list_workspaces()
        .into_iter()
        .any(|workspace| workspace.id == created.id);
    assert!(
        restored,
        "workspace creation must reach Storage before restart"
    );
    close(restarted);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri service fixture requires Windows"
)]
fn tab_create_round_trip_expected_red() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let workspace_id = instance.fixture.service.mux.read().list_workspaces()[0]
        .id
        .clone();
    let created = instance
        .fixture
        .service
        .create_tab_with_activate(&workspace_id, false)
        .expect("create tab through ServiceContext");

    let restarted = instance.restart(root.path());
    assert!(
        restarted
            .fixture
            .service
            .mux
            .read()
            .get_tab(&created)
            .is_some(),
        "tab creation must reach Storage before restart"
    );
    close(restarted);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri service fixture requires Windows"
)]
fn tab_rename_round_trip_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let workspace_id = instance.fixture.service.mux.read().list_workspaces()[0]
        .id
        .clone();
    let tab_id = instance
        .fixture
        .service
        .create_named_tab(&workspace_id, "before rename")
        .unwrap();
    instance
        .fixture
        .service
        .rename_tab(&tab_id, "after rename")
        .expect("rename through ServiceContext");

    let restarted = instance.restart(root.path());
    assert_eq!(
        restarted
            .fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .title,
        "after rename"
    );
    close(restarted);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri service fixture requires Windows"
)]
fn agent_tab_title_change_round_trip_expected_red() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let tab_id = instance.fixture.service.mux.read().list_workspaces()[0].tabs[0].clone();
    instance
        .fixture
        .service
        .set_tab_title(&tab_id, "agent title")
        .expect("update title through ServiceContext");
    assert_eq!(
        instance
            .fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .title,
        "agent title"
    );

    let restarted = instance.restart(root.path());
    assert_eq!(
        restarted
            .fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .title,
        "agent title",
        "agent-origin title changes must reach Storage before restart"
    );
    close(restarted);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri service fixture requires Windows"
)]
fn tab_description_change_round_trip_expected_red() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let tab_id = instance.fixture.service.mux.read().list_workspaces()[0].tabs[0].clone();
    instance
        .fixture
        .service
        .set_tab_description(&tab_id, "runtime description")
        .expect("update description through ServiceContext");

    let restarted = instance.restart(root.path());
    assert_eq!(
        restarted
            .fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .and_then(|tab| tab.description.as_deref()),
        Some("runtime description"),
        "tab descriptions are part of the persisted desk leaf"
    );
    close(restarted);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri service fixture requires Windows"
)]
fn tab_move_and_reorder_round_trip_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let (source_id, destination_id) = {
        let service = &instance.fixture.service;
        let workspaces = service.mux.read().list_workspaces();
        let source_id = workspaces[0].id.clone();
        let destination_id = service
            .create_workspace("destination".into(), None, false)
            .unwrap()
            .id;
        service
            .rename_workspace(&destination_id, "destination saved")
            .unwrap();
        let first = service
            .create_named_tab(&source_id, "first movable")
            .unwrap();
        let second = service
            .create_named_tab(&source_id, "second movable")
            .unwrap();
        service
            .reorder_tab(&source_id, &[second.clone(), first.clone()])
            .unwrap();
        service
            .move_tab(&first, &source_id, &destination_id)
            .unwrap();
        (source_id, destination_id)
    };

    let restarted = instance.restart(root.path());
    let mux = restarted.fixture.service.mux.read();
    let source = mux.get_workspace(&source_id).unwrap();
    assert_eq!(source.tabs.len(), 2);
    let destination = mux.get_workspace(&destination_id).unwrap();
    assert_eq!(destination.tabs.len(), 1);
    let destination_tab = mux.get_tab(&destination.tabs[0]).unwrap();
    assert_eq!(destination_tab.title, "first movable");
    drop(mux);
    close(restarted);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri service fixture requires Windows"
)]
fn workspace_rename_and_close_round_trip_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let workspace_id = instance
        .fixture
        .service
        .create_workspace("before rename".into(), None, false)
        .unwrap()
        .id;
    instance
        .fixture
        .service
        .rename_workspace(&workspace_id, "after rename")
        .unwrap();
    instance
        .fixture
        .service
        .create_named_tab(&workspace_id, "closed with workspace")
        .unwrap();
    instance
        .fixture
        .service
        .close_workspace(&workspace_id)
        .expect("close workspace through ServiceContext");

    let restarted = instance.restart(root.path());
    assert!(restarted
        .fixture
        .service
        .mux
        .read()
        .get_workspace(&workspace_id)
        .is_none());
    close(restarted);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri service fixture requires Windows"
)]
fn tab_close_and_closed_history_round_trip_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let workspace_id = instance.fixture.service.mux.read().list_workspaces()[0]
        .id
        .clone();
    let tab_id = instance
        .fixture
        .service
        .create_named_tab(&workspace_id, "closing history")
        .unwrap();
    instance
        .fixture
        .service
        .close_tab(&tab_id)
        .expect("close tab through ServiceContext");

    let restarted = instance.restart(root.path());
    let service = &restarted.fixture.service;
    assert!(service.mux.read().get_tab(&tab_id).is_none());
    let closed = service.list_recently_closed(&workspace_id);
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].title, "closing history");
    close(restarted);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri service fixture requires Windows"
)]
fn session_identity_bind_and_rebind_round_trip_expected_red() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, tab_id) = MultiplexerState::new();
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("identity-provider".into());
        tab.cwd = Some(root.path().to_string_lossy().into_owned());
    }
    let instance = super::ServiceInstance::open_with_mux(root.path(), mux);
    instance
        .fixture
        .service
        .set_agent_session_id(&tab_id, Some("session-old".into()));
    instance
        .fixture
        .service
        .set_agent_session_id(&tab_id, Some("session-new".into()));

    let restarted = instance.restart(root.path());
    assert_eq!(
        restarted
            .fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .and_then(|tab| tab.agent_session_id.as_deref()),
        Some("session-new"),
        "session binding and rebind must update the persisted identity catalogue"
    );
    close(restarted);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri service fixture requires Windows"
)]
fn team_create_round_trip_expected_red() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, tab_id) = MultiplexerState::new();
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("identity-provider".into());
        tab.cwd = Some(root.path().to_string_lossy().into_owned());
    }
    mux.set_orchestrator_role(&tab_id).unwrap();
    let instance = super::ServiceInstance::open_with_mux(root.path(), mux);
    instance
        .fixture
        .service
        .deliver_orchestrator_onboarding(&tab_id);
    let team_id = instance
        .fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .and_then(|tab| tab.team_id.clone())
        .expect("onboarding creates a Team through ServiceContext");

    let restarted = instance.restart(root.path());
    assert!(restarted
        .fixture
        .service
        .mux
        .read()
        .get_team(&team_id)
        .is_some());
    close(restarted);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri service fixture requires Windows"
)]
fn team_rename_round_trip_expected_red() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, tab_id) = MultiplexerState::new();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&tab_id)
        .unwrap()
        .team_id;
    let instance = super::ServiceInstance::open_with_mux(root.path(), mux);
    crate::commands::mux_commands::mux_rename_team_service_core(
        &instance.fixture.service,
        team_id.clone(),
        "renamed in service".into(),
        None,
    )
    .expect("rename Team through the command service boundary");

    let restarted = instance.restart(root.path());
    assert_eq!(
        restarted
            .fixture
            .service
            .mux
            .read()
            .get_team(&team_id)
            .and_then(|team| team.name.as_deref()),
        Some("renamed in service")
    );
    close(restarted);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri service fixture requires Windows"
)]
fn team_close_round_trip_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, tab_id) = MultiplexerState::new();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&tab_id)
        .unwrap()
        .team_id;
    let instance = super::ServiceInstance::open_with_mux(root.path(), mux);
    tauri::async_runtime::block_on(instance.fixture.service.close_team(
        &team_id,
        TeamMutationCaller::Human,
        true,
    ))
    .expect("close Team through ServiceContext");

    let restarted = instance.restart(root.path());
    assert!(restarted
        .fixture
        .service
        .mux
        .read()
        .get_team(&team_id)
        .is_none());
    close(restarted);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri service fixture requires Windows"
)]
fn plan_team_binding_round_trip_expected_red() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.set_orchestrator_role(&tab_id).unwrap();
    let instance = super::ServiceInstance::open_with_mux(root.path(), mux);
    let outcome = instance
        .fixture
        .service
        .create_team_plan_for_caller(
            "round-trip plan".into(),
            root.path().to_string_lossy().into_owned(),
            crate::plan::OrchestratorRef {
                tab_id: Some(tab_id.clone()),
                session_id: None,
                title: Some("orchestrator".into()),
            },
            TeamPlanCaller {
                tab_id: None,
                session_id: None,
                binding_nonce: None,
            },
            Vec::new(),
            None,
            1_700_000_000_000,
        )
        .expect("create Team and bind its Plan through ServiceContext");
    let team_id = outcome.team_id;
    let plan_id = outcome.plan.id;

    let restarted = instance.restart(root.path());
    let plan = restarted
        .fixture
        .service
        .plan_store
        .get(&plan_id)
        .expect("plan database survives restart");
    assert_eq!(plan.team_id.as_deref(), Some(team_id.as_str()));
    assert!(
        restarted
            .fixture
            .service
            .mux
            .read()
            .get_team(&team_id)
            .is_some(),
        "the Plan's Team binding must also survive restart"
    );
    close(restarted);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri service fixture requires Windows"
)]
fn layout_active_tab_round_trip_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let workspace_id = instance.fixture.service.mux.read().list_workspaces()[0]
        .id
        .clone();
    let active_tab_id = instance.fixture.service.mux.read().list_workspaces()[0].tabs[0].clone();
    instance
        .fixture
        .service
        .create_named_tab(&workspace_id, "layout second leaf")
        .unwrap();
    instance
        .fixture
        .service
        .set_active_tab(&workspace_id, &active_tab_id)
        .expect("select the layout's active tab through ServiceContext");

    let restarted = instance.restart(root.path());
    let persisted = StorageDeskAdapter::from_storage(restarted.storage.clone())
        .snapshot()
        .expect("read reopened desk")
        .workspaces
        .into_iter()
        .find(|workspace| workspace.record.id == workspace_id)
        .unwrap();
    assert_eq!(
        persisted.active_tab_id.as_deref(),
        Some(active_tab_id.as_str())
    );
    close(restarted);
}
