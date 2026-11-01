use super::enrollment::{enroll, verify_client_process, verify_component, EnrollmentPolicy};
use super::peer::pin_peer;
use super::wire_io::{encode_frame, read_frame, send_frame, write_encoded_frame};
use crate::bootstrap::process::PinnedProcess;
use codeterm_types::{
    native_host::{
        NativeHostCancellation, NativeHostCompletion, NativeHostConnection, NativeHostEvent,
        NativeHostEventAudience, NativeHostFrame, NativeHostOperation, NativeHostRejection,
        NativeHostReply, NativeHostRequest, NativeHostRequestId, NativeHostResult, NativeHostScope,
        NativeHostUncertainty, NativeViewBinding, NativeWebviewDiagnosticsReport,
        NativeWebviewDiagnosticsSnapshot, MAX_PENDING_NATIVE_HOST_REQUESTS,
        NATIVE_HOST_REQUEST_DEADLINE,
    },
    native_update::{
        NativeUpdateExecutor, NativeUpdateRefusal, NativeUpdateRequest, NativeUpdateResult,
    },
    server_bootstrap::{ProcessProvenance, ServerComponentManifest, ServerReady},
};
use interprocess::local_socket::{
    tokio::{prelude::*, Listener, Stream},
    GenericNamespaced, ListenerOptions,
};
use parking_lot::{Mutex, RwLock};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    collections::{HashMap, HashSet},
    io,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};
use tokio::{
    io::AsyncWriteExt,
    sync::{mpsc, oneshot, watch, Semaphore},
    time::timeout,
};

type Frame<UpdateRequest, UpdateResult> = NativeHostFrame<UpdateRequest, UpdateResult>;
type Completion<UpdateResult> = NativeHostCompletion<NativeHostResult<UpdateResult>>;

pub struct NativeHostServer<UpdateRequest, UpdateResult> {
    listener: Listener,
    endpoint: NativeHostEndpoint<UpdateRequest, UpdateResult>,
    ready: ServerReady,
    manifest: ServerComponentManifest,
    allowed_view_origins: HashSet<String>,
}

#[derive(Debug)]
pub enum NativeHostBindError {
    InventoryRejected(io::Error),
    ListenerUnavailable(io::Error),
}

pub struct NativeHostEndpoint<UpdateRequest, UpdateResult> {
    pub(super) active: Arc<RwLock<Option<Arc<Session<UpdateRequest, UpdateResult>>>>>,
    pub(super) active_generation: Arc<AtomicU64>,
    pub(super) attached_once: Arc<AtomicBool>,
    pub(super) next_request_id: Arc<AtomicU64>,
    pub(super) next_event_id: Arc<AtomicU64>,
    pub(super) ready: ServerReady,
    pub(super) manifest: ServerComponentManifest,
}

impl<UpdateRequest, UpdateResult> Clone for NativeHostEndpoint<UpdateRequest, UpdateResult> {
    fn clone(&self) -> Self {
        Self {
            active: self.active.clone(),
            active_generation: self.active_generation.clone(),
            attached_once: self.attached_once.clone(),
            next_request_id: self.next_request_id.clone(),
            next_event_id: self.next_event_id.clone(),
            ready: self.ready.clone(),
            manifest: self.manifest.clone(),
        }
    }
}

struct AcceptState<UpdateRequest, UpdateResult> {
    endpoint: NativeHostEndpoint<UpdateRequest, UpdateResult>,
    ready: ServerReady,
    manifest: ServerComponentManifest,
    allowed_view_origins: HashSet<String>,
    next_generation: AtomicU64,
    attaching: Mutex<Option<NativeHostConnection>>,
    pending_attaches: Arc<Semaphore>,
}

pub(super) struct Session<UpdateRequest, UpdateResult> {
    pub(super) connection: NativeHostConnection,
    pub(super) client_process: ProcessProvenance,
    pub(super) process: PinnedProcess,
    pub(super) active_generation: Arc<AtomicU64>,
    pub(super) allowed_view_origins: HashSet<String>,
    pub(super) control: mpsc::Sender<Frame<UpdateRequest, UpdateResult>>,
    pub(super) events: mpsc::Sender<NativeHostEvent>,
    pub(super) state: Mutex<SessionState<UpdateResult>>,
    pub(super) shutdown: watch::Sender<bool>,
    pub(super) retired: AtomicBool,
}

pub(super) struct SessionState<UpdateResult> {
    pub(super) view: Option<NativeViewBinding>,
    pub(super) diagnostics: Option<NativeWebviewDiagnosticsReport>,
    pub(super) pending: HashMap<NativeHostRequestId, Pending<UpdateResult>>,
}

pub(super) struct Pending<UpdateResult> {
    pub(super) scope: NativeHostScope,
    pub(super) reply: Option<oneshot::Sender<NativeHostReply<UpdateResult>>>,
    pub(super) dispatched: bool,
}

