use super::*;
use crate::storage::{Storage, StorageConfig};
use std::sync::mpsc::TryRecvError;
use std::sync::Arc;
use tauri::Listener;

fn test_adapter(root: &std::path::Path) -> crate::session::store::StorageDeskAdapter {
    let storage = Storage::open(root, StorageConfig::default()).expect("Storage owner");
    crate::session::store::StorageDeskAdapter::from_storage(storage)
}

fn test_worker(
    writer: impl FnMut(u64) -> Result<SaveOutcome, String> + Send + 'static,
) -> SessionPersistenceWorker {
    let mailbox = Arc::new(PersistenceMailbox::default());
    let worker = SessionPersistenceWorker {
        mailbox: mailbox.clone(),
        next_revision: Arc::new(AtomicU64::new(0)),
        submission_order: Arc::new(parking_lot::Mutex::new(())),
    };
    std::thread::spawn(move || {
        run_worker(mailbox, None, PersistenceBackend::Test(Box::new(writer)))
    });
    worker
}

fn pending_worker() -> SessionPersistenceWorker {
    let mailbox = Arc::new(PersistenceMailbox::default());
    let worker = SessionPersistenceWorker {
        mailbox: mailbox.clone(),
        next_revision: Arc::new(AtomicU64::new(0)),
        submission_order: Arc::new(parking_lot::Mutex::new(())),
    };
    std::thread::spawn(move || run_worker(mailbox, None, PersistenceBackend::Pending));
    worker
}

fn stop_test_worker(worker: &SessionPersistenceWorker) {
    let (response, completion) = mpsc::channel();
    worker.configure(WorkerMessage::Stop(response)).unwrap();
    completion.recv_timeout(Duration::from_secs(5)).unwrap();
}

pub(crate) fn assert_unavailable_rejects_persistence_without_a_commit() {
    let (commit_tx, commit_rx) = mpsc::channel();
    let worker = test_worker(move |revision| {
        commit_tx.send(revision).unwrap();
        Ok(SaveOutcome::Saved)
    });
    worker
        .configure(WorkerMessage::Unavailable("Storage unavailable".into()))
        .unwrap();

    let error = worker
        .submit_snapshot(test_snapshot(1), SaveIntent::PreserveDesk, true)
        .unwrap_err();

    assert_eq!(error, "Storage unavailable");
    assert!(matches!(
        commit_rx.try_recv(),
        Err(TryRecvError::Empty | TryRecvError::Disconnected)
    ));
    stop_test_worker(&worker);
}

