use super::{close, started};
use crate::multiplexer::state::MultiplexerState;
use crate::service::TeamPlanCaller;
use crate::session::store::StorageDeskAdapter;

#[test]
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
fn orphan_team_does_not_block_tab_commit_and_is_dropped_on_restart() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, anchor_tab_id) = MultiplexerState::new();
    let main_workspace_id = mux.find_workspace_for_tab(&anchor_tab_id).unwrap();
    let valid_team_id = mux
        .resolve_or_mint_team_for_parent(&anchor_tab_id)
        .unwrap()
        .team_id;
    let orphan_workspace_id = mux.create_workspace("Removed workspace".into(), None).id;
    let orphan_tab_id = mux.create_tab(&orphan_workspace_id).unwrap();
    let orphan_team_id = mux
        .resolve_or_mint_team_for_parent(&orphan_tab_id)
        .unwrap()
        .team_id;
    mux.close_workspace(&orphan_workspace_id).unwrap();
    assert!(mux.get_team(&orphan_team_id).is_some());
    let second_workspace_id = mux
        .create_workspace("Second Team workspace".into(), None)
        .id;
    let second_tab_id = mux.create_tab(&second_workspace_id).unwrap();
    let second_valid_team_id = mux
        .resolve_or_mint_team_for_parent(&second_tab_id)
        .unwrap()
        .team_id;

    let instance = super::ServiceInstance::open_with_mux(root.path(), mux);
    crate::service::session_persist::flush_session_persistence(&instance.fixture.service)
        .expect("flush startup resync with orphan Team");
    let startup_saved = StorageDeskAdapter::from_storage(instance.storage.clone())
        .state()
        .expect("read startup resync");
    assert!(startup_saved
        .snapshot
        .teams
        .iter()
        .any(|team| team.id.as_str() == valid_team_id.as_str()));
    assert!(startup_saved
        .snapshot
        .teams
        .iter()
        .any(|team| team.id.as_str() == second_valid_team_id.as_str()));
    assert!(!startup_saved
        .snapshot
        .teams
        .iter()
        .any(|team| team.id.as_str() == orphan_team_id.as_str()));
    let created_workspace = instance
        .fixture
        .service
        .create_workspace("Committed workspace".into(), None, false)
        .expect("create workspace beside orphan Team");
    let created_tab = instance
        .fixture
        .service
        .create_tab_with_activate(&main_workspace_id, false)
        .expect("create tab while orphan Team is present");
    crate::service::session_persist::flush_session_persistence(&instance.fixture.service)
        .expect("flush tab and workspace changes");
    let saved = StorageDeskAdapter::from_storage(instance.storage.clone())
        .state()
        .expect("read committed desk");

    assert!(saved.tab_revisions.contains_key(&created_tab));
    assert!(saved
        .snapshot
        .workspaces
        .iter()
        .any(|workspace| workspace.record.id.as_str() == created_workspace.id.as_str()));
    assert!(saved
        .snapshot
        .teams
        .iter()
        .any(|team| team.id.as_str() == valid_team_id.as_str()));
    assert!(!saved
        .snapshot
        .teams
        .iter()
        .any(|team| team.id.as_str() == orphan_team_id.as_str()));

    let restarted = instance.restart(root.path());
    let restored_mux = restarted.fixture.service.mux.read();
    assert!(restored_mux.get_tab(&created_tab).is_some());
    assert!(restored_mux.get_workspace(&created_workspace.id).is_some());
    assert!(restored_mux.get_team(&valid_team_id).is_some());
    assert!(restored_mux.get_team(&second_valid_team_id).is_some());
    assert!(restored_mux.get_team(&orphan_team_id).is_none());
    drop(restored_mux);
    close(restarted);
}

#[test]
fn tab_rename_round_trip_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let workspace_id = instance.fixture.service.mux.read().list_workspaces()[0]
        .id
        .clone();
    let tab_id = instance
        .fixture
        .service
        .create_named_tab(&workspace_id, "beforeRename")
        .unwrap();
    instance
        .fixture
        .service
        .rename_tab(&tab_id, "afterRename")
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
        "afterRename"
    );
    close(restarted);
}

#[test]
fn agent_tab_title_change_round_trip_expected_red() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let tab_id = instance.fixture.service.mux.read().list_workspaces()[0].tab_ids[0].clone();
    instance
        .fixture
        .service
        .set_tab_title(&tab_id, "AgentTitle")
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
        "AgentTitle"
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
        "AgentTitle",
        "agent-origin title changes must reach Storage before restart"
    );
    close(restarted);
}

#[test]
fn tab_description_change_round_trip_expected_red() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let tab_id = instance.fixture.service.mux.read().list_workspaces()[0].tab_ids[0].clone();
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
            .create_named_tab(&source_id, "firstMovable")
            .unwrap();
        let second = service
            .create_named_tab(&source_id, "secondMovable")
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
    assert_eq!(destination_tab.title, "firstMovable");
    drop(mux);
    close(restarted);
}

#[test]
fn display_order_across_tab_levels_round_trips_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let (lead, worker, loose) = {
        let service = &instance.fixture.service;
        let workspace_id = service.mux.read().list_workspaces()[0].id.clone();
        let lead = service.create_named_tab(&workspace_id, "lead").unwrap();
        let worker = service.create_named_tab(&workspace_id, "worker").unwrap();
        let loose = service.create_named_tab(&workspace_id, "loose").unwrap();
        service
            .mux
            .write()
            .get_tab_mut(&worker)
            .unwrap()
            .parent_tab_id = Some(lead.clone());
        service
            .reorder_display(
                &workspace_id,
                &[loose.clone(), lead.clone(), worker.clone()],
            )
            .unwrap();
        (lead, worker, loose)
    };

    let restarted = instance.restart(root.path());
    let mux = restarted.fixture.service.mux.read();
    let order_of = |id: &str| mux.get_tab(id).unwrap().sort_order;
    assert!(order_of(&loose) < order_of(&lead));
    assert!(order_of(&lead) < order_of(&worker));
    drop(mux);
    close(restarted);
}