struct PendingCallGuard<UpdateRequest, UpdateResult> {
    session: Arc<Session<UpdateRequest, UpdateResult>>,
    request_id: NativeHostRequestId,
    armed: bool,
}

impl<UpdateRequest, UpdateResult> NativeHostServer<UpdateRequest, UpdateResult>
where
    UpdateRequest: Serialize + DeserializeOwned + Send + 'static,
    UpdateResult: Serialize + DeserializeOwned + Send + 'static,
{
    pub fn bind(
        ready: ServerReady,
        manifest: ServerComponentManifest,
        allowed_view_origins: HashSet<String>,
    ) -> Result<Self, NativeHostBindError> {
        verify_component(&manifest, &ready, &ready.resources)
            .map_err(NativeHostBindError::InventoryRejected)?;
        if matches!(
            manifest.client_identity,
            codeterm_types::server_bootstrap::ClientIdentity::None
        ) {
            return Err(NativeHostBindError::InventoryRejected(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Headless components do not admit native attachment",
            )));
        }
        let listener = create_listener(&ready).map_err(NativeHostBindError::ListenerUnavailable)?;
        Ok(Self {
            listener,
            endpoint: NativeHostEndpoint {
                active: Arc::new(RwLock::new(None)),
                active_generation: Arc::new(AtomicU64::new(0)),
                attached_once: Arc::new(AtomicBool::new(false)),
                next_request_id: Arc::new(AtomicU64::new(1)),
                next_event_id: Arc::new(AtomicU64::new(1)),
                ready: ready.clone(),
                manifest: manifest.clone(),
            },
            ready,
            manifest,
            allowed_view_origins,
        })
    }

    pub fn endpoint(&self) -> NativeHostEndpoint<UpdateRequest, UpdateResult> {
        self.endpoint.clone()
    }

    pub(crate) fn attached_once(&self) -> Arc<AtomicBool> {
        self.endpoint.attached_once.clone()
    }

    pub async fn serve(self) -> io::Result<()>
    where
        UpdateRequest: Sync,
        UpdateResult: Sync,
    {
        let NativeHostServer {
            listener,
            endpoint,
            ready,
            manifest,
            allowed_view_origins,
        } = self;
        let state = Arc::new(AcceptState {
            endpoint,
            ready,
            manifest,
            allowed_view_origins,
            next_generation: AtomicU64::new(1),
            attaching: Mutex::new(None),
            pending_attaches: Arc::new(Semaphore::new(8)),
        });
        loop {
            let stream = listener.accept().await?;
            let Ok(permit) = state.pending_attaches.clone().try_acquire_owned() else {
                drop(stream);
                continue;
            };
            let state = state.clone();
            tokio::spawn(async move {
                let _permit = permit;
                if let Err(error) = state.attach(stream).await {
                    log::debug!("[native-host] attachment refused: {error}");
                }
            });
        }
    }
}

fn create_listener(ready: &ServerReady) -> io::Result<Listener> {
    let address = codeterm_types::native_host::native_host_socket_address(&ready.bootstrap_address)
        .ok_or_else(|| io::Error::other("Invalid native host bootstrap address"))?;
    codeterm_util::local_endpoint::prepare_namespaced(&address)?;
    let target = codeterm_util::local_endpoint::display_name(&address);
    let name = address
        .to_ns_name::<GenericNamespaced>()
        .map_err(|error| codeterm_util::local_endpoint::bind_error(&target, error))?;
    let options = ListenerOptions::new().name(name);
    // fchmod on a socket is EINVAL on macOS (interprocess: Unsupported); only Linux pins the mode.
    #[cfg(target_os = "linux")]
    let options = {
        use interprocess::os::unix::local_socket::ListenerOptionsExt;
        options.mode(0o600)
    };
    #[cfg(windows)]
    let options = {
        use interprocess::os::windows::{
            local_socket::ListenerOptionsExt, security_descriptor::SecurityDescriptor,
        };
        let sddl = widestring::U16CString::from_str("D:P(A;;GA;;;OW)").map_err(io::Error::other)?;
        options.security_descriptor(SecurityDescriptor::deserialize(&sddl)?)
    };
    options
        .create_tokio()
        .map_err(|error| codeterm_util::local_endpoint::bind_error(&target, error))
}

impl NativeHostEndpoint<NativeUpdateRequest, NativeUpdateResult> {
    pub(crate) async fn request_update_on_connection(
        &self,
        connection: &codeterm_types::native_host::NativeHostConnection,
        operation: NativeHostOperation<NativeUpdateRequest>,
    ) -> Completion<NativeUpdateResult> {
        let session = self
            .active
            .read()
            .as_ref()
            .filter(|session| &session.connection == connection)
            .cloned();
        let Some(session) = session else {
            return NativeHostCompletion::Rejected(NativeHostRejection::NotAttached);
        };
        self.request_captured(&session, NativeHostScope::Client, operation)
            .await
    }