#[test]
fn mutation_submitted_before_storage_init_is_committed_and_restored_after_restart() {
    let root = tempfile::tempdir().expect("storage root");
    let authority = crate::startup::open_instance_storage_at(root.path());
    let storage = authority
        .require_ready()
        .expect("real startup initializes storage")
        .clone();
    let adapter = crate::session::store::StorageDeskAdapter::from_storage(storage.clone());
    assert!(adapter
        .snapshot()
        .expect("real startup seeds the empty desk")
        .workspaces
        .is_empty());
    let worker = pending_worker();
    let (state, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = state.list_workspaces()[0].id.clone();
    let mux = Arc::new(codeterm_util::lock_hold::TimedRwLock::new(
        "preinit-persistence-test",
        state,
    ));

    mux.write()
        .rename_workspace(&workspace_id, "Mutation before storage init")
        .expect("pre-init mutation");
    worker
        .submit_current(&mux, SaveIntent::PreserveDesk, false)
        .expect("retain first pre-init revision");
    mux.write()
        .rename_workspace(&workspace_id, "Latest pre-init mutation")
        .expect("latest pre-init mutation");
    worker
        .submit_current(&mux, SaveIntent::PreserveDesk, false)
        .expect("retain latest pre-init revision");

    worker
        .configure(WorkerMessage::InstallStorage(adapter))
        .expect("initialize Storage desk backend");
    worker.flush().expect("flush retained pre-init mutation");
    stop_test_worker(&worker);
    storage
        .shutdown()
        .wait(PERSISTENCE_DEADLINE)
        .expect("close first Storage owner");
    drop(authority);

    let restarted_authority = crate::startup::open_instance_storage_at(root.path());
    let restarted_storage = restarted_authority
        .require_ready()
        .expect("restart opens initialized Storage")
        .clone();
    let restarted =
        crate::session::store::StorageDeskAdapter::from_storage(restarted_storage.clone());
    let snapshot = restarted.snapshot().expect("restored Storage desk");
    let restored = crate::multiplexer::state::MultiplexerState::from_session_snapshot(&snapshot);
    assert_eq!(
        restored.list_workspaces()[0].name,
        "Latest pre-init mutation",
        "the latest mutation submitted before backend initialization survives restart"
    );
    restarted_storage
        .shutdown()
        .wait(PERSISTENCE_DEADLINE)
        .expect("close restarted Storage owner");
    drop(restarted_authority);
}

fn test_snapshot(revision: u64) -> crate::session::SessionSnapshot {
    let mut snapshot = desk_delta_snapshot(0);
    snapshot.timestamp = revision;
    snapshot
}

#[test]
fn session_persist_projection_drops_only_orphan_teams_deterministically() {
    let mut snapshot = desk_delta_snapshot(1);
    let valid_team = codeterm_types::team::Team::new("ws-0", 1);
    let orphan_team = codeterm_types::team::Team::new("removed-workspace", 2);
    let valid_id = valid_team.id.clone();
    snapshot.teams = vec![valid_team, orphan_team];

    let first = crate::session::store::project_snapshot(&snapshot, Vec::new()).unwrap();
    let second = crate::session::store::project_snapshot(&snapshot, Vec::new()).unwrap();

    assert_eq!(first, second);
    assert_eq!(first.workspaces.len(), 1);
    assert_eq!(
        first
            .teams
            .iter()
            .map(|team| team.id.as_str())
            .collect::<Vec<_>>(),
        vec![valid_id.as_str()]
    );
}

#[test]
fn runtime_team_projection_preserves_persisted_order_and_appends_new_teams() {
    use codeterm_types::team::{Team, TeamLifecycle};

    let mut persisted = desk_delta_snapshot(1);
    persisted.teams = ["team-z", "team-a"]
        .into_iter()
        .enumerate()
        .map(|(index, id)| Team {
            id: id.into(),
            name: None,
            lifecycle: TeamLifecycle::Active,
            created_at: index as u64,
            workspace_id: "ws-0".into(),
        })
        .collect();
    let mut mux = crate::multiplexer::state::MultiplexerState::from_session_snapshot(&persisted);
    assert_eq!(
        mux.to_session_snapshot()
            .teams
            .iter()
            .map(|team| team.id.as_str())
            .collect::<Vec<_>>(),
        vec!["team-z", "team-a"]
    );

    mux.upsert_team(Team {
        id: "team-new".into(),
        name: None,
        lifecycle: TeamLifecycle::Forming,
        created_at: 3,
        workspace_id: "ws-0".into(),
    });
    let projected =
        crate::session::store::project_snapshot(&mux.to_session_snapshot(), Vec::new()).unwrap();
    assert_eq!(
        projected
            .teams
            .iter()
            .map(|team| team.id.as_str())
            .collect::<Vec<_>>(),
        vec!["team-z", "team-a", "team-new"]
    );
}

fn queue_barrier(
    worker: &SessionPersistenceWorker,
    snapshot: crate::session::SessionSnapshot,
) -> Receiver<Result<SaveOutcome, String>> {
    let _submission = worker.submission_order.lock();
    let (response, completion) = mpsc::channel();
    let revision = worker.next_revision.fetch_add(1, Ordering::Relaxed) + 1;
    worker
        .mailbox
        .push(WorkerMessage::Persist(PersistRequest {
            revision,
            snapshot,
            intent: SaveIntent::PreserveDesk,
            responses: vec![response],
        }))
        .unwrap();
    completion
}

#[test]
fn desk_facade_coalesces_by_revision() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (written_tx, written_rx) = mpsc::channel();
    let writer = move |revision| {
        written_tx.send(revision).unwrap();
        if revision == 1 {
            entered_tx.send(()).unwrap();
            release_rx
                .recv_timeout(Duration::from_secs(5))
                .map_err(|error| error.to_string())?;
        }
        Ok(SaveOutcome::Saved)
    };
    let worker = test_worker(writer);

    assert!(worker
        .submit_snapshot(test_snapshot(1), SaveIntent::PreserveDesk, false)
        .unwrap()
        .is_none());
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    worker
        .submit_snapshot(test_snapshot(2), SaveIntent::PreserveDesk, false)
        .unwrap();
    worker
        .submit_snapshot(test_snapshot(3), SaveIntent::PreserveDesk, false)
        .unwrap();
    release_tx.send(()).unwrap();

    assert_eq!(written_rx.recv_timeout(Duration::from_secs(5)).unwrap(), 1);
    assert_eq!(written_rx.recv_timeout(Duration::from_secs(5)).unwrap(), 3);
    stop_test_worker(&worker);
}