#[test]
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
        .create_named_tab(&workspace_id, "closedWithWorkspace")
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
fn tab_close_and_closed_history_round_trip_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let workspace_id = instance.fixture.service.mux.read().list_workspaces()[0]
        .id
        .clone();
    let tab_id = instance
        .fixture
        .service
        .create_named_tab(&workspace_id, "closingHistory")
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
    assert_eq!(closed[0].title, "closingHistory");
    close(restarted);
}

#[test]
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
fn team_orchestrator_provider_and_session_round_trip_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, tab_id) = MultiplexerState::new();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&tab_id)
        .expect("pair reservation creates the Orchestrator's Team")
        .team_id;
    mux.set_orchestrator_role(&tab_id).unwrap();
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("codex".into());
        tab.agent_type_locked = true;
        tab.agent_session_id = Some("orchestrator-session".into());
        tab.session_binding_nonce = Some("orchestrator-binding".into());
        tab.reported_session_binding_nonce = Some("orchestrator-binding".into());
        tab.cwd = Some(root.path().to_string_lossy().into_owned());
    }
    let instance = super::ServiceInstance::open_with_mux(root.path(), mux);
    crate::service::session_persist::write_session_snapshot_with_intent(
        &instance.fixture.service,
        &instance.fixture.service.mux,
        crate::session::store::SaveIntent::AuthorizedReduction,
    )
    .expect("persist the Orchestrator provider and session");

    let saved = StorageDeskAdapter::from_storage(instance.storage.clone())
        .state()
        .expect("read the persisted Team desk");
    assert!(saved
        .snapshot
        .teams
        .iter()
        .any(|team| team.id.as_str() == team_id.as_str()));
    let persisted_tab = saved
        .snapshot
        .workspaces
        .iter()
        .flat_map(|workspace| &workspace.tabs)
        .find(|tab| tab.id == tab_id)
        .expect("persisted Orchestrator row exists");
    let codeterm_storage::DeskLayoutNode::Leaf { fields, .. } = &persisted_tab.layout else {
        panic!("a tab row has a leaf layout");
    };
    assert_eq!(fields.values["agent_type"].as_str(), Some("codex"));
    assert_eq!(
        fields.values["claude_session_id"].as_str(),
        Some("orchestrator-session")
    );
    assert_eq!(fields.values["role"].as_str(), Some("Manager"));
    assert_eq!(fields.values["role_profile"].as_str(), Some("orchestrator"));
    assert_eq!(fields.values["team_id"].as_str(), Some(team_id.as_str()));

    let restarted = instance.restart(root.path());
    let claim =
        crate::service::pty_io::claim_restored_pty_boot(&restarted.fixture.service.mux, &tab_id)
            .expect("restored Orchestrator is claimable")
            .expect("restored provider remains an agent");
    assert_eq!(claim.provider_id, "codex");
    assert_eq!(claim.session_id.as_deref(), Some("orchestrator-session"));
    let mux = restarted.fixture.service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.team_id.as_deref(), Some(team_id.as_str()));
    assert_eq!(tab.role_profile.as_deref(), Some("orchestrator"));
    assert!(mux.get_team(&team_id).is_some());
    drop(mux);
    close(restarted);
}

#[test]
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
fn layout_active_tab_round_trip_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let workspace_id = instance.fixture.service.mux.read().list_workspaces()[0]
        .id
        .clone();
    let active_tab_id = instance.fixture.service.mux.read().list_workspaces()[0].tab_ids[0].clone();
    instance
        .fixture
        .service
        .create_named_tab(&workspace_id, "layoutSecondLeaf")
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
        .find(|workspace| workspace.id == workspace_id)
        .unwrap();
    assert_eq!(
        persisted.active_tab_id.as_deref(),
        Some(active_tab_id.as_str())
    );
    close(restarted);
}

#[test]
fn surface_resume_point_round_trip_expected_pass() {
    use crate::multiplexer::types::{SurfaceBacking, TabBacking};
    use crate::service::surface::resume::write_surface_resume;
    use crate::service::surface::runtime::SurfaceResume;

    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let workspace_id = instance.fixture.service.mux.read().list_workspaces()[0]
        .id
        .clone();
    let tab_id = instance
        .fixture
        .service
        .create_named_tab(&workspace_id, "Browser")
        .unwrap();
    instance
        .fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .backing = TabBacking::Surface(SurfaceBacking {
        plugin_id: "browser".into(),
        surface_id: "browser".into(),
        instance_id: "inst-1".into(),
        owner_label: true,
        resume_url: None,
        resume_profile: None,
    });
    let resume = SurfaceResume {
        url: Some("https://mail.example.com/inbox#thread/42".into()),
        profile: Some("work".into()),
    };
    assert_eq!(
        write_surface_resume(
            &instance.fixture.service,
            &instance.fixture.service.mux,
            &tab_id,
            &resume
        ),
        Some(true)
    );

    let restarted = instance.restart(root.path());
    let restored = restarted
        .fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .and_then(|tab| tab.surface().cloned())
        .expect("surface tab restored");
    assert_eq!(
        restored.resume_url.as_deref(),
        Some("https://mail.example.com/inbox#thread/42")
    );
    assert_eq!(restored.resume_profile.as_deref(), Some("work"));
    assert!(restored.owner_label);
    close(restarted);
}