    pub(crate) fn update_request(&self) -> NativeUpdateRequest {
        NativeUpdateRequest {
            operation_id: codeterm_types::native_update::NativeUpdateOperationId::new(
                uuid::Uuid::new_v4(),
            )
            .expect("new UUID is nonzero"),
            action: codeterm_types::native_update::NativeUpdateAction::Pair,
            expected_server: codeterm_types::native_update::ExpectedServer {
                components: self.manifest.clone(),
                identity: self.ready.identity.clone(),
                incarnation: codeterm_types::server_bootstrap::ServerExpectation {
                    epoch: self.ready.epoch.clone(),
                    process: self.ready.process.clone(),
                },
            },
            staged_components: self.manifest.clone(),
            restart_authorized: true,
        }
    }

    pub fn active_native_update_executor(
        &self,
    ) -> Result<Arc<dyn NativeUpdateExecutor>, NativeUpdateRefusal> {
        super::enrollment::active_native_update_executor(self)
    }
}

impl<UpdateRequest, UpdateResult> NativeHostEndpoint<UpdateRequest, UpdateResult>
where
    UpdateRequest: Serialize + DeserializeOwned + Send + 'static,
    UpdateResult: Serialize + DeserializeOwned + Send + 'static,
{
    pub fn quit_client(
        &self,
    ) -> Result<Option<codeterm_types::app_quit::QuitClient>, codeterm_types::app_quit::QuitRefusal>
    {
        let Some(session) = self.active.read().clone() else {
            return Ok(None);
        };
        if session.retired.load(Ordering::Acquire)
            || self.active_generation.load(Ordering::Acquire) != session.connection.generation
            || session.process.provenance().ok().as_ref() != Some(&session.client_process)
        {
            return Err(codeterm_types::app_quit::QuitRefusal::Unavailable);
        }
        Ok(Some(codeterm_types::app_quit::QuitClient {
            connection: session.connection.clone(),
            process: session.client_process.clone(),
        }))
    }

    pub fn connection(&self) -> Option<NativeHostConnection> {
        self.active
            .read()
            .as_ref()
            .filter(|session| !session.retired.load(Ordering::Acquire))
            .map(|session| session.connection.clone())
    }

    pub fn cdp_port(&self) -> Option<u16> {
        match self.webview_diagnostics_snapshot()?.status {
            codeterm_types::native_host::NativeWebviewDiagnosticsStatus::Active {
                port, ..
            } => Some(port),
            _ => None,
        }
    }

    pub fn frontend_port(&self) -> Option<u16> {
        let session = self.active.read().as_ref().cloned()?;
        let state = session.state.lock();
        if session.retired.load(Ordering::Acquire)
            || self.active_generation.load(Ordering::Acquire) != session.connection.generation
        {
            return None;
        }
        let origin = url::Url::parse(&state.view.as_ref()?.origin).ok()?;
        (origin.scheme() == "http"
            && matches!(origin.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")))
        .then(|| origin.port())
        .flatten()
    }

    pub fn webview_diagnostics_snapshot(&self) -> Option<NativeWebviewDiagnosticsSnapshot> {
        let session = self
            .active
            .read()
            .as_ref()
            .filter(|session| !session.retired.load(Ordering::Acquire))
            .cloned()?;
        if self.active_generation.load(Ordering::Acquire) != session.connection.generation {
            return None;
        }
        let report = {
            let state = session.state.lock();
            if session.retired.load(Ordering::Acquire)
                || self.active_generation.load(Ordering::Acquire) != session.connection.generation
            {
                return None;
            }
            let report = state.diagnostics.as_ref()?;
            if state
                .view
                .as_ref()
                .is_none_or(|view| view.session_id != report.view_session)
            {
                return None;
            }
            report.clone()
        };
        Some(NativeWebviewDiagnosticsSnapshot {
            connection: session.connection.clone(),
            view_session: report.view_session,
            client_process: session.client_process.clone(),
            status: report.status,
        })
    }

    pub async fn request(
        &self,
        scope: NativeHostScope,
        operation: NativeHostOperation<UpdateRequest>,
    ) -> Completion<UpdateResult> {
        let Some(session) = self
            .active
            .read()
            .as_ref()
            .filter(|session| !session.retired.load(Ordering::Acquire))
            .cloned()
        else {
            return NativeHostCompletion::Rejected(NativeHostRejection::NotAttached);
        };
        let Some(request_id) = next_id(&self.next_request_id) else {
            return NativeHostCompletion::Rejected(NativeHostRejection::RequestIdsExhausted);
        };
        session
            .request(NativeHostRequestId(request_id), scope, operation)
            .await
    }

    pub(super) async fn request_captured(
        &self,
        session: &Arc<Session<UpdateRequest, UpdateResult>>,
        scope: NativeHostScope,
        operation: NativeHostOperation<UpdateRequest>,
    ) -> NativeHostCompletion<NativeHostResult<UpdateResult>> {
        if session.retired.load(Ordering::Acquire)
            || self.active_generation.load(Ordering::Acquire) != session.connection.generation
            || !self
                .active
                .read()
                .as_ref()
                .is_some_and(|active| Arc::ptr_eq(active, session))
        {
            return NativeHostCompletion::Rejected(NativeHostRejection::NotAttached);
        }
        let Some(request_id) = next_id(&self.next_request_id) else {
            return NativeHostCompletion::Rejected(NativeHostRejection::RequestIdsExhausted);
        };
        session
            .request(NativeHostRequestId(request_id), scope, operation)
            .await
    }

    pub fn emit(&self, name: &str, payload: serde_json::Value) {
        self.emit_audience(NativeHostEventAudience::Generic, name, payload);
    }

    pub fn emit_native(&self, name: &str, payload: serde_json::Value) {
        self.emit_audience(NativeHostEventAudience::Native, name, payload);
    }

    pub fn emit_to(&self, target: &str, name: &str, payload: serde_json::Value) {
        self.emit_audience(
            NativeHostEventAudience::Targeted(target.to_owned()),
            name,
            payload,
        );
    }

    fn emit_audience(
        &self,
        audience: NativeHostEventAudience,
        name: &str,
        payload: serde_json::Value,
    ) {
        let Some(session) = self
            .active
            .read()
            .as_ref()
            .filter(|session| !session.retired.load(Ordering::Acquire))
            .cloned()
        else {
            return;
        };
        let Some(event_id) = next_id(&self.next_event_id) else {
            return;
        };
        let event = NativeHostEvent {
            event_id,
            connection: session.connection.clone(),
            audience,
            name: name.to_owned(),
            payload,
        };
        if session.events.try_send(event).is_err() {
            log::debug!("[native-host] event queue full; native event dropped");
        }
    }

    fn detach_if_current(&self, session: &Arc<Session<UpdateRequest, UpdateResult>>) {
        let previous = {
            let mut active = self.active.write();
            if active
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, session))
            {
                self.active_generation.store(0, Ordering::Release);
                active.take()
            } else {
                None
            }
        };
        if let Some(previous) = previous {
            previous.retire(NativeHostUncertainty::DisconnectedAfterDispatch);
        }
    }
}

