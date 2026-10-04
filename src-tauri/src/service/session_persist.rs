use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use tauri::{Emitter, Manager};

use crate::multiplexer::commands::MuxState;
use crate::session::store::{SaveIntent, SaveOutcome};
use crate::session::SessionState;

const PERSISTENCE_DEADLINE: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionPersistUrgency {
    Prompt,
    Debounced,
}

#[derive(Clone)]
pub(crate) struct SessionPersistenceWorker {
    mailbox: Arc<PersistenceMailbox>,
    next_revision: Arc<AtomicU64>,
    submission_order: Arc<parking_lot::Mutex<()>>,
}

struct PersistRequest {
    revision: u64,
    snapshot: crate::session::SessionSnapshot,
    intent: SaveIntent,
    responses: Vec<Sender<Result<SaveOutcome, String>>>,
}

enum WorkerMessage {
    Persist(PersistRequest),
    InstallStorage(crate::session::store::StorageDeskAdapter),
    Unavailable(String),
    Stop(Sender<()>),
}

#[derive(Default)]
struct MailboxState {
    pending: VecDeque<WorkerMessage>,
    stopping: bool,
}

#[derive(Default)]
struct PersistenceMailbox {
    state: parking_lot::Mutex<MailboxState>,
    ready: parking_lot::Condvar,
}

impl PersistenceMailbox {
    fn abort(&self, error: &str) {
        let mut state = self.state.lock();
        state.stopping = true;
        for message in state.pending.drain(..) {
            match message {
                WorkerMessage::Persist(request) => {
                    for response in request.responses {
                        let _ = response.send(Err(error.to_string()));
                    }
                }
                WorkerMessage::Stop(response) => {
                    let _ = response.send(());
                }
                WorkerMessage::InstallStorage(_) | WorkerMessage::Unavailable(_) => {}
            }
        }
    }

    fn push(&self, message: WorkerMessage) -> Result<(), String> {
        let mut state = self.state.lock();
        if state.stopping {
            return Err("desk persistence worker is stopping".to_string());
        }
        if matches!(&message, WorkerMessage::Stop(_)) {
            state.stopping = true;
        }
        match message {
            WorkerMessage::Persist(mut request) => {
                let coalesce = matches!(
                    state.pending.back(),
                    Some(WorkerMessage::Persist(pending)) if pending.intent == request.intent
                );
                if coalesce {
                    let Some(WorkerMessage::Persist(pending)) = state.pending.back_mut() else {
                        unreachable!();
                    };
                    pending.revision = request.revision;
                    pending.snapshot = request.snapshot;
                    pending.responses.append(&mut request.responses);
                } else {
                    state.pending.push_back(WorkerMessage::Persist(request));
                }
            }
            message => state.pending.push_back(message),
        }
        self.ready.notify_one();
        Ok(())
    }

    fn pop(&self) -> WorkerMessage {
        let mut state = self.state.lock();
        loop {
            if let Some(message) = state.pending.pop_front() {
                return message;
            }
            self.ready.wait(&mut state);
        }
    }
}

struct PersistenceContext {
    app: tauri::AppHandle,
    mux: MuxState,
    session: SessionState,
}

enum PersistenceBackend {
    Json,
    Storage {
        adapter: crate::session::store::StorageDeskAdapter,
        revision: codeterm_storage::Revision,
    },
    Unavailable(String),
    #[cfg(test)]
    Test(Box<dyn FnMut(u64) -> Result<SaveOutcome, String> + Send>),
}

/// `Ok(RefusedReduction)` is the guard succeeding (fuller desk kept on disk,
/// already logged at error by the store); `Err` is a genuine persist failure.
pub(crate) fn write_session_snapshot(
    app: &tauri::AppHandle,
    mux: &MuxState,
) -> Result<SaveOutcome, String> {
    write_session_snapshot_with_intent(app, mux, SaveIntent::PreserveDesk)
}

