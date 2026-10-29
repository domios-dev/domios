mod peer;
use peer::{validate_challenge, verify_native_peer};
mod bootstrap;
mod quit;
mod recovery;
mod startup_log;
pub(super) mod update;
#[cfg(test)]
pub(super) use bootstrap::read_single_bounded_line;
#[cfg(test)]
pub(super) use bootstrap::HelperFailure;
use bootstrap::VerifiedClientInstall;
pub(crate) use bootstrap::{
    startup_io_error, startup_reached, startup_refused, startup_started, ClientStartupFailure,
    ClientStartupStage,
};

pub(crate) fn record_client_only_quit() {
    startup_log::record("quit", "client_only_server_not_stopped", None, None);
}

use codeterm_process_identity::PinnedProcess;
use codeterm_types::{
    native_host::{
        NativeHostChallenge, NativeHostCompletion, NativeHostConnection, NativeHostEvent,
        NativeHostEventAudience, NativeHostFrame, NativeHostOperation, NativeHostRejection,
        NativeHostReply, NativeHostRequest, NativeHostRequestId, NativeHostResult, NativeHostScope,
        NativeHostUncertainty, NativeViewBinding, NativeWebviewDiagnosticsReport,
        NativeWebviewDiagnosticsSnapshot, MAX_NATIVE_HOST_FRAME_BYTES,
        MAX_PENDING_NATIVE_HOST_REQUESTS, NATIVE_HOST_PROTOCOL,
    },
    native_update::{NativeUpdateRequest, NativeUpdateResult},
    server_bootstrap::{LaunchOptions, ProcessProvenance, ServerComponentManifest, ServerReady},
};
use interprocess::local_socket::{
    tokio::{prelude::*, Stream},
    GenericNamespaced,
};
use parking_lot::{Mutex, RwLock};
#[cfg(test)]
use std::path::Path;
use std::{
    collections::{HashSet, VecDeque},
    io,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::{Emitter, Manager};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{mpsc, oneshot, watch, Semaphore},
    time::timeout,
};
use url::Url;

type AppHandle = codeterm_client_shell::app_runtime::AppHandle;
type Frame = NativeHostFrame<NativeUpdateRequest, NativeUpdateResult>;

const REQUEST_DEADLINE: Duration = Duration::from_secs(10);

enum NativeLocalOutcome {
    Completed(NativeHostResult<NativeUpdateResult>),
    Refused(NativeHostRejection),
    Uncertain(NativeHostUncertainty),
}