impl<UpdateRequest, UpdateResult> AcceptState<UpdateRequest, UpdateResult>
where
    UpdateRequest: Serialize + DeserializeOwned + Send + Sync + 'static,
    UpdateResult: Serialize + DeserializeOwned + Send + Sync + 'static,
{
    async fn attach(&self, mut stream: Stream) -> io::Result<()> {
        let process = pin_peer(&stream)?;
        let generation = next_id(&self.next_generation)
            .ok_or_else(|| io::Error::other("Native connection generations exhausted"))?;
        let policy = EnrollmentPolicy {
            challenge: codeterm_types::native_host::NativeHostChallenge::new(
                NativeHostConnection {
                    instance_id: self.ready.instance_id.clone(),
                    identity: self.ready.identity.clone(),
                    server_epoch: self.ready.epoch.clone(),
                    generation,
                },
            ),
            client_identity: self.manifest.client_identity.clone(),
            resources: self.ready.resources.clone(),
            allowed_view_origins: self.allowed_view_origins.clone(),
        };
        let verification_policy = EnrollmentPolicy {
            challenge: policy.challenge.clone(),
            client_identity: policy.client_identity.clone(),
            resources: policy.resources.clone(),
            allowed_view_origins: policy.allowed_view_origins.clone(),
        };
        let (process, verified) = tokio::task::spawn_blocking(move || {
            let verified = verify_client_process(&process, &verification_policy)?;
            Ok::<_, io::Error>((process, verified))
        })
        .await
        .map_err(io::Error::other)??;
        send_frame(
            &mut stream,
            &Frame::<UpdateRequest, UpdateResult>::Challenge(policy.challenge.clone()),
        )
        .await?;
        let request: Frame<UpdateRequest, UpdateResult> =
            timeout(NATIVE_HOST_REQUEST_DEADLINE, read_frame(&mut stream))
                .await
                .map_err(|_| {
                    io::Error::new(io::ErrorKind::TimedOut, "Native enrollment deadline")
                })??;
        let Frame::Enroll(request) = request else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Expected native enrollment request",
            ));
        };
        let enrollment = enroll(&policy, request, verified).map_err(|_| {
            io::Error::new(io::ErrorKind::PermissionDenied, "Native enrollment refused")
        })?;
        let view = enrollment.view.clone();
        let connection = enrollment.connection.clone();
        self.reserve_attach(&connection, &enrollment.client_process)?;
        let (control_tx, control_rx) = mpsc::channel(MAX_PENDING_NATIVE_HOST_REQUESTS);
        let (event_tx, event_rx) = mpsc::channel(MAX_PENDING_NATIVE_HOST_REQUESTS);
        let (shutdown, _) = watch::channel(false);
        let session = Arc::new(Session {
            connection: connection.clone(),
            client_process: enrollment.client_process.clone(),
            process,
            active_generation: self.endpoint.active_generation.clone(),
            allowed_view_origins: self.allowed_view_origins.clone(),
            control: control_tx,
            events: event_tx,
            state: Mutex::new(SessionState {
                view,
                diagnostics: None,
                pending: HashMap::new(),
            }),
            shutdown,
            retired: AtomicBool::new(false),
        });
        if let Err(error) = send_frame(
            &mut stream,
            &Frame::<UpdateRequest, UpdateResult>::Enrolled(enrollment),
        )
        .await
        {
            self.release_attach(&connection);
            session.retire(NativeHostUncertainty::DisconnectedAfterDispatch);
            return Err(error);
        }
        let previous = match self.activate_attach(&connection, session.clone()) {
            Ok(previous) => previous,
            Err(error) => {
                self.release_attach(&connection);
                session.retire(NativeHostUncertainty::DisconnectedAfterDispatch);
                return Err(error);
            }
        };
        if let Some(previous) = previous {
            previous.retire(NativeHostUncertainty::DisconnectedAfterDispatch);
        }
        let endpoint = self.endpoint.clone();
        let session_for_detach = session.clone();
        tokio::spawn(async move {
            session.run(stream, control_rx, event_rx).await;
            endpoint.detach_if_current(&session_for_detach);
        });
        Ok(())
    }

    fn reserve_attach(
        &self,
        connection: &NativeHostConnection,
        candidate: &ProcessProvenance,
    ) -> io::Result<()> {
        let mut attaching = self.attaching.lock();
        if attaching.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "A native host connection is already enrolling",
            ));
        }
        let active = self.endpoint.active.read().clone();
        if let Some(active) = active {
            if !active.retired.load(Ordering::Acquire)
                && active.process.ensure_alive().is_ok()
                && active.client_process != *candidate
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Another native client process owns this server instance",
                ));
            }
        }
        *attaching = Some(connection.clone());
        Ok(())
    }

    fn release_attach(&self, connection: &NativeHostConnection) {
        let mut attaching = self.attaching.lock();
        if attaching.as_ref() == Some(connection) {
            *attaching = None;
        }
    }

    fn activate_attach(
        &self,
        connection: &NativeHostConnection,
        session: Arc<Session<UpdateRequest, UpdateResult>>,
    ) -> io::Result<Option<Arc<Session<UpdateRequest, UpdateResult>>>> {
        let mut attaching = self.attaching.lock();
        if attaching.as_ref() != Some(connection) {
            return Err(io::Error::other("Native enrollment reservation expired"));
        }
        self.endpoint
            .active_generation
            .store(connection.generation, Ordering::Release);
        self.endpoint.attached_once.store(true, Ordering::Release);
        let previous = self.endpoint.active.write().replace(session);
        *attaching = None;
        Ok(previous)
    }
}