pub(crate) fn write_session_snapshot_with_intent(
    app: &tauri::AppHandle,
    mux: &MuxState,
    intent: SaveIntent,
) -> Result<SaveOutcome, String> {
    let worker = ensure_worker(app)?;
    worker
        .submit_current(mux, intent, true)?
        .ok_or_else(|| "desk persistence barrier returned no outcome".to_string())
}

pub(crate) fn submit_session_snapshot_with_intent(
    app: &tauri::AppHandle,
    mux: &MuxState,
    intent: SaveIntent,
) -> Result<(), String> {
    let worker = ensure_worker(app)?;
    worker.submit_current(mux, intent, false).map(|_| ())
}

pub(crate) fn install_storage_desk_adapter(
    app: &tauri::AppHandle,
    adapter: crate::session::store::StorageDeskAdapter,
) -> Result<(), String> {
    let worker = ensure_worker(app)?;
    worker.configure(WorkerMessage::InstallStorage(adapter))
}

pub(crate) fn set_storage_unavailable(
    app: &tauri::AppHandle,
    reason: String,
) -> Result<(), String> {
    let worker = ensure_worker(app)?;
    worker.configure(WorkerMessage::Unavailable(reason))
}

pub(crate) fn start_worker(app: &tauri::AppHandle) -> Result<(), String> {
    ensure_worker(app).map(|_| ())
}

pub(crate) fn stop_worker(app: &tauri::AppHandle) -> Result<(), String> {
    let worker = ensure_worker(app)?;
    let (response, completion) = mpsc::channel();
    worker.configure(WorkerMessage::Stop(response))?;
    completion
        .recv_timeout(PERSISTENCE_DEADLINE)
        .map_err(|error| format!("desk persistence worker shutdown was not confirmed: {error}"))
}

fn ensure_worker(app: &tauri::AppHandle) -> Result<SessionPersistenceWorker, String> {
    if let Some(worker) = app.try_state::<SessionPersistenceWorker>() {
        return Ok(worker.inner().clone());
    }
    let mux = app
        .try_state::<MuxState>()
        .ok_or_else(|| "Multiplexer state not registered".to_string())?
        .inner()
        .clone();
    let session = app
        .try_state::<SessionState>()
        .ok_or_else(|| "SessionState not registered".to_string())?
        .inner()
        .clone();
    let context = PersistenceContext {
        app: app.clone(),
        mux,
        session,
    };
    let mailbox = Arc::new(PersistenceMailbox::default());
    let worker = SessionPersistenceWorker {
        mailbox: mailbox.clone(),
        next_revision: Arc::new(AtomicU64::new(0)),
        submission_order: Arc::new(parking_lot::Mutex::new(())),
    };
    if app.manage(worker.clone()) {
        let thread_mailbox = mailbox.clone();
        match std::thread::Builder::new()
            .name("desk-persistence".into())
            .spawn(move || run_worker(thread_mailbox, Some(context), PersistenceBackend::Json))
        {
            Ok(_) => Ok(worker),
            Err(error) => {
                let reason = format!("desk persistence worker failed to start: {error}");
                mailbox.abort(&reason);
                Err(reason)
            }
        }
    } else {
        app.try_state::<SessionPersistenceWorker>()
            .map(|worker| worker.inner().clone())
            .ok_or_else(|| "desk persistence worker registration raced".to_string())
    }
}

impl SessionPersistenceWorker {
    fn submit_current(
        &self,
        mux: &MuxState,
        intent: SaveIntent,
        wait: bool,
    ) -> Result<Option<SaveOutcome>, String> {
        let completion = {
            let _submission = self.submission_order.lock();
            let snapshot = mux.read().to_session_snapshot();
            self.submit_snapshot_ordered(snapshot, intent, wait)?
        };
        wait_for_completion(completion)
    }

    #[cfg(test)]
    fn submit_snapshot(
        &self,
        snapshot: crate::session::SessionSnapshot,
        intent: SaveIntent,
        wait: bool,
    ) -> Result<Option<SaveOutcome>, String> {
        let completion = {
            let _submission = self.submission_order.lock();
            self.submit_snapshot_ordered(snapshot, intent, wait)?
        };
        wait_for_completion(completion)
    }

