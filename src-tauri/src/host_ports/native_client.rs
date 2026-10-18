use codeterm_types::{
    native_host::{
        NativeHostChallenge, NativeHostCompletion, NativeHostConnection, NativeHostEvent,
        NativeHostEventAudience, NativeHostFrame, NativeHostOperation, NativeHostRejection,
        NativeHostReply, NativeHostRequest, NativeHostRequestId, NativeHostResult, NativeHostScope,
        NativeHostUncertainty, NativeViewBinding, NativeWebviewDiagnosticsReport,
        NativeWebviewDiagnosticsSnapshot, MAX_NATIVE_HOST_FRAME_BYTES,
        MAX_PENDING_NATIVE_HOST_REQUESTS, NATIVE_HOST_PROTOCOL,
    },
    native_update::{NativeUpdateObservationOutcome, NativeUpdateRequest, NativeUpdateResult},
    server_bootstrap::{
        AttachReply, AttachRequest, BootstrapOperation, InstanceSelector, LaunchOptions,
        ProcessProvenance, ServerComponentManifest, ServerExpectation, ServerIdentity, ServerReady,
        API_VERSION, BOOTSTRAP_PROTOCOL, SERVER_COMPONENT_SCHEMA,
    },
};
use interprocess::local_socket::{
    tokio::{prelude::*, Stream},
    GenericNamespaced,
};
use parking_lot::{Mutex, RwLock};
use serde::{de::DeserializeOwned, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashSet, VecDeque},
    fs::File,
    io::{self, Read},
    path::{Component, Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::{Emitter, Manager};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::{mpsc, oneshot, watch, Semaphore},
    time::timeout,
};
use url::Url;

type AppHandle = codeterm_host_runtime::app_runtime::AppHandle;
type Frame = NativeHostFrame<NativeUpdateRequest, NativeUpdateResult>;

const BOOTSTRAP_DEADLINE: Duration = Duration::from_secs(30);
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
    NativeHostUnavailable,
}

pub(crate) struct NativeClientInitialization {
    pub(crate) ready: ServerReady,
    pub(crate) native: NativeClientStatus,
}

#[derive(Clone, Default)]
pub(crate) struct NativeHostClientSlot(Arc<RwLock<Option<Arc<NativeHostClient>>>>);

impl NativeHostClientSlot {
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

    pub(crate) fn webview_diagnostics_snapshot(&self) -> Option<NativeWebviewDiagnosticsSnapshot> {
        self.0.read().as_ref()?.diagnostics_snapshot()
    }

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

    fn clear_if_current(&self, client: &Arc<NativeHostClient>) {
        let mut active = self.0.write();
        if active
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, client))
        {
            active.take();
        }
    }
}

struct VerifiedClientInstall {
    manifest: ServerComponentManifest,
    resource_root: PathBuf,
    server_executable: PathBuf,
    client_process: ProcessProvenance,
    identity: ServerIdentity,
    launch_options: LaunchOptions,
}

pub(crate) async fn initialize_client(
    app: AppHandle,
    manifest: ServerComponentManifest,
    trusted_resource_root: PathBuf,
    launch_options: LaunchOptions,
) -> Result<NativeClientInitialization, String> {
    let verified = tokio::task::spawn_blocking(move || {
        verify_install(manifest, trusted_resource_root, launch_options)
    })
    .await
    .map_err(|error| format!("Client install verification task failed: {error}"))?
    .map_err(|error| error.to_string())?;

    let reply = attach_or_launch(&verified).await?;
    validate_ready(&verified, &reply.ready).map_err(|error| error.to_string())?;
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
                slot.clear_if_current(&client);
            });
            NativeClientStatus::Attached
        }
        Err(error) => {
            log::debug!("[native-client] optional host attachment unavailable: {error}");
            NativeClientStatus::NativeHostUnavailable
        }
    };

    Ok(NativeClientInitialization {
        ready: reply.ready,
        native,
    })
}