impl<UpdateRequest, UpdateResult> Session<UpdateRequest, UpdateResult>
where
    UpdateRequest: Serialize + DeserializeOwned + Send + 'static,
    UpdateResult: Serialize + DeserializeOwned + Send + 'static,
{
    pub(super) async fn request(
        self: &Arc<Self>,
        request_id: NativeHostRequestId,
        scope: NativeHostScope,
        operation: NativeHostOperation<UpdateRequest>,
    ) -> NativeHostCompletion<NativeHostResult<UpdateResult>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        {
            let mut state = self.state.lock();
            if self.retired.load(Ordering::Acquire) {
                return NativeHostCompletion::Rejected(NativeHostRejection::NotAttached);
            }
            if !scope_is_authorized(&state.view, &scope) {
                return NativeHostCompletion::Rejected(NativeHostRejection::InvalidView);
            }
            if state.pending.len() >= MAX_PENDING_NATIVE_HOST_REQUESTS {
                return NativeHostCompletion::Rejected(NativeHostRejection::QueueFull);
            }
            state.pending.insert(
                request_id,
                Pending {
                    scope: scope.clone(),
                    reply: Some(reply_tx),
                    dispatched: false,
                },
            );
        }
        let deadline = if matches!(&operation, NativeHostOperation::ObserveNativeUpdate { .. }) {
            codeterm_types::native_host::NATIVE_UPDATE_OBSERVATION_DEADLINE
        } else {
            NATIVE_HOST_REQUEST_DEADLINE
        };
        let request = NativeHostRequest {
            request_id,
            connection: self.connection.clone(),
            scope: scope.clone(),
            operation,
        };
        if self.control.try_send(Frame::Request(request)).is_err() {
            let rejection = {
                let mut state = self.state.lock();
                if self.retired.load(Ordering::Acquire) {
                    state.pending.remove(&request_id);
                    NativeHostRejection::NotAttached
                } else if state.pending.remove(&request_id).is_some() {
                    NativeHostRejection::QueueFull
                } else {
                    NativeHostRejection::InvalidView
                }
            };
            return NativeHostCompletion::Rejected(rejection);
        }
        let mut guard = PendingCallGuard {
            session: self.clone(),
            request_id,
            armed: true,
        };
        let mut reply_rx = reply_rx;
        match timeout(deadline, &mut reply_rx).await {
            Ok(Ok(reply)) => {
                guard.disarm();
                reply.completion
            }
            Ok(Err(_)) => {
                guard.disarm();
                NativeHostCompletion::Uncertain(NativeHostUncertainty::DisconnectedAfterDispatch)
            }
            Err(_) => {
                if let Ok(reply) = reply_rx.try_recv() {
                    guard.disarm();
                    return reply.completion;
                }
                let dispatched = self.cancel_request(request_id);
                guard.disarm();
                match dispatched {
                    Some(true) | None => NativeHostCompletion::Uncertain(
                        NativeHostUncertainty::DeadlineAfterDispatch,
                    ),
                    Some(false) => {
                        NativeHostCompletion::Rejected(NativeHostRejection::DeadlineBeforeDispatch)
                    }
                }
            }
        }
    }

    pub(super) fn retire(&self, uncertainty: NativeHostUncertainty) {
        if self.retired.swap(true, Ordering::AcqRel) {
            return;
        }
        let _ = self.shutdown.send(true);
        let pending = {
            let mut state = self.state.lock();
            state.view = None;
            state.diagnostics = None;
            std::mem::take(&mut state.pending)
        };
        for (request_id, pending) in pending {
            if let Some(reply) = pending.reply {
                let _ = reply.send(NativeHostReply {
                    request_id,
                    connection: self.connection.clone(),
                    scope: pending.scope,
                    completion: if pending.dispatched {
                        NativeHostCompletion::Uncertain(uncertainty.clone())
                    } else {
                        NativeHostCompletion::Rejected(NativeHostRejection::NotAttached)
                    },
                });
            }
        }
    }

    async fn run(
        self: Arc<Self>,
        stream: Stream,
        control_rx: mpsc::Receiver<Frame<UpdateRequest, UpdateResult>>,
        event_rx: mpsc::Receiver<NativeHostEvent>,
    ) {
        let (reader, writer) = tokio::io::split(stream);
        let writer_session = self.clone();
        let mut shutdown = self.shutdown.subscribe();
        let writer_task = tokio::spawn(async move {
            writer_session
                .write_loop(writer, control_rx, event_rx, &mut shutdown)
                .await;
            writer_session.retire(NativeHostUncertainty::DisconnectedAfterDispatch);
        });
        self.read_loop(reader).await;
        self.retire(NativeHostUncertainty::DisconnectedAfterDispatch);
        let _ = writer_task.await;
    }

    async fn write_loop<W>(
        &self,
        mut writer: W,
        mut control_rx: mpsc::Receiver<Frame<UpdateRequest, UpdateResult>>,
        mut event_rx: mpsc::Receiver<NativeHostEvent>,
        shutdown: &mut watch::Receiver<bool>,
    ) where
        W: tokio::io::AsyncWrite + Unpin,
    {
        loop {
            let frame = tokio::select! {
                biased;
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() { break; }
                    continue;
                }
                control = control_rx.recv() => match control {
                    Some(frame) => frame,
                    None => break,
                },
                event = event_rx.recv() => match event {
                    Some(event) => Frame::Event(event),
                    None => continue,
                },
            };
            let bytes = match frame {
                Frame::Request(request) => {
                    let request_id = request.request_id;
                    let bytes =
                        match encode_frame::<UpdateRequest, UpdateResult>(&Frame::Request(request))
                        {
                            Ok(bytes) => bytes,
                            Err(_) => {
                                self.reject_queued(request_id);
                                continue;
                            }
                        };
                    if !self.mark_dispatched(request_id) {
                        continue;
                    }
                    bytes
                }
                Frame::Event(event) => {
                    match encode_frame::<UpdateRequest, UpdateResult>(&Frame::Event(event)) {
                        Ok(bytes) => bytes,
                        Err(error) => {
                            log::debug!("[native-host] oversized event dropped: {error}");
                            continue;
                        }
                    }
                }
                frame => match encode_frame(&frame) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        log::debug!("[native-host] invalid control frame: {error}");
                        break;
                    }
                },
            };
            if !matches!(
                timeout(
                    NATIVE_HOST_REQUEST_DEADLINE,
                    write_encoded_frame(&mut writer, &bytes),
                )
                .await,
                Ok(Ok(()))
            ) {
                break;
            }
        }
        let _ = writer.shutdown().await;
    }

    async fn read_loop<R>(&self, mut reader: R)
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        let mut shutdown = self.shutdown.subscribe();
        loop {
            let frame = tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() { break; }
                    continue;
                }
                frame = read_frame::<R, UpdateRequest, UpdateResult>(&mut reader) => match frame {
                    Ok(frame) => frame,
                    Err(_) => break,
                },
            };
            match frame {
                Frame::Reply(reply) => self.accept_reply(reply),
                Frame::BindView { connection, view } if connection == self.connection => {
                    self.bind_view(view);
                }
                Frame::DiagnosticsStatus(report) if report.connection == self.connection => {
                    self.accept_diagnostics_status(report);
                }
                Frame::ReleaseView {
                    connection,
                    session_id,
                } if connection == self.connection => {
                    self.release_view(session_id);
                }
                Frame::Cancel(cancellation) if cancellation.connection == self.connection => {
                    self.cancelled_by_client(cancellation.request_id);
                }
                _ => break,
            }
        }
    }

    pub(super) fn accept_reply(&self, reply: NativeHostReply<UpdateResult>) {
        if self.retired.load(Ordering::Acquire) || reply.connection != self.connection {
            return;
        }
        let pending = {
            let mut state = self.state.lock();
            if state
                .pending
                .get(&reply.request_id)
                .is_some_and(|call| call.dispatched && call.scope == reply.scope)
            {
                state.pending.remove(&reply.request_id)
            } else {
                None
            }
        };
        if let Some(mut pending) = pending {
            if let Some(sender) = pending.reply.take() {
                let _ = sender.send(reply);
            }
        }
    }

    pub(super) fn accept_diagnostics_status(&self, report: NativeWebviewDiagnosticsReport) {
        if self.retired.load(Ordering::Acquire)
            || report.connection != self.connection
            || !report.status.is_well_formed()
            || self.active_generation.load(Ordering::Acquire) != self.connection.generation
        {
            return;
        }
        let mut state = self.state.lock();
        if self.retired.load(Ordering::Acquire)
            || !state
                .view
                .as_ref()
                .is_some_and(|view| view.session_id == report.view_session)
        {
            return;
        }
        state.diagnostics = Some(report);
    }

    pub(super) fn mark_dispatched(&self, request_id: NativeHostRequestId) -> bool {
        let completion = {
            let mut state = self.state.lock();
            if self.retired.load(Ordering::Acquire)
                || self.active_generation.load(Ordering::Acquire) != self.connection.generation
            {
                return false;
            }
            let Some(call) = state.pending.get(&request_id) else {
                return false;
            };
            if call.dispatched {
                return false;
            }
            let authorized = scope_is_authorized(&state.view, &call.scope);
            if !authorized {
                state.pending.remove(&request_id)
            } else {
                let Some(call) = state.pending.get_mut(&request_id) else {
                    return false;
                };
                call.dispatched = true;
                return true;
            }
        };
        if let Some(mut call) = completion {
            if let Some(reply) = call.reply.take() {
                let _ = reply.send(NativeHostReply {
                    request_id,
                    connection: self.connection.clone(),
                    scope: call.scope,
                    completion: NativeHostCompletion::Rejected(NativeHostRejection::InvalidView),
                });
            }
        }
        false
    }

    fn reject_queued(&self, request_id: NativeHostRequestId) {
        let pending = {
            let mut state = self.state.lock();
            if state
                .pending
                .get(&request_id)
                .is_some_and(|call| !call.dispatched)
            {
                state.pending.remove(&request_id)
            } else {
                None
            }
        };
        if let Some(mut pending) = pending {
            if let Some(reply) = pending.reply.take() {
                let _ = reply.send(NativeHostReply {
                    request_id,
                    connection: self.connection.clone(),
                    scope: pending.scope,
                    completion: NativeHostCompletion::Rejected(NativeHostRejection::HostRejected),
                });
            }
        }
    }

    pub(super) fn bind_view(&self, view: NativeViewBinding) {
        if self.retired.load(Ordering::Acquire)
            || !view.is_well_formed()
            || !self.allowed_view_origins.contains(&view.origin)
        {
            return;
        }
        let (replies, cancellations) = {
            let mut state = self.state.lock();
            if self.retired.load(Ordering::Acquire) {
                return;
            }
            let Some(previous) = state.view.replace(view.clone()) else {
                state.diagnostics = None;
                return;
            };
            if previous.session_id == view.session_id {
                if previous.origin == view.origin {
                    return;
                }
                state.view = Some(previous);
                return;
            }
            state.diagnostics = None;
            let old_session = previous.session_id;
            let (replies, cancellations) = self.revoke_view_requests(&mut state, old_session);
            (replies, cancellations)
        };
        self.complete_view_revocation(replies, cancellations);
    }

    pub(super) fn release_view(&self, session_id: uuid::Uuid) {
        let (replies, cancellations) = {
            let mut state = self.state.lock();
            if !state
                .view
                .as_ref()
                .is_some_and(|view| view.session_id == session_id)
            {
                return;
            }
            state.view = None;
            state.diagnostics = None;
            self.revoke_view_requests(&mut state, session_id)
        };
        self.complete_view_revocation(replies, cancellations);
    }

    fn revoke_view_requests(
        &self,
        state: &mut SessionState<UpdateResult>,
        session_id: uuid::Uuid,
    ) -> (
        Vec<(
            oneshot::Sender<NativeHostReply<UpdateResult>>,
            NativeHostReply<UpdateResult>,
        )>,
        Vec<NativeHostRequestId>,
    ) {
        let mut replies = Vec::new();
        let mut cancellations = Vec::new();
        let request_ids = state
            .pending
            .iter()
            .filter_map(|(request_id, call)| {
                (call.scope == NativeHostScope::View(session_id)).then_some(*request_id)
            })
            .collect::<Vec<_>>();
        for request_id in request_ids {
            if state
                .pending
                .get(&request_id)
                .is_some_and(|call| call.dispatched)
            {
                if let Some(call) = state.pending.get_mut(&request_id) {
                    if let Some(reply) = call.reply.take() {
                        let response = NativeHostReply {
                            request_id,
                            connection: self.connection.clone(),
                            scope: call.scope.clone(),
                            completion: NativeHostCompletion::Uncertain(
                                NativeHostUncertainty::CancelledAfterDispatch,
                            ),
                        };
                        replies.push((reply, response));
                    }
                }
                cancellations.push(request_id);
            } else if let Some(mut call) = state.pending.remove(&request_id) {
                if let Some(reply) = call.reply.take() {
                    let response = NativeHostReply {
                        request_id,
                        connection: self.connection.clone(),
                        scope: call.scope,
                        completion: NativeHostCompletion::Rejected(
                            NativeHostRejection::InvalidView,
                        ),
                    };
                    replies.push((reply, response));
                }
            }
        }
        (replies, cancellations)
    }

    fn complete_view_revocation(
        &self,
        replies: Vec<(
            oneshot::Sender<NativeHostReply<UpdateResult>>,
            NativeHostReply<UpdateResult>,
        )>,
        cancellations: Vec<NativeHostRequestId>,
    ) {
        for (sender, reply) in replies {
            let _ = sender.send(reply);
        }
        for request_id in cancellations {
            let cancellation = NativeHostCancellation {
                request_id,
                connection: self.connection.clone(),
            };
            let _ = self.control.try_send(Frame::Cancel(cancellation));
        }
    }

    fn cancelled_by_client(&self, request_id: NativeHostRequestId) {
        let mut state = self.state.lock();
        if let Some(call) = state
            .pending
            .get_mut(&request_id)
            .filter(|call| call.dispatched)
        {
            if let Some(reply) = call.reply.take() {
                let _ = reply.send(NativeHostReply {
                    request_id,
                    connection: self.connection.clone(),
                    scope: call.scope.clone(),
                    completion: NativeHostCompletion::Uncertain(
                        NativeHostUncertainty::CancelledAfterDispatch,
                    ),
                });
            }
        }
    }
}