fn map_shell_result<T>(
    result: Result<T, super::NativeShellFailure>,
    map: impl FnOnce(T) -> NativeHostResult<NativeUpdateResult>,
) -> NativeLocalOutcome {
    match result {
        Ok(value) => NativeLocalOutcome::Completed(map(value)),
        Err(super::NativeShellFailure::Refused(reason)) => NativeLocalOutcome::Refused(reason),
        Err(super::NativeShellFailure::Uncertain(reason)) => NativeLocalOutcome::Uncertain(reason),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NativeClientStatus {
    Attached,
    NativeHostUnavailable { message: String, retryable: bool },
}

pub(crate) struct NativeClientInitialization {
    pub(crate) ready: ServerReady,
    pub(crate) native: NativeClientStatus,
}

#[derive(Clone)]
struct NativeClientInstallInputs {
    manifest: ServerComponentManifest,
    trusted_resource_root: PathBuf,
    launch_options: LaunchOptions,
}

#[derive(Clone)]
struct NativeClientInitializationGate {
    inputs: Arc<NativeClientInstallInputs>,
    running: Arc<AtomicBool>,
    attached_once: Arc<AtomicBool>,
}

impl NativeClientInitializationGate {
    fn acquire(&self) -> Option<NativeClientInitializationPermit> {
        self.running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        Some(NativeClientInitializationPermit(self.running.clone()))
    }
}

struct NativeClientInitializationPermit(Arc<AtomicBool>);

impl Drop for NativeClientInitializationPermit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ExactServerShutdownResult {
    Confirmed {
        server: ServerReady,
        client_generation: u64,
    },
    RefusedBeforeDispatch(ExactServerShutdownRefusal),
    UncertainAfterDispatch(ExactServerShutdownUncertainty),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExactServerShutdownRefusal {
    ApiAttachmentUnavailable,
    NativeHostUnavailable,
    IncompatibleServerProtocol,
    StaleIncarnation,
    ProcessInspectionFailed,
    HelperArtifactInvalid,
    VerificationIoFailure,
    VerificationDeadline,
    VerificationInterrupted,
    HelperDeadlineBeforeDispatch,
    HelperUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExactServerShutdownUncertainty {
    HelperOutcomeUnknown,
    HelperDeadlineAfterDispatch,
    InvalidReply,
    ServerExitUnobserved,
}

#[derive(Clone, Default)]
pub(crate) struct NativeHostClientSlot(Arc<RwLock<Option<Arc<NativeHostClient>>>>);

impl NativeHostClientSlot {
    pub(crate) async fn request_exact_server_shutdown(
        &self,
        api: &super::ServerApiAttachmentSlot,
        expected: Option<&codeterm_types::app_quit::QuitSnapshot>,
    ) -> ExactServerShutdownResult {
        let Some(server_api) = api.current() else {
            return ExactServerShutdownResult::RefusedBeforeDispatch(
                ExactServerShutdownRefusal::ApiAttachmentUnavailable,
            );
        };
        let Some(client) = self.0.read().clone() else {
            return ExactServerShutdownResult::RefusedBeforeDispatch(
                ExactServerShutdownRefusal::NativeHostUnavailable,
            );
        };
        if expected.is_some_and(|expected| !quit::matches(&client, expected, server_api.ready())) {
            return ExactServerShutdownResult::RefusedBeforeDispatch(
                ExactServerShutdownRefusal::ApiAttachmentUnavailable,
            );
        }
        client
            .request_exact_server_shutdown(server_api.ready().clone(), api, self)
            .await
    }

    fn is_current(&self, client: &Arc<NativeHostClient>) -> bool {
        self.0
            .read()
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(active, client))
    }

    pub(crate) fn bind_observed_view(
        &self,
        url: &Url,
        generation: super::ClientViewGeneration,
    ) -> Result<(), NativeHostRejection> {
        let Some(client) = self.0.read().clone() else {
            return Err(NativeHostRejection::NotAttached);
        };
        client.bind_observed_view(url, generation)
    }

    pub(crate) fn release_view(&self) {
        if let Some(client) = self.0.read().clone() {
            client.release_view();
        }
    }

    pub(crate) fn webview_diagnostics_snapshot(
        &self,
        generation: super::ClientViewGeneration,
    ) -> Option<NativeWebviewDiagnosticsSnapshot> {
        let client = self.0.read().clone()?;
        if *client.root_generation.lock() != Some(generation) {
            return None;
        }
        client.diagnostics_snapshot()
    }

    #[cfg(windows)]
    pub(crate) fn set_diagnostics_port_preflight(&self, port: u16, available: bool) {
        if let Some(client) = self.0.read().as_ref() {
            client.diagnostics.set_port_preflight(port, available);
        }
    }

    pub(crate) async fn report_root_rendered(
        &self,
        generation: super::ClientViewGeneration,
        url: Url,
    ) {
        let client = self.0.read().clone();
        if let Some(client) = client {
            client.report_root_rendered(generation, url).await;
        }
    }

    fn replace(&self, client: Arc<NativeHostClient>) {
        if let Some(previous) = self.0.write().replace(client) {
            previous.retire();
        }
    }

    fn clear_if_current(&self, client: &Arc<NativeHostClient>) -> bool {
        let mut active = self.0.write();
        if active
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, client))
        {
            active.take();
            true
        } else {
            false
        }
    }
}

pub(crate) async fn initialize_client(
    app: AppHandle,
    manifest: ServerComponentManifest,
    trusted_resource_root: PathBuf,
    launch_options: LaunchOptions,
) -> Result<NativeClientInitialization, ClientStartupFailure> {
    let inputs = Arc::new(NativeClientInstallInputs {
        manifest,
        trusted_resource_root,
        launch_options,
    });
    if app.try_state::<NativeClientInitializationGate>().is_some() {
        return Err("Native client initialization inputs are already installed".into());
    }
    let gate = NativeClientInitializationGate {
        inputs: inputs.clone(),
        running: Arc::new(AtomicBool::new(false)),
        attached_once: Arc::new(AtomicBool::new(false)),
    };
    if !app.manage(gate.clone()) {
        return Err("Native client initialization inputs could not be retained".into());
    }
    let _permit = gate
        .acquire()
        .ok_or_else(|| "Native client initialization is already active".to_string())?;
    let initialized = initialize_client_once(app, inputs, true).await?;
    gate.attached_once.store(true, Ordering::Release);
    Ok(initialized)
}

pub(crate) async fn retry_initialize_client(
    app: AppHandle,
) -> Result<NativeClientInitialization, ClientStartupFailure> {
    let Some(gate) = app
        .try_state::<NativeClientInitializationGate>()
        .map(|state| state.inner().clone())
    else {
        return crate::run::retry_client_installation(app).await;
    };
    let _permit = gate
        .acquire()
        .ok_or_else(|| "Native client initialization is already active".to_string())?;
    let inputs = recovery::retry_inputs(gate.inputs.as_ref().clone()).await?;
    // A dead server leaves the attach "unavailable"; relaunching it is safe unless the user is quitting.
    let allow_launch = !crate::crash::exit::is_client_exit_pending();
    let initialized = initialize_client_once(app, Arc::new(inputs), allow_launch).await?;
    gate.attached_once.store(true, Ordering::Release);
    Ok(initialized)
}

async fn initialize_client_once(
    app: AppHandle,
    inputs: Arc<NativeClientInstallInputs>,
    allow_launch: bool,
) -> Result<NativeClientInitialization, ClientStartupFailure> {
    let NativeClientInstallInputs {
        manifest,
        trusted_resource_root,
        launch_options,
    } = inputs.as_ref().clone();
    let verified = tokio::task::spawn_blocking(move || {
        bootstrap::verify_install(manifest, trusted_resource_root, launch_options)
    })
    .await
    .map_err(|error| format!("Client install verification task failed: {error}"))?
    .map_err(|error| {
        startup_io_error(ClientStartupStage::Install, &error);
        error.to_string()
    })?;

    let reply = bootstrap::attach_or_launch(&verified, allow_launch).await?;
    bootstrap::validate_ready(&verified, &reply.ready).map_err(|error| error.to_string())?;
    let token = reply
        .api_token
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| "Server bootstrap did not provide an API credential".to_string())?;

    if app.try_state::<super::ServerApiAttachmentSlot>().is_none() {
        let _ = app.manage(super::ServerApiAttachmentSlot::default());
    }
    let api = app
        .state::<super::ServerApiAttachmentSlot>()
        .inner()
        .clone();
    startup_started(ClientStartupStage::ApiAttachment);
    api.install(reply.ready.clone(), token)?;

    if app.try_state::<NativeHostClientSlot>().is_none() {
        let _ = app.manage(NativeHostClientSlot::default());
    }
    let slot = app.state::<NativeHostClientSlot>().inner().clone();
    let allowed_origins = crate::startup::frontend_shell::native_view_origins(&app, &reply.ready);
    let diagnostics_mode = app
        .try_state::<crate::ClientStartupOptions>()
        .map(|options| options.local_webview_diagnostics)
        .unwrap_or_default();

    startup_started(ClientStartupStage::NativeEnrollment);
    let native = match connect_native_host(
        app.clone(),
        &verified,
        &reply.ready,
        allowed_origins,
        diagnostics_mode,
    )
    .await
    {
        Ok((client, stream)) => {
            slot.replace(client.clone());
            tauri::async_runtime::spawn(async move {
                client.clone().run(stream).await;
                recovery::disconnected(&slot, &client);
            });
            NativeClientStatus::Attached
        }
        Err(error) => {
            startup_refused(ClientStartupStage::NativeEnrollment);
            log::debug!("[native-client] optional host attachment unavailable: {error}");
            NativeClientStatus::NativeHostUnavailable {
                message: error.message().into(),
                retryable: error.retryable(),
            }
        }
    };

    Ok(NativeClientInitialization {
        ready: reply.ready,
        native,
    })
}