#[test]
fn desk_facade_flush_barrier_waits_off_runtime() {
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let writer = move |_revision| {
        started_tx.send(()).unwrap();
        release_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        Ok(SaveOutcome::Saved)
    };
    let worker = test_worker(writer);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let barrier_worker = worker.clone();
    let barrier = runtime.spawn_blocking(move || {
        barrier_worker.submit_snapshot(test_snapshot(1), SaveIntent::PreserveDesk, true)
    });

    runtime.block_on(async move {
        tokio::task::spawn_blocking(move || {
            started_rx.recv_timeout(Duration::from_secs(5)).unwrap()
        })
        .await
        .unwrap();
        assert!(!barrier.is_finished());
        release_tx.send(()).unwrap();
        assert!(matches!(
            barrier.await.unwrap(),
            Ok(Some(SaveOutcome::Saved))
        ));
    });
    stop_test_worker(&worker);
}

#[test]
fn desk_facade_held_writer_keeps_ipc_responsive() {
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (written_tx, written_rx) = mpsc::channel();
    let writer = move |revision| {
        written_tx.send(revision).unwrap();
        if revision == 1 {
            started_tx.send(()).unwrap();
            release_rx
                .recv_timeout(Duration::from_secs(5))
                .map_err(|error| error.to_string())?;
        }
        Ok(SaveOutcome::Saved)
    };
    let worker = test_worker(writer);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    runtime.block_on(async {
        assert!(worker
            .submit_snapshot(test_snapshot(1), SaveIntent::PreserveDesk, false)
            .unwrap()
            .is_none());
        tokio::task::spawn_blocking(move || {
            started_rx.recv_timeout(Duration::from_secs(5)).unwrap()
        })
        .await
        .unwrap();
        assert!(worker
            .submit_snapshot(test_snapshot(2), SaveIntent::PreserveDesk, false)
            .unwrap()
            .is_none());
        let (ipc_tx, ipc_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let _ = ipc_tx.send(());
        });
        ipc_rx.await.unwrap();
    });

    release_tx.send(()).unwrap();
    assert_eq!(written_rx.recv_timeout(Duration::from_secs(5)).unwrap(), 1);
    assert_eq!(written_rx.recv_timeout(Duration::from_secs(5)).unwrap(), 2);
    stop_test_worker(&worker);
}

#[test]
fn desk_facade_never_reports_durable_before_commit() {
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let writer = move |_revision| {
        started_tx.send(()).unwrap();
        release_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        Ok(SaveOutcome::Saved)
    };
    let worker = test_worker(writer);
    let completion = queue_barrier(&worker, test_snapshot(1));
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();

    assert!(matches!(completion.try_recv(), Err(TryRecvError::Empty)));
    release_tx.send(()).unwrap();
    assert!(matches!(
        completion.recv_timeout(Duration::from_secs(5)).unwrap(),
        Ok(SaveOutcome::Saved)
    ));
    stop_test_worker(&worker);
}