fn verify_install(
    manifest: ServerComponentManifest,
    trusted_resource_root: PathBuf,
    launch_options: LaunchOptions,
) -> io::Result<VerifiedClientInstall> {
    if manifest.schema != SERVER_COMPONENT_SCHEMA
        || manifest.api_version != API_VERSION
        || manifest.bootstrap_protocol != BOOTSTRAP_PROTOCOL
        || manifest.server_build.trim().is_empty()
        || !valid_digest(&manifest.server_sha256)
        || !valid_digest(&manifest.client_sha256)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Installed component manifest is invalid",
        ));
    }
    if launch_options.probe_sink.is_some() || launch_options.probe_identity_alias.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Native client cannot forward process-probe identities",
        ));
    }
    launch_options.validate(cfg!(debug_assertions))?;

    let resource_root = trusted_resource_root.canonicalize()?;
    if !resource_root.is_dir() || launch_options.resources.canonicalize()? != resource_root {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Server resources differ from the verified component root",
        ));
    }
    let server_relative = &manifest.server_executable;
    if !valid_server_relative_path(server_relative) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Server executable must be a confined resource-relative path",
        ));
    }
    let server_executable = resource_root.join(server_relative).canonicalize()?;
    if !server_executable.starts_with(&resource_root) || !server_executable.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Server executable escaped the verified resource root",
        ));
    }
    if file_sha256(&server_executable)? != manifest.server_sha256 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Server executable digest does not match the installed manifest",
        ));
    }

    let client_executable = std::env::current_exe()?.canonicalize()?;
    if client_executable == server_executable
        || file_sha256(&client_executable)? != manifest.client_sha256
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Client executable differs from the installed component manifest",
        ));
    }

    let paths = codeterm_paths::instance_identity::ResolvedInstancePaths::current()?;
    Ok(VerifiedClientInstall {
        manifest,
        resource_root,
        server_executable,
        client_process: ProcessProvenance {
            pid: std::process::id(),
            created: String::new(),
            executable: client_executable,
        },
        identity: ServerIdentity {
            data: paths.data().to_path_buf(),
            config: paths.config().to_path_buf(),
        },
        launch_options,
    })
}

fn valid_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_server_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn file_sha256(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 32 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

async fn attach_or_launch(install: &VerifiedClientInstall) -> Result<AttachReply, String> {
    let inspect = attach_request(install);
    match invoke_helper(&install.server_executable, "attach", &inspect).await {
        Ok(line) => validate_attach_reply(
            install,
            &inspect,
            parse_line(&line).map_err(|error| error.to_string())?,
            true,
        )
        .map_err(|error| error.to_string()),
        Err(_) => {
            match invoke_helper(
                &install.server_executable,
                "launch",
                &install.launch_options,
            )
            .await
            {
                Ok(line) => validate_attach_reply(
                    install,
                    &inspect,
                    parse_line(&line).map_err(|error| error.to_string())?,
                    false,
                )
                .map_err(|error| error.to_string()),
                Err(_) => {
                    let retry = attach_request(install);
                    let line = invoke_helper(&install.server_executable, "attach", &retry).await?;
                    validate_attach_reply(
                        install,
                        &retry,
                        parse_line(&line).map_err(|error| error.to_string())?,
                        true,
                    )
                    .map_err(|error| error.to_string())
                }
            }
        }
    }
}

fn attach_request(install: &VerifiedClientInstall) -> AttachRequest {
    AttachRequest {
        protocol: install.manifest.bootstrap_protocol,
        api_version: install.manifest.api_version,
        build: install.manifest.server_build.clone(),
        identity: install.identity.clone(),
        challenge: uuid::Uuid::new_v4().to_string(),
        operation: BootstrapOperation::Inspect,
        expected: None,
    }
}

fn validate_attach_reply(
    install: &VerifiedClientInstall,
    request: &AttachRequest,
    reply: AttachReply,
    attached: bool,
) -> io::Result<AttachReply> {
    if attached && reply.challenge != request.challenge || !attached && !reply.challenge.is_empty()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Server helper challenge does not match its bootstrap operation",
        ));
    }
    validate_ready(install, &reply.ready)?;
    Ok(reply)
}

fn validate_ready(install: &VerifiedClientInstall, ready: &ServerReady) -> io::Result<()> {
    let process_executable = ready.process.executable.canonicalize()?;
    let resources = ready.resources.canonicalize()?;
    if ready.protocol != install.manifest.bootstrap_protocol
        || ready.api_version != install.manifest.api_version
        || ready.build != install.manifest.server_build
        || ready.identity != install.identity
        || ready.epoch.trim().is_empty()
        || ready.instance_id.trim().is_empty()
        || ready.process.pid == 0
        || ready.process.created.trim().is_empty()
        || process_executable != install.server_executable
        || resources != install.resource_root
        || !ready.api_address.ip().is_loopback()
        || codeterm_types::native_host::native_host_socket_address(&ready.bootstrap_address)
            .is_none()
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Server readiness differs from the trusted installed identity",
        ));
    }
    Ok(())
}