async fn connect_native_host(
    app: AppHandle,
    install: &VerifiedClientInstall,
    ready: &ServerReady,
    allowed_view_origins: HashSet<String>,
    diagnostics_mode: crate::LocalWebviewDiagnosticsMode,
) -> Result<(Arc<NativeHostClient>, Stream), ClientStartupFailure> {
    let address = codeterm_types::native_host::native_host_socket_address(&ready.bootstrap_address)
        .ok_or_else(|| "Server native-host address is invalid".to_string())?;
    let name = address
        .to_ns_name::<GenericNamespaced>()
        .map_err(|error| error.to_string())?;
    let mut stream = timeout(REQUEST_DEADLINE, Stream::connect(name))
        .await
        .map_err(|_| {
            ClientStartupFailure::native_connection(io::Error::new(
                io::ErrorKind::TimedOut,
                "Native-host socket connection deadline elapsed",
            ))
        })?
        .map_err(ClientStartupFailure::native_connection)?;
    let server_process =
        Arc::new(verify_native_peer(&stream, ready).map_err(|error| error.to_string())?);

    let challenge_frame: Frame = timeout(REQUEST_DEADLINE, read_frame(&mut stream))
        .await
        .map_err(|_| "Native-host challenge deadline elapsed".to_string())?
        .map_err(|error| error.to_string())?;
    let NativeHostFrame::Challenge(challenge) = challenge_frame else {
        return Err("Native host did not begin with an enrollment challenge".into());
    };
    validate_challenge(&challenge, ready)?;
    timeout(
        REQUEST_DEADLINE,
        write_frame(
            &mut stream,
            &NativeHostFrame::<NativeUpdateRequest, NativeUpdateResult>::Enroll(
                codeterm_types::native_host::NativeHostEnrollmentRequest {
                    protocol: NATIVE_HOST_PROTOCOL,
                    challenge: challenge.challenge.clone(),
                    connection: challenge.connection.clone(),
                    view: None,
                },
            ),
        ),
    )
    .await
    .map_err(|_| "Native-host enrollment write deadline elapsed".to_string())?
    .map_err(|error| error.to_string())?;

    let enrollment_frame: Frame = timeout(REQUEST_DEADLINE, read_frame(&mut stream))
        .await
        .map_err(|_| "Native-host enrollment deadline elapsed".to_string())?
        .map_err(|error| error.to_string())?;
    let NativeHostFrame::Enrolled(enrollment) = enrollment_frame else {
        return Err("Native host did not acknowledge enrollment".into());
    };
    server_process
        .ensure_alive()
        .map_err(|error| error.to_string())?;
    let own_process = &install.client_process;
    if enrollment.protocol != NATIVE_HOST_PROTOCOL
        || enrollment.challenge != challenge.challenge
        || enrollment.connection != challenge.connection
        || enrollment.view.is_some()
        || enrollment.client_process.pid != own_process.pid
        || enrollment.client_process != *own_process
    {
        return Err("Native-host enrollment does not match this client process".into());
    }

    let (outbound, outbound_rx) = mpsc::channel(MAX_PENDING_NATIVE_HOST_REQUESTS);
    let (shutdown, _) = watch::channel(false);
    let client = Arc::new(NativeHostClient {
        app,
        connection: challenge.connection,
        server_process,
        ready: ready.clone(),
        manifest: install.manifest.clone(),
        resource_root: install.resource_root.clone(),
        server_executable: install.server_executable.clone(),
        client_process: enrollment.client_process,
        allowed_view_origins,
        outbound,
        outbound_rx: Mutex::new(Some(outbound_rx)),
        shutdown,
        retired: AtomicBool::new(false),
        view: Mutex::new(None),
        root_generation: Mutex::new(None),
        diagnostics: super::webview_diagnostics::DiagnosticsSession::new(diagnostics_mode),
        #[cfg(debug_assertions)]
        publish_dev_locator: super::webview_diagnostics::dev_locator_enabled(
            cfg!(debug_assertions),
            &install.launch_options.instance,
        ),
        pending: Mutex::new(HashSet::new()),
        request_ids: Mutex::new(super::RequestReplayWindow::default()),
        cancellations: Mutex::new(VecDeque::new()),
        last_event_id: AtomicU64::new(0),
    });
    let recovering = client.clone();
    match tokio::task::spawn_blocking(move || update::recover(&recovering)).await {
        Ok(Ok(())) => {}
        outcome => log::warn!("[updater] native handoff recovery skipped: {outcome:?}"),
    }
    Ok((client, stream))
}