    fn submit_snapshot_ordered(
        &self,
        snapshot: crate::session::SessionSnapshot,
        intent: SaveIntent,
        wait: bool,
    ) -> Result<Option<Receiver<Result<SaveOutcome, String>>>, String> {
        let (response, completion) = if wait {
            let (sender, receiver) = mpsc::channel();
            (Some(sender), Some(receiver))
        } else {
            (None, None)
        };
        let revision = self.next_revision.fetch_add(1, Ordering::Relaxed) + 1;
        self.mailbox.push(WorkerMessage::Persist(PersistRequest {
            revision,
            snapshot,
            intent,
            responses: response.into_iter().collect(),
        }))?;
        Ok(completion)
    }

    fn configure(&self, message: WorkerMessage) -> Result<(), String> {
        let _submission = self.submission_order.lock();
        self.mailbox.push(message)
    }
}

fn wait_for_completion(
    completion: Option<Receiver<Result<SaveOutcome, String>>>,
) -> Result<Option<SaveOutcome>, String> {
    match completion {
        Some(completion) => completion
            .recv_timeout(PERSISTENCE_DEADLINE)
            .map_err(|error| format!("desk persistence barrier was not confirmed: {error}"))?
            .map(Some),
        None => Ok(None),
    }
}

fn run_worker(
    mailbox: Arc<PersistenceMailbox>,
    context: Option<PersistenceContext>,
    mut backend: PersistenceBackend,
) {
    let mut committed_submission = 0;
    let mut last_outcome = SaveOutcome::Saved;
    loop {
        let message = mailbox.pop();
        match message {
            WorkerMessage::Persist(request) => {
                let latest = &request;
                let result = if latest.revision <= committed_submission {
                    Ok(last_outcome.clone())
                } else {
                    commit_request(
                        context.as_ref(),
                        &mut backend,
                        latest,
                    )
                };
                match &result {
                    Ok(SaveOutcome::Saved) => {
                        committed_submission = latest.revision;
                        last_outcome = SaveOutcome::Saved;
                    }
                    Ok(outcome @ SaveOutcome::RefusedReduction(_)) => {
                        last_outcome = outcome.clone();
                    }
                    Err(error) => log::warn!(
                        "[session-persist] revision {} was not committed: {}",
                        latest.revision,
                        error
                    ),
                }
                for response in request.responses {
                    let _ = response.send(result.clone());
                }
            }
            WorkerMessage::InstallStorage(adapter) => {
                backend = PersistenceBackend::Unavailable(
                    "storage desk backend is initializing".to_string(),
                );
                match adapter.state() {
                    Ok(state) => {
                        if let Some(context) = context.as_ref() {
                            let _ = context
                                .app
                                .emit("desk-revision-committed", state.revision.to_string());
                        }
                        backend = PersistenceBackend::Storage {
                            adapter,
                            revision: state.revision,
                        };
                    }
                    Err(error) => {
                        let reason = format!("storage desk backend unavailable: {error}");
                        log::error!("[session-persist] {reason}");
                        backend = PersistenceBackend::Unavailable(reason);
                    }
                }
            }
            WorkerMessage::Unavailable(reason) => {
                backend = PersistenceBackend::Unavailable(reason);
            }
            WorkerMessage::Stop(response) => {
                drop(backend);
                let _ = response.send(());
                return;
            }
        }
    }
}

fn commit_request(
    context: Option<&PersistenceContext>,
    backend: &mut PersistenceBackend,
    request: &PersistRequest,
) -> Result<SaveOutcome, String> {
    match backend {
        PersistenceBackend::Json => {
            let context = context
                .ok_or_else(|| "JSON persistence context is unavailable".to_string())?;
            let outcome = crate::session::save_captured_snapshot_with_live_identities(
                &context.session,
                &context.mux,
                &request.snapshot,
                request.intent,
            )?;
            crate::session::scrollback_sync::sync_scrollback_sidecars_after_save(
                &context.app,
                &outcome,
            );
            Ok(outcome)
        }
        PersistenceBackend::Storage { adapter, revision } => {
            let context = context
                .ok_or_else(|| "Storage persistence context is unavailable".to_string())?;
            commit_storage_snapshot(adapter, revision, request, &context.app)
        }
        PersistenceBackend::Unavailable(reason) => Err(reason.clone()),
        #[cfg(test)]
        PersistenceBackend::Test(writer) => writer(request.revision),
    }
}

