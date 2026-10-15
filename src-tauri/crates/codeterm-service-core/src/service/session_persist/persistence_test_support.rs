use super::*;
use std::sync::mpsc::TryRecvError;

pub(super) fn test_worker(
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

pub(super) fn stop_test_worker(worker: &SessionPersistenceWorker) {
    let (response, completion) = mpsc::channel();
    worker.configure(WorkerMessage::Stop(response)).unwrap();
    completion.recv_timeout(Duration::from_secs(5)).unwrap();
}

pub fn assert_unavailable_rejects_persistence_without_a_commit() {
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

pub(super) fn test_snapshot(revision: u64) -> crate::session::SessionSnapshot {
    let mut snapshot = desk_delta_snapshot(0);
    snapshot.timestamp = revision;
    snapshot
}

pub(super) fn desk_delta_snapshot(
    workspace_count: usize,
) -> crate::session::store::SessionSnapshot {
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