struct NativeHostClient {
    app: AppHandle,
    connection: NativeHostConnection,
    server_process: Arc<PinnedProcess>,
    ready: ServerReady,
    manifest: ServerComponentManifest,
    resource_root: PathBuf,
    server_executable: PathBuf,
    client_process: ProcessProvenance,
    allowed_view_origins: HashSet<String>,
    outbound: mpsc::Sender<Frame>,
    outbound_rx: Mutex<Option<mpsc::Receiver<Frame>>>,
    shutdown: watch::Sender<bool>,
    retired: AtomicBool,
    view: Mutex<Option<NativeViewBinding>>,
    root_generation: Mutex<Option<super::ClientViewGeneration>>,
    diagnostics: super::webview_diagnostics::DiagnosticsSession,
    #[cfg(debug_assertions)]
    publish_dev_locator: bool,
    pending: Mutex<HashSet<NativeHostRequestId>>,
    request_ids: Mutex<super::RequestReplayWindow>,
    cancellations: Mutex<VecDeque<(NativeHostRequestId, oneshot::Sender<()>)>>,
    last_event_id: AtomicU64,
}

impl NativeHostClient {
    fn bind_observed_view(
        &self,
        url: &Url,
        generation: super::ClientViewGeneration,
    ) -> Result<(), NativeHostRejection> {
        self.release_view();
        let origin = url.origin().ascii_serialization();
        if !self.allowed_view_origins.contains(&origin) {
            return Err(NativeHostRejection::InvalidView);
        }
        let view =
            NativeViewBinding::observed(origin).map_err(|_| NativeHostRejection::InvalidView)?;
        *self.root_generation.lock() = Some(generation);
        *self.view.lock() = Some(view.clone());
        let status = self.diagnostics.bind_view();
        self.try_send(Frame::BindView {
            connection: self.connection.clone(),
            view: view.clone(),
        })?;
        self.try_send(Frame::DiagnosticsStatus(NativeWebviewDiagnosticsReport {
            connection: self.connection.clone(),
            view_session: view.session_id,
            status,
        }))
    }