async fn invoke_helper<T: Serialize>(
    executable: &Path,
    command_name: &str,
    request: &T,
) -> Result<String, String> {
    let mut child_command = Command::new(executable);
    child_command
        .arg(command_name)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
        child_command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = child_command
        .spawn()
        .map_err(|error| format!("Server helper could not start: {error}"))?;
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| "Server helper input pipe is unavailable".to_string())?;
    let bytes = serde_json::to_vec(request).map_err(|error| error.to_string())?;
    if bytes.is_empty() || bytes.len() >= MAX_NATIVE_HOST_FRAME_BYTES {
        return Err("Server helper request exceeds its bound".into());
    }
    timeout(BOOTSTRAP_DEADLINE, async {
        input.write_all(&bytes).await?;
        input.write_all(b"\n").await?;
        input.flush().await
    })
    .await
    .map_err(|_| "Server helper request write deadline elapsed".to_string())?
    .map_err(|error| error.to_string())?;
    drop(input);

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Server helper output pipe is unavailable".to_string())?;
    let mut reader = tokio::io::BufReader::new(stdout);
    let mut line = Vec::new();
    let count = timeout(BOOTSTRAP_DEADLINE, reader.read_until(b'\n', &mut line))
        .await
        .map_err(|_| "Server helper readiness deadline elapsed".to_string())?
        .map_err(|error| error.to_string())?;
    if count == 0 || count > MAX_NATIVE_HOST_FRAME_BYTES || line.last() != Some(&b'\n') {
        let _ = child.kill().await;
        return Err("Server helper returned no bounded readiness frame".into());
    }
    let status = timeout(BOOTSTRAP_DEADLINE, child.wait())
        .await
        .map_err(|_| "Server helper exit deadline elapsed".to_string())?
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err("Server helper refused the requested operation".into());
    }
    String::from_utf8(line).map_err(|error| error.to_string())
}

fn parse_line<T: serde::de::DeserializeOwned>(line: &str) -> io::Result<T> {
    serde_json::from_str(line.trim_end()).map_err(io::Error::other)
}

async fn connect_native_host(
    app: AppHandle,
    install: &VerifiedClientInstall,
    ready: &ServerReady,
    allowed_view_origins: HashSet<String>,
    diagnostics_mode: crate::LocalWebviewDiagnosticsMode,
) -> Result<(Arc<NativeHostClient>, Stream), String> {
    let address = codeterm_types::native_host::native_host_socket_address(&ready.bootstrap_address)
        .ok_or_else(|| "Server native-host address is invalid".to_string())?;
    let name = address
        .to_ns_name::<GenericNamespaced>()
        .map_err(|error| error.to_string())?;
    let mut stream = timeout(REQUEST_DEADLINE, Stream::connect(name))
        .await
        .map_err(|_| "Native-host socket connection deadline elapsed".to_string())?
        .map_err(|error| error.to_string())?;
    verify_native_peer(&stream, ready).map_err(|error| error.to_string())?;

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
    let own_process = &install.client_process;
    if enrollment.protocol != NATIVE_HOST_PROTOCOL
        || enrollment.challenge != challenge.challenge
        || enrollment.connection != challenge.connection
        || enrollment.view.is_some()
        || enrollment.client_process.pid != own_process.pid
        || enrollment.client_process.executable.canonicalize().ok()
            != Some(own_process.executable.clone())
        || enrollment.client_process.created.trim().is_empty()
    {
        return Err("Native-host enrollment does not match this client process".into());
    }

    let (outbound, outbound_rx) = mpsc::channel(MAX_PENDING_NATIVE_HOST_REQUESTS);
    let (shutdown, _) = watch::channel(false);
    let client = Arc::new(NativeHostClient {
        app,
        connection: challenge.connection,
        client_process: enrollment.client_process,
        allowed_view_origins,
        outbound,
        outbound_rx: Mutex::new(Some(outbound_rx)),
        shutdown,
        retired: AtomicBool::new(false),
        view: Mutex::new(None),
        root_generation: Mutex::new(None),
        diagnostics: super::webview_diagnostics::DiagnosticsSession::new(diagnostics_mode),
        pending: Mutex::new(HashSet::new()),
        request_ids: Mutex::new(super::RequestReplayWindow::default()),
        cancellations: Mutex::new(VecDeque::new()),
        last_event_id: AtomicU64::new(0),
    });
    Ok((client, stream))
}

fn validate_challenge(challenge: &NativeHostChallenge, ready: &ServerReady) -> Result<(), String> {
    if challenge.protocol != NATIVE_HOST_PROTOCOL
        || uuid::Uuid::parse_str(&challenge.challenge).is_err()
        || challenge.connection.instance_id != ready.instance_id
        || challenge.connection.identity != ready.identity
        || challenge.connection.server_epoch != ready.epoch
        || challenge.connection.generation == 0
    {
        return Err("Native-host challenge is stale or belongs to another server".into());
    }
    Ok(())
}