#[test]
fn failed_boot_resync_is_retried_by_the_next_mutation() {
    let (attempt_tx, attempt_rx) = mpsc::channel();
    let writer = move |revision| {
        attempt_tx.send(revision).unwrap();
        if revision == 1 {
            Err("temporary commit failure".to_string())
        } else {
            Ok(SaveOutcome::Saved)
        }
    };
    let worker = test_worker(writer);

    assert_eq!(
        worker
            .submit_snapshot(test_snapshot(1), SaveIntent::AuthorizedReduction, true)
            .unwrap_err(),
        "temporary commit failure"
    );
    assert_eq!(attempt_rx.recv_timeout(Duration::from_secs(5)).unwrap(), 1);

    assert!(matches!(
        worker.submit_snapshot(test_snapshot(2), SaveIntent::PreserveDesk, true),
        Ok(Some(SaveOutcome::Saved))
    ));
    assert_eq!(attempt_rx.recv_timeout(Duration::from_secs(5)).unwrap(), 2);
    assert!(matches!(attempt_rx.try_recv(), Err(TryRecvError::Empty)));
    worker.flush().unwrap();
    stop_test_worker(&worker);
}

#[test]
fn storage_desk_app_adapter_imports_and_flushes_exact_selection() {
    let root = tempfile::tempdir().unwrap();
    let adapter = test_adapter(root.path());
    let snapshot = crate::session::store::SessionSnapshot {
        version: "test".into(),
        timestamp: 17,
        active_workspace_id: Some("ws-a".into()),
        teams: vec![],
        workspaces: vec![crate::session::store::WorkspaceSnapshot {
            id: "ws-a".into(),
            name: "Alpha".into(),
            tabs: vec![],
            active_tab_id: None,
            cwd: None,
            provisioned_cwd: None,
            protected: true,
            sort_order: 8,
            task_keys: Default::default(),
        }],
    };

    adapter.import_snapshot(&snapshot, Vec::new()).unwrap();
    let initial = adapter.state().unwrap();
    let workspace_revision = initial.workspace_revisions["ws-a"];
    let committed_revision = adapter
        .rename_workspace("ws-a".into(), workspace_revision, "Renamed".into())
        .unwrap();
    assert!(committed_revision > initial.revision);
    assert!(adapter
        .rename_workspace("ws-a".into(), workspace_revision, "Stale".into())
        .is_err());
    let mut backup = snapshot.clone();
    backup.timestamp = 99;
    backup.active_workspace_id = Some("ws-b".into());
    backup.workspaces[0].name = "Stale backup".into();
    backup
        .workspaces
        .push(crate::session::store::WorkspaceSnapshot {
            id: "ws-b".into(),
            name: "Restored".into(),
            tabs: vec![],
            active_tab_id: None,
            cwd: None,
            provisioned_cwd: None,
            protected: false,
            sort_order: 19,
            task_keys: Default::default(),
        });
    adapter.additive_restore(&backup, Vec::new()).unwrap();
    adapter.additive_restore(&backup, Vec::new()).unwrap();
    adapter.flush().unwrap();
    let restored = adapter.snapshot().unwrap();

    assert_eq!(restored.active_workspace_id, Some("ws-a".into()));
    assert_eq!(restored.workspaces[0].active_tab_id, None);
    assert_eq!(restored.workspaces[0].name, "Renamed");
    assert_eq!(restored.workspaces[0].cwd, None);
    assert_eq!(restored.workspaces[0].provisioned_cwd, None);
    assert_eq!(restored.workspaces[0].sort_order, 8);
    assert!(restored.workspaces[0].protected);
    assert_eq!(restored.timestamp, 17);
    assert_eq!(restored.workspaces.len(), 2);
    assert_eq!(restored.workspaces[1].name, "Restored");
}