    fn release_view(&self) {
        let view = self.view.lock().take();
        self.root_generation.lock().take();
        self.diagnostics.release_view();
        if let Some(view) = view {
            let _ = self.try_send(Frame::ReleaseView {
                connection: self.connection.clone(),
                session_id: view.session_id,
            });
        }
    }

    async fn report_root_rendered(&self, generation: super::ClientViewGeneration, url: Url) {
        let Some(view) = self.view.lock().clone() else {
            return;
        };
        if self.retired.load(Ordering::Acquire)
            || *self.root_generation.lock() != Some(generation)
            || view.origin != url.origin().ascii_serialization()
        {
            return;
        }
        let status = self.diagnostics.inspect_view(generation, url.clone()).await;
        if self.retired.load(Ordering::Acquire)
            || *self.root_generation.lock() != Some(generation)
            || self.view.lock().as_ref().map(|view| view.session_id) != Some(view.session_id)
        {
            return;
        }
        self.diagnostics.adopt_status(status.clone());
        #[cfg(debug_assertions)]
        if self.publish_dev_locator {
            if let Some(snapshot) = self.diagnostics_snapshot() {
                eprintln!(
                    "DOMIOS_DEV_NATIVE_LOCATOR {}",
                    serde_json::json!({
                        "snapshot": snapshot,
                        "navigation_generation": generation.navigation,
                        "origin": url.origin().ascii_serialization(),
                    })
                );
            }
        }
        if self
            .try_send(Frame::DiagnosticsStatus(NativeWebviewDiagnosticsReport {
                connection: self.connection.clone(),
                view_session: view.session_id,
                status,
            }))
            .is_err()
        {
            self.retire();
        }
    }

    fn diagnostics_snapshot(&self) -> Option<NativeWebviewDiagnosticsSnapshot> {
        if self.retired.load(Ordering::Acquire) || self.server_process.ensure_alive().is_err() {
            return None;
        }
        let view = self.view.lock().clone()?;
        self.diagnostics
            .snapshot(&self.connection, view.session_id, &self.client_process)
    }

    fn try_send(&self, frame: Frame) -> Result<(), NativeHostRejection> {
        if self.retired.load(Ordering::Acquire) || self.server_process.ensure_alive().is_err() {
            self.retire();
            return Err(NativeHostRejection::NotAttached);
        }
        self.outbound
            .try_send(frame)
            .map_err(|_| NativeHostRejection::QueueFull)
    }

    fn retire(&self) {
        if self.retired.swap(true, Ordering::AcqRel) {
            return;
        }
        self.view.lock().take();
        self.root_generation.lock().take();
        self.diagnostics.release_view();
        self.shutdown.send_replace(true);
        for (_, cancellation) in self.cancellations.lock().drain(..) {
            let _ = cancellation.send(());
        }
    }

    async fn run(self: Arc<Self>, stream: Stream) {
        let Some(outbound_rx) = self.outbound_rx.lock().take() else {
            self.retire();
            return;
        };
        let (reader, writer) = tokio::io::split(stream);
        let writer_client = self.clone();
        let mut writer_shutdown = self.shutdown.subscribe();
        let writer_task = tokio::spawn(async move {
            writer_client
                .write_loop(writer, outbound_rx, &mut writer_shutdown)
                .await;
            writer_client.retire();
        });
        self.read_loop(reader).await;
        self.retire();
        let _ = writer_task.await;
    }