fn commit_storage_snapshot(
    adapter: &crate::session::store::StorageDeskAdapter,
    revision: &mut codeterm_storage::Revision,
    request: &PersistRequest,
    app: &tauri::AppHandle,
) -> Result<SaveOutcome, String> {
    let identities = desk_agent_session_identities(&request.snapshot);
    let mut rebases = 0;
    loop {
        let completion = adapter.delta_snapshot(
            &request.snapshot,
            identities.clone(),
            *revision,
            request.intent,
        )?;
        match completion.wait(PERSISTENCE_DEADLINE) {
            Ok(committed) => {
                *revision = committed.receipt.revision;
                let _ = app.emit("desk-revision-committed", revision.to_string());
                crate::session::scrollback_sync::sync_scrollback_sidecars_after_save(
                    app,
                    &SaveOutcome::Saved,
                );
                return Ok(SaveOutcome::Saved);
            }
            Err(error) => {
                let latest = adapter.state()?;
                if latest.revision == *revision || rebases == 3 {
                    return Err(format!("desk completion was not confirmed: {error}"));
                }
                *revision = latest.revision;
                rebases += 1;
            }
        }
    }
}

fn desk_agent_session_identities(
    snapshot: &crate::session::SessionSnapshot,
) -> Vec<codeterm_storage::DeskAgentSessionIdentity> {
    use crate::session::store::SplitNodeSnapshot;

    fn collect(
        node: &SplitNodeSnapshot,
        identities: &mut BTreeMap<(String, String), codeterm_storage::DeskAgentSessionIdentity>,
    ) {
        match node {
            SplitNodeSnapshot::Split { children, .. } => {
                for child in children {
                    collect(child, identities);
                }
            }
            SplitNodeSnapshot::Leaf {
                agent_type: Some(provider),
                claude_session_id: Some(session_id),
                agent_identity: Some(binding),
                role: Some(role),
                role_profile,
                parent_tab_id,
                can_spawn,
                ..
            } if binding.session_binding_nonce.is_some()
                && binding.session_binding_nonce == binding.reported_session_binding_nonce =>
            {
                identities.insert(
                    (provider.clone(), session_id.clone()),
                    codeterm_storage::DeskAgentSessionIdentity {
                        provider: provider.clone(),
                        session_id: session_id.clone(),
                        record_provider: provider.clone(),
                        role: role.clone(),
                        role_profile: role_profile.clone(),
                        parent_tab_id: parent_tab_id.clone(),
                        can_spawn: *can_spawn,
                    },
                );
            }
            SplitNodeSnapshot::Leaf { .. } => {}
        }
    }

    let mut identities = BTreeMap::new();
    for workspace in &snapshot.workspaces {
        for tab in &workspace.tabs {
            collect(&tab.split_tree, &mut identities);
        }
    }
    identities.into_values().collect()
}

pub(crate) fn persist_session_after_close(svc: &super::ServiceContext) {
    if let Err(error) = submit_session_snapshot_with_intent(
        &svc.app_handle,
        &svc.mux,
        SaveIntent::AuthorizedReduction,
    ) {
        log::warn!("[session-persist] post-close save failed: {}", error);
    }
}

pub(crate) fn persist_session_after_mutation(
    svc: &super::ServiceContext,
    urgency: SessionPersistUrgency,
) -> Result<(), String> {
    let intent = match urgency {
        SessionPersistUrgency::Prompt | SessionPersistUrgency::Debounced => {
            SaveIntent::PreserveDesk
        }
    };
    submit_session_snapshot_with_intent(&svc.app_handle, &svc.mux, intent)
}