#[test]
fn storage_desk_restores_live_provider_sessions_for_each_provider() {
    let root = tempfile::tempdir().unwrap();
    let adapter = test_adapter(root.path());
    let (mut mux, parent_id) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&parent_id).unwrap();
    let providers = ["claude", "codex", "opencode", "grok"];
    let mut expected = Vec::new();

    for provider in providers {
        let tab_id = mux.create_tab(&workspace_id).unwrap();
        mux.set_tab_role(
            &tab_id,
            crate::multiplexer::types::AgentRole::Worker,
            Some(&parent_id),
        )
        .unwrap();
        let session_id = format!("{provider}-session");
        let nonce = format!("{provider}-nonce");
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some(provider.into());
        tab.agent_type_locked = true;
        tab.agent_session_id = Some(session_id.clone());
        tab.session_binding_nonce = Some(nonce.clone());
        tab.reported_session_binding_nonce = Some(nonce);
        expected.push((tab_id, provider, session_id));
    }

    let snapshot = mux.to_session_snapshot();
    let identities = desk_agent_session_identities(&snapshot);
    adapter.import_snapshot(&snapshot, identities).unwrap();
    adapter.flush().unwrap();
    assert_eq!(adapter.identity_catalog().unwrap().len(), providers.len());
    let restored_snapshot = adapter.snapshot().unwrap();
    let restored =
        crate::multiplexer::state::MultiplexerState::from_session_snapshot(&restored_snapshot);

    for (tab_id, provider, session_id) in expected {
        let tab = restored.get_tab(&tab_id).expect("restored agent tab");
        assert_eq!(tab.agent_type.as_deref(), Some(provider));
        assert_eq!(tab.agent_session_id.as_deref(), Some(session_id.as_str()));
        assert!(tab.agent_type_locked);
    }
}

fn desk_delta_snapshot(workspace_count: usize) -> crate::session::store::SessionSnapshot {
    crate::session::store::SessionSnapshot {
        version: "test".into(),
        timestamp: 17,
        active_workspace_id: None,
        teams: vec![],
        workspaces: (0..workspace_count)
            .map(|index| crate::session::store::WorkspaceSnapshot {
                id: format!("ws-{index}"),
                name: format!("Workspace {index}"),
                tabs: vec![],
                active_tab_id: None,
                cwd: None,
                provisioned_cwd: None,
                protected: false,
                sort_order: index as i32,
                task_keys: Default::default(),
            })
            .collect(),
    }
}

#[test]
fn desk_delta_rename_touches_three_rows_in_a_large_inventory() {
    let root = tempfile::tempdir().unwrap();
    let adapter = test_adapter(root.path());
    let mut snapshot = desk_delta_snapshot(512);
    adapter.import_snapshot(&snapshot, Vec::new()).unwrap();
    let initial = adapter.state().unwrap();
    snapshot.workspaces[255].name = "Renamed".into();

    let committed = adapter
        .delta_snapshot(
            &snapshot,
            Vec::new(),
            initial.revision,
            crate::session::store::SaveIntent::PreserveDesk,
        )
        .unwrap()
        .wait(std::time::Duration::from_secs(30))
        .unwrap();

    assert_eq!(committed.receipt.rows_written, 3);
    let saved = adapter.state().unwrap();
    assert_eq!(saved.snapshot.workspaces.len(), 512);
    assert_eq!(saved.snapshot.workspaces[255].record.name, "Renamed");
}

#[test]
fn desk_delta_adjacent_reorder_normalizes_the_full_large_inventory() {
    let root = tempfile::tempdir().unwrap();
    let adapter = test_adapter(root.path());
    let mut snapshot = desk_delta_snapshot(512);
    adapter.import_snapshot(&snapshot, Vec::new()).unwrap();
    let initial = adapter.state().unwrap();
    snapshot.workspaces.swap(255, 256);
    snapshot.workspaces[255].sort_order = 255;
    snapshot.workspaces[256].sort_order = 256;

    let committed = adapter
        .delta_snapshot(
            &snapshot,
            Vec::new(),
            initial.revision,
            crate::session::store::SaveIntent::PreserveDesk,
        )
        .unwrap()
        .wait(std::time::Duration::from_secs(30))
        .unwrap();

    assert!(committed.receipt.rows_written >= 1024);
    let saved = adapter.state().unwrap();
    assert_eq!(saved.snapshot.workspaces[255].record.id.as_str(), "ws-256");
    assert_eq!(saved.snapshot.workspaces[256].record.id.as_str(), "ws-255");
}

