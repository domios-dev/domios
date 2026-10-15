use std::sync::mpsc;
use std::time::Duration;

use codeterm_service_core::service::session_persist::{
    flush_session_persistence, install_storage_desk_adapter, stop_worker, PERSISTENCE_DEADLINE,
};
use codeterm_service_core::storage::{Storage, StorageConfig};
use tauri::Listener;

use crate::TeamExt;

#[test]
fn service_created_tab_is_committed_and_restores_from_a_fresh_storage_owner() {
    let (mut mux, lazy_agent_tab_id) =
        codeterm_service_core::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&lazy_agent_tab_id)
        .expect("initial Tab")
        .agent_type = Some("codex".into());
    let fixture = crate::test_fixture::ServiceFixture::new(mux);
    let root = fixture.temp.path().join("desk-reopen");
    let storage = Storage::open(&root, StorageConfig::default()).expect("Storage owner");
    let adapter =
        codeterm_service_core::session::store::StorageDeskAdapter::from_storage(storage.clone());
    let initial = fixture.service.mux.read().to_session_snapshot();
    adapter
        .import_snapshot(&initial, Vec::new())
        .expect("import starting desk");
    install_storage_desk_adapter(
        fixture._app.handle(),
        codeterm_service_core::session::store::StorageDeskAdapter::from_storage(storage.clone()),
    )
    .expect("install persistence backend");

    let (event_tx, event_rx) = mpsc::channel();
    let listener = fixture._app.listen("mux-state-changed", move |_| {
        let _ = event_tx.send(());
    });
    let flush_mutation = || {
        event_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("coalesced mux event follows the persistence submission");
        flush_session_persistence(fixture._app.handle()).expect("flush desk worker");
    };
    let workspace_id = fixture.service.mux.read().list_workspaces()[0].id.clone();
    let tab_id = fixture
        .service
        .create_tab_with_activate(&workspace_id, true)
        .expect("create through ServiceContext");
    fixture
        .service
        .rename_tab(&tab_id, "CommittedAgent")
        .expect("rename through ServiceContext");
    let moved_workspace = fixture
        .service
        .create_workspace("Committed workspace".into(), None, false)
        .expect("create workspace through ServiceContext");
    fixture
        .service
        .move_tab(&tab_id, &workspace_id, &moved_workspace.id)
        .expect("move through ServiceContext");
    fixture
        .service
        .set_tab_description(&tab_id, "desk description")
        .expect("update description through ServiceContext");
    let second_tab = fixture
        .service
        .create_tab_with_activate(&moved_workspace.id, false)
        .expect("create layout sibling through ServiceContext");
    fixture
        .service
        .reorder_tab(&moved_workspace.id, &[second_tab.clone(), tab_id.clone()])
        .expect("reorder layout through ServiceContext");
    let nonce = "committed-session-nonce";
    if let Some(tab) = fixture.service.mux.write().get_tab_mut(&tab_id) {
        tab.agent_type = Some("codex".into());
        tab.role = Some(codeterm_service_core::multiplexer::types::AgentRole::Worker);
        tab.session_binding_nonce = Some(nonce.into());
    }
    fixture
        .service
        .set_agent_session_id(&tab_id, Some("committed-session".into()));
    if let Some(tab) = fixture.service.mux.write().get_tab_mut(&tab_id) {
        tab.reported_session_binding_nonce = Some(nonce.into());
    }
    fixture.service.emit_state_changed();
    let transient_tab = fixture
        .service
        .create_tab_with_activate(&workspace_id, false)
        .expect("create tab to close");
    fixture
        .service
        .close_tab(&transient_tab)
        .expect("close through ServiceContext");
    let closing_workspace = fixture
        .service
        .create_workspace("Workspace to close".into(), None, false)
        .expect("create workspace through ServiceContext");
    flush_mutation();

    let committed = adapter.state().expect("committed desk state");
    assert!(committed.tab_revisions.contains_key(&tab_id));
    assert!(committed.tab_revisions.contains_key(&second_tab));
    assert!(!committed.tab_revisions.contains_key(&transient_tab));
    assert!(committed
        .snapshot
        .workspaces
        .iter()
        .any(|workspace| workspace.record.id.as_str() == moved_workspace.id.as_str()));
    let moved_snapshot = committed
        .snapshot
        .workspaces
        .iter()
        .find(|workspace| workspace.record.id.as_str() == moved_workspace.id.as_str())
        .expect("moved workspace row");
    let second_layout_tab = moved_snapshot
        .tabs
        .iter()
        .find(|tab| tab.id == second_tab)
        .expect("layout sibling row");
    let moved_layout_tab = moved_snapshot
        .tabs
        .iter()
        .find(|tab| tab.id == tab_id)
        .expect("moved layout row");
    assert!(second_layout_tab.sort_order < moved_layout_tab.sort_order);
    assert!(
        committed
            .snapshot
            .agent_session_identities
            .iter()
            .any(|identity| identity.provider == "codex"
                && identity.session_id == "committed-session")
    );
    assert!(committed
        .snapshot
        .workspaces
        .iter()
        .any(|workspace| workspace.record.id.as_str() == closing_workspace.id.as_str()));

    fixture
        .service
        .close_workspace(&closing_workspace.id)
        .expect("close workspace through ServiceContext");
    flush_mutation();
    assert!(!adapter
        .state()
        .unwrap()
        .snapshot
        .workspaces
        .iter()
        .any(|workspace| workspace.record.id.as_str() == closing_workspace.id.as_str()));

    let owner_tab = fixture
        .service
        .create_tab_with_activate(&workspace_id, false)
        .expect("create Team owner tab through ServiceContext");
    fixture
        .service
        .mux
        .write()
        .set_orchestrator_role(&owner_tab)
        .expect("prepare Team owner role");
    let created_team = fixture
        .service
        .create_team_plan_for_orchestrator(
            "Committed Team plan".into(),
            root.display().to_string(),
            codeterm_service_core::plan::OrchestratorRef::from_tab(&owner_tab),
            Vec::new(),
            None,
            1,
        )
        .expect("create Team through ServiceContext");
    flush_mutation();
    assert!(adapter
        .state()
        .unwrap()
        .snapshot
        .teams
        .iter()
        .any(|team| team.id == created_team.team_id));
    fixture
        .service
        .rename_team(
            &created_team.team_id,
            "Committed Team rename".into(),
            codeterm_service_core::service::TeamMutationCaller::Human,
        )
        .expect("rename Team through ServiceContext");
    flush_mutation();
    assert!(adapter
        .state()
        .unwrap()
        .snapshot
        .teams
        .iter()
        .any(|team| team.id == created_team.team_id
            && team.name.as_deref() == Some("Committed Team rename")));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Team close runtime");
    runtime
        .block_on(fixture.service.close_team(
            &created_team.team_id,
            codeterm_service_core::service::TeamMutationCaller::Human,
            true,
        ))
        .expect("close Team through ServiceContext");
    flush_mutation();
    assert!(!adapter
        .state()
        .unwrap()
        .snapshot
        .teams
        .iter()
        .any(|team| team.id == created_team.team_id));

    fixture
        .service
        .focus_workspace(&moved_workspace.id)
        .expect("change workspace state through ServiceContext");
    flush_mutation();
    let committed = adapter.state().expect("all committed desk domains");
    assert_eq!(
        committed.snapshot.active_workspace_id,
        Some(moved_workspace.id.clone())
    );
    fixture.service.emit_state_changed();
    event_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("second coalesced mux event");
    flush_session_persistence(fixture._app.handle()).expect("flush unchanged desk");
    assert_eq!(adapter.state().unwrap().revision, committed.revision);
    stop_worker(fixture._app.handle()).expect("stop desk worker");
    storage
        .shutdown()
        .wait(PERSISTENCE_DEADLINE)
        .expect("close original Storage owner");

    let reopened = Storage::open(&root, StorageConfig::default()).expect("fresh Storage owner");
    let reopened_adapter =
        codeterm_service_core::session::store::StorageDeskAdapter::from_storage(reopened.clone());
    let restored = reopened_adapter.state().expect("restored desk state");
    assert!(restored.tab_revisions.contains_key(&tab_id));
    assert!(!restored
        .snapshot
        .workspaces
        .iter()
        .any(|workspace| { workspace.record.id.as_str() == closing_workspace.id.as_str() }));
    assert!(!restored
        .snapshot
        .teams
        .iter()
        .any(|team| team.id == created_team.team_id));
    assert!(restored
        .snapshot
        .workspaces
        .iter()
        .flat_map(|workspace| &workspace.tabs)
        .any(|tab| tab.id == tab_id));
    assert!(!restored
        .snapshot
        .workspaces
        .iter()
        .flat_map(|workspace| &workspace.tabs)
        .any(|tab| tab.id == transient_tab));
    let restored_tab = restored
        .snapshot
        .workspaces
        .iter()
        .flat_map(|workspace| &workspace.tabs)
        .find(|tab| tab.id == tab_id)
        .expect("renamed and moved tab restored");
    assert_eq!(restored_tab.title, "CommittedAgent");
    assert_eq!(
        restored_tab.description.as_deref(),
        Some("desk description")
    );
    assert_eq!(
        restored.snapshot.active_workspace_id,
        Some(moved_workspace.id.clone())
    );
    let restored_layout = restored
        .snapshot
        .workspaces
        .iter()
        .find(|workspace| workspace.record.id.as_str() == moved_workspace.id.as_str())
        .expect("moved workspace restores")
        .tabs
        .iter()
        .map(|tab| (tab.id.as_str(), tab.sort_order))
        .collect::<std::collections::HashMap<_, _>>();
    assert!(restored_layout[second_tab.as_str()] < restored_layout[tab_id.as_str()]);
    assert!(
        reopened_adapter
            .identity_catalog()
            .expect("restored agent identity catalogue")
            .iter()
            .any(|identity| identity.provider == "codex"
                && identity.session_id == "committed-session")
    );
    let restored_mux =
        codeterm_service_core::multiplexer::state::MultiplexerState::from_session_snapshot(
            &reopened_adapter
                .snapshot()
                .expect("session restore snapshot"),
        );
    let lazy_agent = restored_mux
        .get_tab(&lazy_agent_tab_id)
        .expect("sessionless agent tab remains restorable");
    assert_eq!(lazy_agent.agent_type.as_deref(), Some("codex"));
    assert!(lazy_agent.agent_session_id.is_none());
    reopened
        .shutdown()
        .wait(PERSISTENCE_DEADLINE)
        .expect("close fresh Storage owner");
    fixture._app.unlisten(listener);
}