impl super::ServiceContext {
    pub(crate) fn schedule_session_persist_debounced(&self) {
        if let Err(error) = submit_session_snapshot_with_intent(
            &self.app_handle,
            &self.mux,
            SaveIntent::PreserveDesk,
        ) {
            log::warn!("[session-persist] submission failed: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock_hold::TimedRwLock;
    use crate::multiplexer::state::MultiplexerState;
    use crate::session::store::SessionStore;
    use parking_lot::Mutex;
    use std::sync::mpsc::TryRecvError;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn test_store(dir: &TempDir) -> SessionStore {
        SessionStore::for_test_dir(dir.path().to_path_buf())
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
        std::thread::spawn(move || run_worker(mailbox, None, PersistenceBackend::Test(Box::new(writer))));
        worker
    }

    fn stop_test_worker(worker: &SessionPersistenceWorker) {
        let (response, completion) = mpsc::channel();
        worker
            .configure(WorkerMessage::Stop(response))
            .unwrap();
        completion.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    fn test_snapshot(revision: u64) -> crate::session::SessionSnapshot {
        let mut snapshot = desk_delta_snapshot(0);
        snapshot.timestamp = revision;
        snapshot
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
            barrier_worker.submit_snapshot(
                test_snapshot(1),
                SaveIntent::PreserveDesk,
                true,
            )
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
    fn rename_workspace_persisted_promptly_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Mutex::new(test_store(&dir)));
        let mux: MuxState = Arc::new(TimedRwLock::new("mux", MultiplexerState::empty()));
        let ws = mux.write().create_workspace("before".to_string(), None);

        mux.write()
            .rename_workspace(&ws.id, "after-rename")
            .unwrap();
        let snapshot = mux.read().to_session_snapshot();
        store.lock().save(&snapshot).unwrap();

        let loaded = store.lock().load_last().unwrap().expect("snapshot on disk");
        let ws_snap = loaded
            .workspaces
            .iter()
            .find(|w| w.id == ws.id)
            .expect("workspace in snapshot");
        assert_eq!(ws_snap.name, "after-rename");
    }

    #[test]
    fn rename_tab_persisted_promptly_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Mutex::new(test_store(&dir)));
        let mux: MuxState = Arc::new(TimedRwLock::new("mux", MultiplexerState::empty()));
        let ws = mux.write().create_workspace("ws".to_string(), None);
        let tab_id = mux.write().create_tab(&ws.id).unwrap();

        mux.write().rename_tab(&tab_id, "renamed-tab").unwrap();
        let snapshot = mux.read().to_session_snapshot();
        store.lock().save(&snapshot).unwrap();

        let loaded = store.lock().load_last().unwrap().expect("snapshot on disk");
        let ws_snap = loaded.workspaces.iter().find(|w| w.id == ws.id).unwrap();
        let tab = ws_snap.tabs.iter().find(|t| t.id == tab_id).unwrap();
        assert_eq!(tab.title, "renamed-tab");
    }

    #[test]
    fn storage_desk_app_adapter_imports_and_flushes_exact_selection() {
        let root = tempfile::tempdir().unwrap();
        let adapter = crate::session::store::StorageDeskAdapter::open(root.path()).unwrap();
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
        let adapter = crate::session::store::StorageDeskAdapter::open(root.path()).unwrap();
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
    fn desk_delta_adjacent_reorder_touches_constant_rows_in_a_large_inventory() {
        let root = tempfile::tempdir().unwrap();
        let adapter = crate::session::store::StorageDeskAdapter::open(root.path()).unwrap();
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

        assert_eq!(committed.receipt.rows_written, 7);
        let saved = adapter.state().unwrap();
        assert_eq!(saved.snapshot.workspaces[255].record.id.as_str(), "ws-256");
        assert_eq!(saved.snapshot.workspaces[256].record.id.as_str(), "ws-255");
    }

    #[test]
    fn desk_delta_preserves_omissions_and_rejects_stale_reduction() {
        let root = tempfile::tempdir().unwrap();
        let adapter = crate::session::store::StorageDeskAdapter::open(root.path()).unwrap();
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
        let adapter = crate::session::store::StorageDeskAdapter::open(root.path()).unwrap();
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
}