fn verify_native_peer(stream: &Stream, ready: &ServerReady) -> io::Result<()> {
    let credentials = stream.peer_creds()?;
    #[cfg(unix)]
    if credentials.euid() != Some(unsafe { native_effective_uid() }) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Native-host socket belongs to another user",
        ));
    }
    let pid = native_peer_pid(stream)?;
    if pid != ready.process.pid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Native-host socket peer differs from the inspected server process",
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn native_peer_pid(stream: &Stream) -> io::Result<u32> {
    stream
        .peer_creds()?
        .pid()
        .and_then(|pid| u32::try_from(pid).ok())
        .filter(|pid| *pid > 0)
        .ok_or_else(|| io::Error::other("Native-host peer PID is unavailable"))
}

#[cfg(target_os = "macos")]
fn native_peer_pid(stream: &Stream) -> io::Result<u32> {
    use std::os::fd::{AsFd, AsRawFd};
    let Stream::UdSocket(socket) = stream;
    let mut pid = 0i32;
    let mut size = std::mem::size_of_val(&pid) as u32;
    let result = unsafe {
        getsockopt(
            socket.as_fd().as_raw_fd(),
            0,
            2,
            (&mut pid as *mut i32).cast(),
            &mut size,
        )
    };
    if result != 0 || pid <= 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(pid as u32)
}

#[cfg(target_os = "macos")]
#[link(name = "System")]
unsafe extern "C" {
    fn getsockopt(
        socket: i32,
        level: i32,
        option: i32,
        value: *mut std::ffi::c_void,
        size: *mut u32,
    ) -> i32;
}

#[cfg(unix)]
unsafe fn native_effective_uid() -> u32 {
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn geteuid() -> u32;
        }
        geteuid()
    }
    #[cfg(not(target_os = "macos"))]
    {
        extern "C" {
            fn geteuid() -> u32;
        }
        geteuid()
    }
}

struct NativeHostClient {
    app: AppHandle,
    connection: NativeHostConnection,
    client_process: ProcessProvenance,
    allowed_view_origins: HashSet<String>,
    outbound: mpsc::Sender<Frame>,
    outbound_rx: Mutex<Option<mpsc::Receiver<Frame>>>,
    shutdown: watch::Sender<bool>,
    retired: AtomicBool,
    view: Mutex<Option<NativeViewBinding>>,
    root_generation: Mutex<Option<super::ClientViewGeneration>>,
    diagnostics: super::webview_diagnostics::DiagnosticsSession,
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
        let status = self.diagnostics.inspect_view(generation, url).await;
        if self.retired.load(Ordering::Acquire)
            || *self.root_generation.lock() != Some(generation)
            || self.view.lock().as_ref().map(|view| view.session_id) != Some(view.session_id)
        {
            return;
        }
        self.diagnostics.adopt_status(status.clone());
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
        if self.retired.load(Ordering::Acquire) {
            return None;
        }
        let view = self.view.lock().clone()?;
        self.diagnostics
            .snapshot(&self.connection, view.session_id, &self.client_process)
    }

    fn try_send(&self, frame: Frame) -> Result<(), NativeHostRejection> {
        if self.retired.load(Ordering::Acquire) {
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
        let observation = match &request.operation {
            NativeHostOperation::ObserveNativeUpdate { operation_id, .. } => {
                Some(!operation_id.value().is_nil())
            }
            _ => None,
        };
        if let Some(valid) = observation {
            let completion = if valid {
                NativeHostCompletion::Completed(NativeHostResult::NativeUpdateObservation(
                    NativeUpdateObservationOutcome::NoNewEvidence,
                ))
            } else {
                NativeHostCompletion::Rejected(NativeHostRejection::ProtocolMismatch)
            };
            return Some(self.reply(request, completion));
        }
        if matches!(&request.operation, NativeHostOperation::NativeUpdate { .. }) {
            return Some(self.reply(
                request,
                NativeHostCompletion::Rejected(NativeHostRejection::Unsupported),
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
            | NativeHostOperation::DebianPreflight
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
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
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
    fn component_manifest_rejects_absolute_and_parent_server_paths() {
        assert!(!valid_server_relative_path(Path::new("/server")));
        assert!(!valid_server_relative_path(Path::new("../server")));
        assert!(valid_server_relative_path(Path::new("bin/domios-server")));
    }

    #[test]
    fn digest_format_requires_exact_lowercase_sha256() {
        assert!(valid_digest(&"a".repeat(64)));
        assert!(!valid_digest(&"A".repeat(64)));
        assert!(!valid_digest(&"a".repeat(63)));
    }
}