    async fn write_loop<W>(
        &self,
        mut writer: W,
        mut outbound: mpsc::Receiver<Frame>,
        shutdown: &mut watch::Receiver<bool>,
    ) where
        W: tokio::io::AsyncWrite + Unpin,
    {
        loop {
            let frame = tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() { break; }
                    continue;
                }
                frame = outbound.recv() => match frame {
                    Some(frame) => frame,
                    None => break,
                },
            };
            if !matches!(
                timeout(REQUEST_DEADLINE, write_frame(&mut writer, &frame)).await,
                Ok(Ok(()))
            ) {
                break;
            }
        }
        let _ = writer.shutdown().await;
    }

    async fn read_loop<R>(self: &Arc<Self>, mut reader: R)
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        let limit = Arc::new(Semaphore::new(MAX_PENDING_NATIVE_HOST_REQUESTS));
        let mut shutdown = self.shutdown.subscribe();
        loop {
            let frame = tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() { break; }
                    continue;
                }
                frame = read_frame(&mut reader) => match frame {
                    Ok(frame) => frame,
                    Err(_) => break,
                },
            };
            if self.server_process.ensure_alive().is_err() {
                break;
            }
            match frame {
                NativeHostFrame::Request(request) => {
                    self.accept_request(request, limit.clone());
                }
                NativeHostFrame::Event(event) => self.accept_event(event),
                NativeHostFrame::Cancel(cancel) if cancel.connection == self.connection => {
                    self.cancel_request(cancel.request_id);
                }
                _ => break,
            }
            if self.retired.load(Ordering::Acquire) {
                break;
            }
        }
    }

    fn accept_request(
        self: &Arc<Self>,
        request: NativeHostRequest<NativeUpdateRequest>,
        limit: Arc<Semaphore>,
    ) {
        if self.server_process.ensure_alive().is_err() {
            self.retire();
            return;
        }
        if request.connection != self.connection {
            self.retire();
            return;
        }
        if request.request_id.0 == 0 {
            self.retire();
            return;
        }
        if !self.request_ids.lock().accept(request.request_id.0) {
            self.send_rejection(request, NativeHostRejection::ProtocolMismatch);
            return;
        }
        if !self.scope_is_current(&request.scope) {
            self.send_reply(NativeHostReply {
                request_id: request.request_id,
                connection: self.connection.clone(),
                scope: request.scope,
                completion: NativeHostCompletion::Rejected(NativeHostRejection::InvalidView),
            });
            return;
        }
        let Ok(permit) = limit.try_acquire_owned() else {
            self.send_rejection(request, NativeHostRejection::QueueFull);
            return;
        };
        let (cancel_tx, cancel_rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock();
            if pending.len() >= MAX_PENDING_NATIVE_HOST_REQUESTS {
                drop(pending);
                self.send_rejection(request, NativeHostRejection::QueueFull);
                return;
            }
            if !pending.insert(request.request_id) {
                drop(pending);
                self.send_rejection(request, NativeHostRejection::ProtocolMismatch);
                return;
            }
        }
        self.cancellations
            .lock()
            .push_back((request.request_id, cancel_tx));
        let client = self.clone();
        tauri::async_runtime::spawn(async move {
            let _permit = permit;
            let reply = client.dispatch_request(request.clone(), cancel_rx).await;
            client.pending.lock().remove(&request.request_id);
            client.remove_cancellation(request.request_id);
            if let Some(reply) = reply {
                client.send_reply(reply);
            }
        });
    }

    fn send_rejection(
        &self,
        request: NativeHostRequest<NativeUpdateRequest>,
        reason: NativeHostRejection,
    ) {
        self.send_reply(NativeHostReply {
            request_id: request.request_id,
            connection: self.connection.clone(),
            scope: request.scope,
            completion: NativeHostCompletion::Rejected(reason),
        });
    }

    fn send_reply(&self, reply: NativeHostReply<NativeUpdateResult>) {
        if self.retired.load(Ordering::Acquire) {
            return;
        }
        if self.try_send(Frame::Reply(reply)).is_err() {
            self.retire();
        }
    }

    fn cancel_request(&self, request_id: NativeHostRequestId) {
        let cancellation = {
            let mut cancellations = self.cancellations.lock();
            cancellations
                .iter()
                .position(|(id, _)| *id == request_id)
                .and_then(|index| cancellations.remove(index))
        };
        if let Some((_, cancellation)) = cancellation {
            let _ = cancellation.send(());
        }
    }

    fn remove_cancellation(&self, request_id: NativeHostRequestId) {
        let mut cancellations = self.cancellations.lock();
        if let Some(index) = cancellations.iter().position(|(id, _)| *id == request_id) {
            cancellations.remove(index);
        }
    }

    fn scope_is_current(&self, scope: &NativeHostScope) -> bool {
        if self.retired.load(Ordering::Acquire) {
            return false;
        }
        match scope {
            NativeHostScope::Client => true,
            NativeHostScope::View(session_id) => self
                .view
                .lock()
                .as_ref()
                .is_some_and(|view| &view.session_id == session_id),
        }
    }

    async fn dispatch_request(
        self: &Arc<Self>,
        request: NativeHostRequest<NativeUpdateRequest>,
        mut cancel: oneshot::Receiver<()>,
    ) -> Option<NativeHostReply<NativeUpdateResult>> {
        if !self.scope_is_current(&request.scope) {
            return Some(self.reply_rejected(request, NativeHostRejection::InvalidView));
        }
        if matches!(&request.operation, NativeHostOperation::ExactQuit { .. })
            && !matches!(&request.scope, NativeHostScope::Client)
        {
            return Some(self.reply_rejected(request, NativeHostRejection::Unauthorized));
        }
        if matches!(
            &request.operation,
            NativeHostOperation::NativeUpdate { .. }
                | NativeHostOperation::ObserveNativeUpdate { .. }
        ) {
            if request.scope != NativeHostScope::Client {
                return Some(self.reply_rejected(request, NativeHostRejection::Unauthorized));
            }
            let operation = request.operation.clone();
            let completion = match operation {
                NativeHostOperation::NativeUpdate {
                    request: update,
                    apply: Some(options),
                } => self
                    .apply_update(update, options)
                    .await
                    .map(NativeHostResult::NativeUpdateApply),
                NativeHostOperation::ObserveNativeUpdate {
                    operation_id,
                    after_sequence,
                } => {
                    let observe = self.observe_update(operation_id, after_sequence);
                    tokio::select! {
                        _ = &mut cancel => return None,
                        result = timeout(codeterm_types::native_host::NATIVE_UPDATE_OBSERVATION_DEADLINE, observe) => {
                            result.unwrap_or(Err(NativeHostRejection::DeadlineBeforeDispatch)).map(NativeHostResult::NativeUpdateApply)
                        }
                    }
                }
                _ => Err(NativeHostRejection::Unsupported),
            };
            return Some(self.reply(
                request,
                match completion {
                    Ok(result) => NativeHostCompletion::Completed(result),
                    Err(reason) => NativeHostCompletion::Rejected(reason),
                },
            ));
        }
        let operation = request.operation.clone();
        let client = self.clone();
        let action =
            tokio::task::spawn_blocking(move || client.dispatch_local_operation(operation));
        tokio::select! {
            biased;
            _ = &mut cancel => None,
            outcome = timeout(REQUEST_DEADLINE, action) => {
                let completion = match outcome {
                    Ok(Ok(NativeLocalOutcome::Completed(result))) => NativeHostCompletion::Completed(result),
                    Ok(Ok(NativeLocalOutcome::Refused(reason))) => NativeHostCompletion::Rejected(reason),
                    Ok(Ok(NativeLocalOutcome::Uncertain(reason))) => NativeHostCompletion::Uncertain(reason),
                    Ok(Err(_)) => NativeHostCompletion::Uncertain(NativeHostUncertainty::DisconnectedAfterDispatch),
                    Err(_) => NativeHostCompletion::Uncertain(NativeHostUncertainty::DeadlineAfterDispatch),
                };
                Some(self.reply(request, completion))
            }
        }
    }

    fn dispatch_local_operation(
        &self,
        operation: NativeHostOperation<NativeUpdateRequest>,
    ) -> NativeLocalOutcome {
        let shell = super::TauriShell(self.app.clone());
        match operation {
            NativeHostOperation::ExactQuit { expected } => quit::dispatch(self, *expected),
            NativeHostOperation::FocusWindow { label } => {
                map_shell_result(shell.focus_window_native(label), |_| {
                    NativeHostResult::Acknowledged
                })
            }
            NativeHostOperation::WindowExists { label } => map_shell_result(
                shell.window_exists_native(label),
                NativeHostResult::WindowExists,
            ),
            NativeHostOperation::OsNotify { title, body } => {
                crate::os_notify::_send_os_notification(&self.app, &title, body.as_deref());
                NativeLocalOutcome::Completed(NativeHostResult::Acknowledged)
            }
            NativeHostOperation::ToastPermission { enabling } => {
                NativeLocalOutcome::Completed(NativeHostResult::ToastPermission(
                    match enabling {
                        Some(enabling) => {
                            crate::os_notify::establish_toast_permission(&self.app, enabling)
                        }
                        None => crate::os_notify::toast_permission(&self.app),
                    }
                    .to_string(),
                ))
            }
            NativeHostOperation::OpenWebview { request } => {
                let Ok(url) = Url::parse(&request.url) else {
                    return NativeLocalOutcome::Refused(NativeHostRejection::InvalidView);
                };
                let origin = url.origin().ascii_serialization();
                if !self.allowed_view_origins.contains(&origin)
                    || !request.width.is_finite()
                    || !request.height.is_finite()
                    || request.width <= 0.0
                    || request.height <= 0.0
                    || request.label.trim().is_empty()
                {
                    return NativeLocalOutcome::Refused(NativeHostRejection::InvalidView);
                }
                map_shell_result(
                    shell.open_webview_native(
                        request.label,
                        url,
                        request.title,
                        request.width,
                        request.height,
                    ),
                    |_| NativeHostResult::Acknowledged,
                )
            }
            NativeHostOperation::CopyImageToClipboard { .. }
            | NativeHostOperation::NativeUpdate { .. }
            | NativeHostOperation::ObserveNativeUpdate { .. } => {
                NativeLocalOutcome::Refused(NativeHostRejection::Unsupported)
            }
        }
    }

    fn reply_rejected(
        &self,
        request: NativeHostRequest<NativeUpdateRequest>,
        reason: NativeHostRejection,
    ) -> NativeHostReply<NativeUpdateResult> {
        self.reply(request, NativeHostCompletion::Rejected(reason))
    }

    fn reply(
        &self,
        request: NativeHostRequest<NativeUpdateRequest>,
        completion: NativeHostCompletion<NativeHostResult<NativeUpdateResult>>,
    ) -> NativeHostReply<NativeUpdateResult> {
        NativeHostReply {
            request_id: request.request_id,
            connection: self.connection.clone(),
            scope: request.scope,
            completion,
        }
    }

    fn accept_event(&self, event: NativeHostEvent) {
        if self.retired.load(Ordering::Acquire)
            || event.connection != self.connection
            || event.event_id == 0
            || self
                .last_event_id
                .try_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                    (event.event_id > current).then_some(event.event_id)
                })
                .is_err()
        {
            return;
        }
        if event.name.trim().is_empty() || event.name.chars().any(char::is_control) {
            return;
        }
        match event.audience {
            NativeHostEventAudience::Generic => {}
            NativeHostEventAudience::Native => {
                let _ = self.app.emit(&event.name, event.payload);
            }
            NativeHostEventAudience::Targeted(target) if !target.trim().is_empty() => {
                let _ = self.app.emit_to(target, &event.name, event.payload);
            }
            NativeHostEventAudience::Targeted(_) => {}
        }
    }
}