impl<UpdateRequest, UpdateResult> Session<UpdateRequest, UpdateResult> {
    fn cancel_request(&self, request_id: NativeHostRequestId) -> Option<bool> {
        let (dispatched, send_cancellation) = {
            let mut state = self.state.lock();
            let dispatched = state.pending.get(&request_id)?.dispatched;
            if !dispatched {
                state.pending.remove(&request_id);
                return Some(false);
            }
            let send_cancellation = state
                .pending
                .get_mut(&request_id)
                .is_some_and(|call| call.reply.take().is_some());
            (dispatched, send_cancellation)
        };
        if send_cancellation {
            let cancellation = NativeHostCancellation {
                request_id,
                connection: self.connection.clone(),
            };
            let _ = self.control.try_send(Frame::Cancel(cancellation));
        }
        Some(dispatched)
    }
}

fn scope_is_authorized(view: &Option<NativeViewBinding>, scope: &NativeHostScope) -> bool {
    match scope {
        NativeHostScope::Client => true,
        NativeHostScope::View(session_id) => view
            .as_ref()
            .is_some_and(|view| &view.session_id == session_id),
    }
}

impl<UpdateRequest, UpdateResult> Drop for PendingCallGuard<UpdateRequest, UpdateResult> {
    fn drop(&mut self) {
        if self.armed {
            self.session.cancel_request(self.request_id);
        }
    }
}

impl<UpdateRequest, UpdateResult> PendingCallGuard<UpdateRequest, UpdateResult> {
    fn disarm(&mut self) {
        self.armed = false;
    }
}

fn next_id(counter: &AtomicU64) -> Option<u64> {
    counter
        .try_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            current.checked_add(1)
        })
        .ok()
}
