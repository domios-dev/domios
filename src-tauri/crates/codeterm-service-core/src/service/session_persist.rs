use std::collections::{BTreeMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use crate::multiplexer::commands::MuxState;
use crate::session::store::{SaveIntent, SaveOutcome};

pub const PERSISTENCE_DEADLINE: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionPersistUrgency {
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
    #[cfg(any(test, feature = "test-support"))]
    Flush(Sender<Result<(), String>>),
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
                #[cfg(any(test, feature = "test-support"))]
                WorkerMessage::Flush(response) => {
                    let _ = response.send(Err(error.to_string()));
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

    fn replay_before_pending(&self, mut messages: VecDeque<WorkerMessage>) {
        let mut state = self.state.lock();
        while let Some(message) = messages.pop_back() {
            state.pending.push_front(message);
        }
        self.ready.notify_all();
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
    ui: Arc<dyn crate::ports::host::UiEvents>,
    mux: MuxState,
    output_buf: crate::OutputBufferState,
    service: std::sync::Weak<super::ServiceContext>,
}

enum PersistenceBackend {
    Pending,
    Storage {
        adapter: crate::session::store::StorageDeskAdapter,
        revision: codeterm_storage::Revision,
    },
    Unavailable(String),
    #[cfg(any(test, feature = "test-support"))]
    Test(Box<dyn FnMut(u64) -> Result<SaveOutcome, String> + Send>),
}

/// `Ok(RefusedReduction)` is the guard succeeding (fuller desk kept on disk,
/// already logged at error by the store); `Err` is a genuine persist failure.
pub fn write_session_snapshot(
    svc: &super::ServiceContext,
    mux: &MuxState,
) -> Result<SaveOutcome, String> {
    write_session_snapshot_with_intent(svc, mux, SaveIntent::PreserveDesk)
}

pub fn write_session_snapshot_with_intent(
    svc: &super::ServiceContext,
    mux: &MuxState,
    intent: SaveIntent,
) -> Result<SaveOutcome, String> {
    let worker = ensure_worker(svc)?;
    worker
        .submit_current(mux, intent, true)?
        .ok_or_else(|| "desk persistence barrier returned no outcome".to_string())
}

pub fn submit_session_snapshot_with_intent(
    svc: &super::ServiceContext,
    mux: &MuxState,
    intent: SaveIntent,
) -> Result<(), String> {
    let worker = ensure_worker(svc)?;
    worker.submit_current(mux, intent, false).map(|_| ())
}

pub fn write_captured_snapshot_with_live_identities(
    svc: &super::ServiceContext,
    mux: &MuxState,
    captured: &crate::session::SessionSnapshot,
    intent: SaveIntent,
) -> Result<SaveOutcome, String> {
    let live = mux.read().to_session_snapshot();
    let mut snapshot = captured.clone();
    merge_live_agent_identities(&mut snapshot, &live);
    let worker = ensure_worker(svc)?;
    worker
        .submit_snapshot(snapshot, intent, true)?
        .ok_or_else(|| "desk persistence barrier returned no outcome".to_string())
}

pub fn install_storage_desk_adapter(
    svc: &super::ServiceContext,
    adapter: crate::session::store::StorageDeskAdapter,
) -> Result<(), String> {
    let worker = ensure_worker(svc)?;
    worker.configure(WorkerMessage::InstallStorage(adapter))
}

pub fn set_storage_unavailable(svc: &super::ServiceContext, reason: String) -> Result<(), String> {
    let worker = ensure_worker(svc)?;
    worker.configure(WorkerMessage::Unavailable(reason))
}

pub fn start_worker(svc: &super::ServiceContext) -> Result<(), String> {
    ensure_worker(svc).map(|_| ())
}

pub fn stop_worker(svc: &super::ServiceContext) -> Result<(), String> {
    let worker = ensure_worker(svc)?;
    let (response, completion) = mpsc::channel();
    worker.configure(WorkerMessage::Stop(response))?;
    completion
        .recv_timeout(PERSISTENCE_DEADLINE)
        .map_err(|error| format!("desk persistence worker shutdown was not confirmed: {error}"))
}

#[cfg(any(test, feature = "test-support"))]
pub fn flush_session_persistence(svc: &super::ServiceContext) -> Result<(), String> {
    ensure_worker(svc)?.flush()
}

fn ensure_worker(svc: &super::ServiceContext) -> Result<SessionPersistenceWorker, String> {
    if let Some(worker) = svc.try_ext::<SessionPersistenceWorker>() {
        return Ok(worker.clone());
    }
    let context = PersistenceContext {
        ui: svc.ui.clone(),
        mux: svc.mux.clone(),
        output_buf: svc.output_buf.clone(),
        service: svc.weak_self(),
    };
    let mailbox = Arc::new(PersistenceMailbox::default());
    let worker = SessionPersistenceWorker {
        mailbox: mailbox.clone(),
        next_revision: Arc::new(AtomicU64::new(0)),
        submission_order: Arc::new(parking_lot::Mutex::new(())),
    };
    if svc.set_ext(worker.clone()).is_ok() {
        let thread_mailbox = mailbox.clone();
        match std::thread::Builder::new()
            .name("desk-persistence".into())
            .spawn(move || run_worker(thread_mailbox, Some(context), PersistenceBackend::Pending))
        {
            Ok(_) => Ok(worker),
            Err(error) => {
                let reason = format!("desk persistence worker failed to start: {error}");
                mailbox.abort(&reason);
                Err(reason)
            }
        }
    } else {
        svc.try_ext::<SessionPersistenceWorker>()
            .cloned()
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

    #[cfg(any(test, feature = "test-support"))]
    fn flush(&self) -> Result<(), String> {
        let (response, completion) = mpsc::channel();
        self.configure(WorkerMessage::Flush(response))?;
        completion
            .recv_timeout(PERSISTENCE_DEADLINE)
            .map_err(|error| format!("desk persistence flush was not confirmed: {error}"))?
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

// `last_error` is read only by the test-only Flush barrier.
#[cfg_attr(not(test), allow(unused_variables, unused_assignments))]
fn run_worker(
    mailbox: Arc<PersistenceMailbox>,
    context: Option<PersistenceContext>,
    mut backend: PersistenceBackend,
) {
    let mut committed_submission = 0;
    let mut last_outcome = SaveOutcome::Saved;
    let mut last_error = None;
    let mut retry_after_error = false;
    let mut reported_errors = HashSet::new();
    let mut reported_orphan_teams = HashSet::new();
    let mut committed_desk = None;
    let mut deferred = VecDeque::new();
    loop {
        let message = mailbox.pop();
        match message {
            WorkerMessage::Persist(request) => {
                if matches!(&backend, PersistenceBackend::Pending) {
                    defer_persist(&mut deferred, request);
                    continue;
                }
                for (team_id, workspace_id) in
                    crate::session::store::orphan_teams(&request.snapshot)
                {
                    if reported_orphan_teams.insert(team_id.clone()) {
                        log::warn!(
                            "[session-persist] dropped orphan Team {team_id}: workspace {workspace_id} is missing"
                        );
                    }
                }
                let latest = &request;
                let projected = project_request_desk(&backend, latest);
                let result = if latest.revision <= committed_submission {
                    Ok(last_outcome.clone())
                } else {
                    match &projected {
                        Ok(Some(desk)) => {
                            let mut desk = desk.clone();
                            desk.timestamp = 0;
                            if !retry_after_error
                                && committed_desk.as_ref() == Some(&(latest.intent, desk))
                            {
                                Ok(SaveOutcome::Saved)
                            } else {
                                commit_request(context.as_ref(), &mut backend, latest)
                            }
                        }
                        Ok(None) => commit_request(context.as_ref(), &mut backend, latest),
                        Err(error) => Err(error.clone()),
                    }
                };
                match &result {
                    Ok(SaveOutcome::Saved) => {
                        committed_submission = latest.revision;
                        last_outcome = SaveOutcome::Saved;
                        last_error = None;
                        retry_after_error = false;
                        if let Some(mut desk) =
                            projected.as_ref().ok().and_then(|desk| desk.clone())
                        {
                            desk.timestamp = 0;
                            committed_desk = Some((latest.intent, desk));
                        }
                    }
                    Ok(outcome @ SaveOutcome::RefusedReduction(_)) => {
                        last_outcome = outcome.clone();
                        last_error = None;
                        retry_after_error = false;
                    }
                    Err(error) => {
                        if reported_errors.insert(error.clone()) {
                            log::warn!(
                                "[session-persist] revision {} was not committed: {}",
                                latest.revision,
                                error
                            );
                        }
                        last_error = Some(error.clone());
                        retry_after_error = true;
                    }
                }
                for response in request.responses {
                    let _ = response.send(result.clone());
                }
            }
            WorkerMessage::InstallStorage(adapter) => match adapter.state() {
                Ok(state) => {
                    let mut desk = state.snapshot;
                    desk.timestamp = 0;
                    committed_desk = Some((SaveIntent::PreserveDesk, desk));
                    if let Some(context) = context.as_ref() {
                        context.ui.emit_native(
                            "desk-revision-committed",
                            serde_json::json!(state.revision.to_string()),
                        );
                    }
                    backend = PersistenceBackend::Storage {
                        adapter,
                        revision: state.revision,
                    };
                    last_error = None;
                    retry_after_error = false;
                    mailbox.replay_before_pending(std::mem::take(&mut deferred));
                }
                Err(error) => {
                    let reason = format!("storage desk backend unavailable: {error}");
                    log::error!("[session-persist] {reason}");
                    backend = PersistenceBackend::Unavailable(reason);
                    committed_desk = None;
                    mailbox.replay_before_pending(std::mem::take(&mut deferred));
                }
            },
            WorkerMessage::Unavailable(reason) => {
                backend = PersistenceBackend::Unavailable(reason);
                committed_desk = None;
                mailbox.replay_before_pending(std::mem::take(&mut deferred));
            }
            #[cfg(any(test, feature = "test-support"))]
            WorkerMessage::Flush(response) => {
                if matches!(&backend, PersistenceBackend::Pending) {
                    deferred.push_back(WorkerMessage::Flush(response));
                    continue;
                }
                let result = last_error.clone().map_or(Ok(()), Err);
                let _ = response.send(result);
            }
            WorkerMessage::Stop(response) => {
                let reason =
                    "desk persistence worker stopped before Storage desk backend initialized";
                for message in deferred.drain(..) {
                    match message {
                        WorkerMessage::Persist(request) => {
                            for response in request.responses {
                                let _ = response.send(Err(reason.to_string()));
                            }
                        }
                        #[cfg(any(test, feature = "test-support"))]
                        WorkerMessage::Flush(response) => {
                            let _ = response.send(Err(reason.to_string()));
                        }
                        _ => unreachable!(),
                    }
                }
                drop(backend);
                let _ = response.send(());
                return;
            }
        }
    }
}

fn defer_persist(deferred: &mut VecDeque<WorkerMessage>, mut request: PersistRequest) {
    let coalesce = matches!(
        deferred.back(),
        Some(WorkerMessage::Persist(pending)) if pending.intent == request.intent
    );
    if coalesce {
        let Some(WorkerMessage::Persist(pending)) = deferred.back_mut() else {
            unreachable!();
        };
        pending.revision = request.revision;
        pending.snapshot = request.snapshot;
        pending.responses.append(&mut request.responses);
    } else {
        deferred.push_back(WorkerMessage::Persist(request));
    }
}

fn project_request_desk(
    backend: &PersistenceBackend,
    request: &PersistRequest,
) -> Result<Option<codeterm_storage::DeskSnapshot>, String> {
    let PersistenceBackend::Storage { .. } = backend else {
        return Ok(None);
    };
    crate::session::store::project_snapshot(
        &request.snapshot,
        desk_agent_session_identities(&request.snapshot),
    )
    .map(Some)
}

fn commit_request(
    context: Option<&PersistenceContext>,
    backend: &mut PersistenceBackend,
    request: &PersistRequest,
) -> Result<SaveOutcome, String> {
    match backend {
        PersistenceBackend::Storage { adapter, revision } => {
            commit_storage_snapshot(adapter, revision, request, context)
        }
        PersistenceBackend::Pending => Err("Storage desk backend is not initialized".to_string()),
        PersistenceBackend::Unavailable(reason) => Err(reason.clone()),
        #[cfg(any(test, feature = "test-support"))]
        PersistenceBackend::Test(writer) => writer(request.revision),
    }
}

fn merge_live_agent_identities(
    snapshot: &mut crate::session::SessionSnapshot,
    live: &crate::session::SessionSnapshot,
) {
    use crate::session::store::{AgentIdentitySnapshot, SplitNodeSnapshot};
    use std::collections::HashMap;

    fn collect(node: &SplitNodeSnapshot, identities: &mut HashMap<String, AgentIdentitySnapshot>) {
        match node {
            SplitNodeSnapshot::Leaf {
                tab_id,
                agent_identity: Some(identity),
                ..
            } => {
                identities.insert(tab_id.clone(), identity.clone());
            }
            SplitNodeSnapshot::Leaf { .. } => {}
            SplitNodeSnapshot::Split { children, .. } => {
                for child in children {
                    collect(child, identities);
                }
            }
        }
    }

    fn apply(node: &mut SplitNodeSnapshot, identities: &HashMap<String, AgentIdentitySnapshot>) {
        match node {
            SplitNodeSnapshot::Leaf {
                tab_id,
                agent_identity,
                ..
            } => {
                if let Some(identity) = identities.get(tab_id) {
                    *agent_identity = Some(identity.clone());
                }
            }
            SplitNodeSnapshot::Split { children, .. } => {
                for child in children {
                    apply(child, identities);
                }
            }
        }
    }

    let mut identities = HashMap::new();
    for workspace in &live.workspaces {
        for tab in &workspace.tabs {
            collect(&tab.split_tree, &mut identities);
        }
    }
    for workspace in &mut snapshot.workspaces {
        for tab in &mut workspace.tabs {
            apply(&mut tab.split_tree, &identities);
        }
    }
}

fn commit_storage_snapshot(
    adapter: &crate::session::store::StorageDeskAdapter,
    revision: &mut codeterm_storage::Revision,
    request: &PersistRequest,
    context: Option<&PersistenceContext>,
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
                if let Some(context) = context {
                    context.ui.emit_native(
                        "desk-revision-committed",
                        serde_json::json!(revision.to_string()),
                    );
                    if let Some(session) = context
                        .service
                        .upgrade()
                        .and_then(|svc| svc.try_ext::<crate::session::SessionState>().cloned())
                    {
                        crate::session::scrollback_sync::sync_scrollback_sidecars_after_save_with(
                            &SaveOutcome::Saved,
                            &context.mux,
                            &context.output_buf,
                            &session,
                        );
                    }
                }
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

pub fn persist_session_after_close(svc: &super::ServiceContext) {
    if let Err(error) =
        submit_session_snapshot_with_intent(svc, &svc.mux, SaveIntent::AuthorizedReduction)
    {
        log::warn!("[session-persist] post-close save failed: {}", error);
    }
}

pub fn persist_session_after_mutation(
    svc: &super::ServiceContext,
    urgency: SessionPersistUrgency,
) -> Result<(), String> {
    let intent = match urgency {
        SessionPersistUrgency::Prompt | SessionPersistUrgency::Debounced => {
            SaveIntent::PreserveDesk
        }
    };
    submit_session_snapshot_with_intent(svc, &svc.mux, intent)
}

impl super::ServiceContext {
    pub(crate) fn schedule_session_persist_debounced(&self) {
        if let Err(error) =
            submit_session_snapshot_with_intent(self, &self.mux, SaveIntent::PreserveDesk)
        {
            log::warn!("[session-persist] submission failed: {error}");
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;

#[cfg(any(test, feature = "test-support"))]
pub(crate) mod persistence_test_support;