async fn read_frame<R>(reader: &mut R) -> io::Result<Frame>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let count = reader.read_u32().await? as usize;
    if count == 0 || count > MAX_NATIVE_HOST_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Native host frame length exceeds its bound",
        ));
    }
    let mut bytes = vec![0; count];
    reader.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

async fn write_frame<W>(writer: &mut W, frame: &Frame) -> io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let bytes = serde_json::to_vec(frame).map_err(io::Error::other)?;
    if bytes.is_empty() || bytes.len() > MAX_NATIVE_HOST_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Native host frame length exceeds its bound",
        ));
    }
    writer.write_u32(bytes.len() as u32).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_diagnostic_never_serializes_error_text_or_request_data() {
        let error = std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "secret callback-url bearer-token request-body",
        );
        let value = bootstrap::diagnostic_payload(
            ClientStartupStage::Install,
            "io_error",
            Some(&error),
            None,
        );
        let encoded = value.to_string();
        assert!(!encoded.contains("secret"));
        assert!(!encoded.contains("callback-url"));
        assert_eq!(value["io_kind"], "PermissionDenied");
        assert_eq!(value["stage"], "install");
        assert!(encoded.len() < 256);
    }

    #[test]
    fn component_manifest_rejects_absolute_and_parent_server_paths() {
        assert!(!bootstrap::valid_server_relative_path(Path::new("/server")));
        assert!(!bootstrap::valid_server_relative_path(Path::new(
            "../server"
        )));
        assert!(bootstrap::valid_server_relative_path(Path::new(
            "bin/domios-server"
        )));
    }

    #[test]
    fn digest_format_requires_exact_lowercase_sha256() {
        assert!(bootstrap::valid_digest(&"a".repeat(64)));
        assert!(!bootstrap::valid_digest(&"A".repeat(64)));
        assert!(!bootstrap::valid_digest(&"a".repeat(63)));
    }

    #[test]
    fn shutdown_cleanup_completion_rejects_protocol_one() {
        assert!(bootstrap::supports_shutdown_completion_protocol(
            codeterm_types::server_bootstrap::BOOTSTRAP_PROTOCOL
        ));
        assert!(!bootstrap::supports_shutdown_completion_protocol(1));
    }
}