#[test]
fn desk_delta_preserves_omissions_and_rejects_stale_reduction() {
    let root = tempfile::tempdir().unwrap();
    let adapter = test_adapter(root.path());
    let mut snapshot = desk_delta_snapshot(2);
    adapter.import_snapshot(&snapshot, Vec::new()).unwrap();
    let initial = adapter.state().unwrap();
    snapshot.workspaces.pop();

    adapter
        .delta_snapshot(
            &snapshot,
            Vec::new(),
            initial.revision,
            crate::session::store::SaveIntent::PreserveDesk,
        )
        .unwrap()
        .wait(std::time::Duration::from_secs(30))
        .unwrap();
    assert_eq!(adapter.state().unwrap().snapshot.workspaces.len(), 2);

    let mut newer = desk_delta_snapshot(2);
    newer.workspaces[0].name = "Current".into();
    adapter
        .delta_snapshot(
            &newer,
            Vec::new(),
            initial.revision,
            crate::session::store::SaveIntent::PreserveDesk,
        )
        .unwrap()
        .wait(std::time::Duration::from_secs(30))
        .unwrap();
    let stale = adapter
        .delta_snapshot(
            &snapshot,
            Vec::new(),
            initial.revision,
            crate::session::store::SaveIntent::PreserveDesk,
        )
        .unwrap()
        .wait(std::time::Duration::from_secs(30));

    assert!(stale.is_err());
    let saved = adapter.state().unwrap();
    assert_eq!(saved.snapshot.workspaces.len(), 2);
    assert_eq!(saved.snapshot.workspaces[0].record.name, "Current");
}

#[test]
fn desk_delta_authorized_reduction_applies_a_workspace_close() {
    let root = tempfile::tempdir().unwrap();
    let adapter = test_adapter(root.path());
    let mut snapshot = desk_delta_snapshot(2);
    adapter.import_snapshot(&snapshot, Vec::new()).unwrap();
    let initial = adapter.state().unwrap();
    snapshot.workspaces.pop();

    adapter
        .delta_snapshot(
            &snapshot,
            Vec::new(),
            initial.revision,
            crate::session::store::SaveIntent::AuthorizedReduction,
        )
        .unwrap()
        .wait(std::time::Duration::from_secs(30))
        .unwrap();

    let saved = adapter.state().unwrap();
    assert_eq!(saved.snapshot.workspaces.len(), 1);
    assert_eq!(saved.snapshot.workspaces[0].record.id.as_str(), "ws-0");
}

#[test]
fn service_created_tab_is_committed_and_restores_from_a_fresh_storage_owner() {
    let (mut mux, lazy_agent_tab_id) = crate::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&lazy_agent_tab_id)
        .expect("initial Tab")
        .agent_type = Some("codex".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let root = fixture.temp.path().join("desk-reopen");
    let storage = Storage::open(&root, StorageConfig::default()).expect("Storage owner");
    let adapter = crate::session::store::StorageDeskAdapter::from_storage(storage.clone());
    let initial = fixture.service.mux.read().to_session_snapshot();
    adapter
        .import_snapshot(&initial, Vec::new())
        .expect("import starting desk");
    install_storage_desk_adapter(
        fixture._app.handle(),
        crate::session::store::StorageDeskAdapter::from_storage(storage.clone()),
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
        tab.role = Some(crate::multiplexer::types::AgentRole::Worker);
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
            crate::plan::OrchestratorRef::from_tab(&owner_tab),
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
            crate::service::TeamMutationCaller::Human,
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
            crate::service::TeamMutationCaller::Human,
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
        crate::session::store::StorageDeskAdapter::from_storage(reopened.clone());
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
    let restored_mux = crate::multiplexer::state::MultiplexerState::from_session_snapshot(
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
