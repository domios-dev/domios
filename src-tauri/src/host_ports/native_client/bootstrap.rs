use super::{
    ExactServerShutdownRefusal, ExactServerShutdownResult, ExactServerShutdownUncertainty,
    NativeHostClientSlot,
};
use codeterm_process_identity::PinnedProcess;
use codeterm_types::{
    native_host::MAX_NATIVE_HOST_FRAME_BYTES,
    server_bootstrap::{
        AttachReply, AttachRequest, BootstrapOperation, LaunchOptions, ProcessProvenance,
        ServerComponentManifest, ServerExpectation, ServerIdentity, ServerReady, API_VERSION,
        BOOTSTRAP_PROTOCOL, SERVER_COMPONENT_SCHEMA,
    },
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt},
    process::Command,
    time::{timeout_at, Instant as TokioInstant},
};

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ClientStartupStage {
    ResourceRoot,
    Inventory,
    Install,
    ResourceVerification,
    ServerArtifact,
    ClientArtifact,
    IdentityResolution,
    Attach,
    Launch,
    ReplyValidation,
    ApiAttachment,
    NativeEnrollment,
    Frontend,
    Startup,
}

#[cfg(any(debug_assertions, test))]
pub(super) fn diagnostic_payload(
    stage: ClientStartupStage,
    status: &'static str,
    error: Option<&io::Error>,
    exit_code: Option<i32>,
) -> serde_json::Value {
    serde_json::json!({ "stage": stage, "status": status,
        "io_kind": error.map(|error| format!("{:?}", error.kind())),
        "os_error": error.and_then(io::Error::raw_os_error), "exit_code": exit_code })
}

fn record_startup(
    stage: ClientStartupStage,
    status: &'static str,
    error: Option<&io::Error>,
    exit_code: Option<i32>,
    terminal: bool,
) {
    record_trail(stage, status, error.map(io::Error::to_string), exit_code);
    #[cfg(debug_assertions)]
    {
        use std::io::Write;
        use std::sync::atomic::AtomicUsize;
        static RECORDS: AtomicUsize = AtomicUsize::new(0);
        if RECORDS
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                (count < 64).then_some(count + 1)
            })
            .is_err()
        {
            return;
        }
        let prefix = if terminal {
            "ERROR: DOMIOS_DEV_FAILURE"
        } else {
            "DOMIOS_CLIENT_DIAGNOSTIC"
        };
        let _ = writeln!(
            std::io::stderr().lock(),
            "{prefix} {}",
            diagnostic_payload(stage, status, error, exit_code)
        );
    }
    #[cfg(not(debug_assertions))]
    let _ = (stage, status, error, exit_code, terminal);
}

fn record_trail(
    stage: ClientStartupStage,
    status: &str,
    error: Option<String>,
    exit_code: Option<i32>,
) {
    let stage = serde_json::to_value(stage)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default();
    super::startup_log::record(&stage, status, error.as_deref(), exit_code);
}

pub(crate) fn startup_started(stage: ClientStartupStage) {
    record_startup(stage, "started", None, None, false);
}
/// The stage completed; the interface is on screen once `Frontend` is reached.
pub(crate) fn startup_reached(stage: ClientStartupStage) {
    record_startup(stage, "reached", None, None, false);
}
pub(crate) fn startup_refused(stage: ClientStartupStage) {
    record_startup(stage, "refused", None, None, true);
}
pub(crate) fn startup_io_error(stage: ClientStartupStage, error: &io::Error) {
    record_startup(stage, "io_error", Some(error), None, false);
}

fn helper_diagnostic(stage: ClientStartupStage, error: &HelperFailure) {
    let (status, code) = match error {
        HelperFailure::PreDispatch(_) => ("predispatch_refusal", None),
        HelperFailure::Deadline { .. } => ("deadline", None),
        HelperFailure::Exit { code, .. } => ("helper_exit", *code),
        HelperFailure::Io { .. } => ("io_error", None),
    };
    record_trail(stage, "helper_failure", Some(error.to_string()), code);
    record_startup(stage, status, None, code, false);
}

pub(super) const BOOTSTRAP_DEADLINE: Duration = Duration::from_secs(30);
const HELPER_STDERR_BYTES: u64 = 16 * 1024;

#[cfg(test)]
pub(in crate::host_ports) async fn read_single_bounded_line<R>(
    reader: &mut R,
    limit: usize,
) -> io::Result<Vec<u8>>
where
    R: AsyncBufRead + Unpin,
{
    read_single_bounded_line_observed(reader, limit, || {}).await
}

async fn read_single_bounded_line_observed<R>(
    reader: &mut R,
    limit: usize,
    newline_received: impl FnOnce(),
) -> io::Result<Vec<u8>>
where
    R: AsyncBufRead + Unpin,
{
    if limit == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Frame limit must be nonzero",
        ));
    }
    let mut frame = Vec::with_capacity(limit.min(4096));
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Frame ended before its newline",
            ));
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let count = newline.map_or(available.len(), |index| index + 1);
        if frame
            .len()
            .checked_add(count)
            .is_none_or(|length| length > limit)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Frame exceeds its byte limit",
            ));
        }
        frame.extend_from_slice(&available[..count]);
        reader.consume(count);
        if newline.is_some() {
            break;
        }
    }
    newline_received();
    let mut trailing = [0; 1];
    if reader.read(&mut trailing).await? != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Frame contains trailing bytes",
        ));
    }
    Ok(frame)
}
#[derive(Debug)]
pub(in crate::host_ports) enum HelperFailure {
    PreDispatch(ExactServerShutdownRefusal),
    Deadline {
        request_forwarded: bool,
    },
    Exit {
        code: Option<i32>,
        request_forwarded: bool,
        detail: Option<String>,
    },
    Io {
        message: String,
        request_forwarded: bool,
    },
}

impl HelperFailure {
    pub(in crate::host_ports) fn request_forwarded(&self) -> bool {
        match self {
            Self::PreDispatch(_) => false,
            Self::Deadline { request_forwarded }
            | Self::Exit {
                request_forwarded, ..
            }
            | Self::Io {
                request_forwarded, ..
            } => *request_forwarded,
        }
    }

    pub(crate) fn allows_initial_server_launch(&self) -> bool {
        matches!(
            self,
            Self::Exit {
                code: Some(
                    codeterm_types::server_bootstrap::BOOTSTRAP_ATTACH_UNAVAILABLE_EXIT_CODE
                ),
                request_forwarded: true,
                ..
            }
        )
    }
}

/// Why the client could not attach to a server, kept typed so the window can tell a stated refusal apart.
#[derive(Debug)]
pub(crate) struct ClientStartupFailure {
    message: String,
    stated_refusal: bool,
}

impl ClientStartupFailure {
    /// A server that refused its launch with a stated reason repeats it on every unchanged retry.
    pub(in crate::host_ports) fn launch(error: &HelperFailure) -> Self {
        Self {
            message: error.to_string(),
            stated_refusal: matches!(
                error,
                HelperFailure::Exit {
                    detail: Some(_),
                    ..
                }
            ),
        }
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    pub(crate) fn stated_refusal(&self) -> bool {
        self.stated_refusal
    }
}

impl From<String> for ClientStartupFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            stated_refusal: false,
        }
    }
}

impl From<&str> for ClientStartupFailure {
    fn from(message: &str) -> Self {
        message.to_string().into()
    }
}

impl std::fmt::Display for ClientStartupFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::fmt::Display for HelperFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PreDispatch(reason) => write!(
                formatter,
                "Server helper was refused before dispatch: {reason:?}"
            ),
            Self::Deadline {
                request_forwarded: false,
            } => formatter.write_str("Server helper deadline elapsed before dispatch"),
            Self::Deadline {
                request_forwarded: true,
            } => formatter.write_str("Server helper deadline elapsed after dispatch"),
            Self::Exit { code, detail, .. } => {
                write!(formatter, "Server helper exited with status {code:?}")?;
                match detail {
                    Some(detail) => write!(formatter, ": {detail}"),
                    None => Ok(()),
                }
            }
            Self::Io { message, .. } => formatter.write_str(message),
        }
    }
}

pub(super) struct VerifiedClientInstall {
    pub(super) manifest: ServerComponentManifest,
    pub(super) resource_root: PathBuf,
    pub(super) server_executable: PathBuf,
    pub(super) client_process: ProcessProvenance,
    pub(super) identity: ServerIdentity,
    pub(super) launch_options: LaunchOptions,
}

pub(super) fn verify_install(
    manifest: ServerComponentManifest,
    trusted_resource_root: PathBuf,
    launch_options: LaunchOptions,
) -> io::Result<VerifiedClientInstall> {
    startup_started(ClientStartupStage::Install);
    if manifest.schema != SERVER_COMPONENT_SCHEMA
        || manifest.api_version != API_VERSION
        || manifest.bootstrap_protocol != BOOTSTRAP_PROTOCOL
        || manifest.server_build.trim().is_empty()
        || !valid_digest(&manifest.server_sha256)
        || codeterm_process_identity::client_identity::validate_client_identity(
            &manifest.client_identity,
        )
        .is_err()
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

    startup_started(ClientStartupStage::ResourceVerification);
    let resource_root = trusted_resource_root.canonicalize()?;
    if !resource_root.is_dir() || launch_options.resources.canonicalize()? != resource_root {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Server resources differ from the verified component root",
        ));
    }
    let server_executable = codeterm_process_identity::client_identity::resolve_server_executable(
        &resource_root,
        &manifest.server_executable,
        &manifest.client_identity,
    )?;
    startup_started(ClientStartupStage::ServerArtifact);
    if file_sha256(&server_executable)? != manifest.server_sha256 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Server executable digest does not match the installed manifest",
        ));
    }

    startup_started(ClientStartupStage::ClientArtifact);
    let client_executable = std::env::current_exe()?.canonicalize()?;
    #[cfg(not(target_os = "macos"))]
    let identity_matches = match &manifest.client_identity {
        codeterm_types::server_bootstrap::ClientIdentity::Sha256 { sha256 } => {
            file_sha256(&client_executable)? == *sha256
        }
        _ => false,
    };
    #[cfg(target_os = "macos")]
    let identity_matches = match &manifest.client_identity {
        codeterm_types::server_bootstrap::ClientIdentity::MacosDesignatedRequirement {
            requirement,
            team_id,
            identifier,
        } => {
            let process = codeterm_process_identity::PinnedProcess::open(std::process::id())?;
            codeterm_process_identity::code_signature_macos::verify_macos_client(
                &process,
                &resource_root,
                requirement,
                team_id,
                identifier,
            )?;
            true
        }
        _ => false,
    };
    if client_executable == server_executable || !identity_matches {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Client executable differs from the installed component manifest",
        ));
    }

    startup_started(ClientStartupStage::IdentityResolution);
    let paths = codeterm_paths::instance_identity::ResolvedInstancePaths::current()?;
    Ok(VerifiedClientInstall {
        manifest,
        resource_root,
        server_executable,
        client_process: PinnedProcess::open(std::process::id())?.provenance()?,
        identity: ServerIdentity {
            data: paths.data().to_path_buf(),
            config: paths.config().to_path_buf(),
        },
        launch_options,
    })
}

pub(super) fn supports_shutdown_completion_protocol(protocol: u32) -> bool {
    protocol == BOOTSTRAP_PROTOCOL
}

pub(super) fn valid_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
pub(super) fn valid_server_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
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

pub(super) fn verify_shutdown_artifact(
    manifest: &ServerComponentManifest,
    ready: &ServerReady,
    trusted_resource_root: &Path,
    server_executable: &Path,
) -> Result<(), ArtifactVerificationFailure> {
    let resource_root = trusted_resource_root
        .canonicalize()
        .map_err(|_| ArtifactVerificationFailure::Io)?;
    let ready_root = ready
        .resources
        .canonicalize()
        .map_err(|_| ArtifactVerificationFailure::Io)?;
    if resource_root != ready_root
        || !resource_root.is_dir()
        || manifest.schema != SERVER_COMPONENT_SCHEMA
        || manifest.api_version != API_VERSION
        || manifest.bootstrap_protocol != BOOTSTRAP_PROTOCOL
        || manifest.server_build != ready.build
        || manifest.api_version != ready.api_version
        || manifest.bootstrap_protocol != ready.protocol
    {
        return Err(ArtifactVerificationFailure::Invalid);
    }
    let installed = codeterm_process_identity::client_identity::resolve_server_executable(
        &resource_root,
        &manifest.server_executable,
        &manifest.client_identity,
    )
    .map_err(|_| ArtifactVerificationFailure::Invalid)?;
    let helper = server_executable
        .canonicalize()
        .map_err(|_| ArtifactVerificationFailure::Io)?;
    let process_image = ready
        .process
        .executable
        .canonicalize()
        .map_err(|_| ArtifactVerificationFailure::Io)?;
    if installed != helper || installed != process_image {
        return Err(ArtifactVerificationFailure::Invalid);
    }
    if file_sha256(&installed).map_err(|_| ArtifactVerificationFailure::Io)?
        != manifest.server_sha256
    {
        return Err(ArtifactVerificationFailure::Invalid);
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub(super) enum ArtifactVerificationFailure {
    Invalid,
    Io,
}

pub(super) async fn attach_or_launch(
    install: &VerifiedClientInstall,
    allow_launch: bool,
) -> Result<AttachReply, ClientStartupFailure> {
    let deadline = TokioInstant::now() + BOOTSTRAP_DEADLINE;
    let inspect = attach_request(install);
    startup_started(ClientStartupStage::Attach);
    match invoke_helper_until(
        &install.server_executable,
        "attach",
        &inspect,
        deadline,
        || Ok(()),
    )
    .await
    {
        Ok(line) => validate_attach_reply(
            install,
            &inspect,
            parse_line(&line).map_err(|error| {
                startup_io_error(ClientStartupStage::ReplyValidation, &error);
                error.to_string()
            })?,
            true,
        )
        .map_err(|error| error.to_string().into()),
        Err(error) if allow_launch && error.allows_initial_server_launch() => {
            helper_diagnostic(ClientStartupStage::Attach, &error);
            startup_started(ClientStartupStage::Launch);
            let trail_pause = super::startup_log::pause_for_server_launch();
            let mut launch_options = install.launch_options.clone();
            if matches!(
                launch_options.instance,
                codeterm_types::server_bootstrap::InstanceSelector::Development { .. }
            ) {
                launch_options.development_owner = Some(
                    PinnedProcess::open(std::process::id())
                        .and_then(|process| process.provenance())
                        .map_err(|error| ClientStartupFailure::from(error.to_string()))?,
                );
            }
            let launch_result = invoke_helper_until(
                &install.server_executable,
                "launch",
                &launch_options,
                deadline,
                || Ok(()),
            )
            .await;
            drop(trail_pause);
            match launch_result {
                Ok(line) => validate_attach_reply(
                    install,
                    &inspect,
                    parse_line(&line).map_err(|error| {
                        startup_io_error(ClientStartupStage::ReplyValidation, &error);
                        error.to_string()
                    })?,
                    false,
                )
                .map_err(|error| error.to_string().into()),
                Err(error) => {
                    helper_diagnostic(ClientStartupStage::Launch, &error);
                    Err(ClientStartupFailure::launch(&error))
                }
            }
        }
        Err(error) => {
            helper_diagnostic(ClientStartupStage::Attach, &error);
            Err(error.to_string().into())
        }
    }
}

pub(super) fn attach_request(install: &VerifiedClientInstall) -> AttachRequest {
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
    startup_started(ClientStartupStage::ReplyValidation);
    validate_ready(install, &reply.ready).inspect_err(|error| {
        startup_io_error(ClientStartupStage::ReplyValidation, error);
    })?;
    Ok(reply)
}

pub(super) fn validate_ready(
    install: &VerifiedClientInstall,
    ready: &ServerReady,
) -> io::Result<()> {
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
        || ready.loopback_api_address().is_none()
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

pub(super) async fn invoke_helper_until<T, F>(
    executable: &Path,
    command_name: &str,
    request: &T,
    deadline: TokioInstant,
    before_dispatch: F,
) -> Result<String, HelperFailure>
where
    T: Serialize,
    F: FnOnce() -> Result<(), HelperFailure> + Send,
{
    let stage = if command_name == "launch" {
        ClientStartupStage::Launch
    } else {
        ClientStartupStage::Attach
    };
    let bytes = serde_json::to_vec(request).map_err(|error| HelperFailure::Io {
        message: error.to_string(),
        request_forwarded: false,
    })?;
    if bytes.is_empty() || bytes.len() >= MAX_NATIVE_HOST_FRAME_BYTES {
        return Err(HelperFailure::Io {
            message: "Server helper request exceeds its bound".into(),
            request_forwarded: false,
        });
    }
    let mut child_command = Command::new(executable);
    child_command
        .arg(command_name)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
        child_command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = child_command.spawn().map_err(|error| {
        startup_io_error(stage, &error);
        HelperFailure::Io {
            message: format!("Server helper could not start: {error}"),
            request_forwarded: false,
        }
    })?;
    let Some(mut input) = child.stdin.take() else {
        stop_and_reap(&mut child, deadline).await;
        return Err(HelperFailure::Io {
            message: "Server helper input pipe is unavailable".into(),
            request_forwarded: false,
        });
    };
    let Some(stdout) = child.stdout.take() else {
        stop_and_reap(&mut child, deadline).await;
        return Err(HelperFailure::Io {
            message: "Server helper output pipe is unavailable".into(),
            request_forwarded: false,
        });
    };
    let stderr = child.stderr.take().map(|stderr| {
        tokio::spawn(async move {
            let mut bytes = Vec::new();
            let _ = stderr
                .take(HELPER_STDERR_BYTES)
                .read_to_end(&mut bytes)
                .await;
            codeterm_types::server_bootstrap::last_diagnostic_line(&bytes)
        })
    });
    let request_forwarded = Arc::new(AtomicBool::new(false));
    let forwarded = request_forwarded.clone();
    let result = timeout_at(deadline, async {
        before_dispatch()?;
        forwarded.store(true, Ordering::Release);
        let stream_result = async {
            input.write_all(&bytes).await?;
            input.write_all(b"\n").await?;
            input.flush().await
        }
        .await;
        if let Err(error) = stream_result {
            startup_io_error(stage, &error);
            return Err(HelperFailure::Io {
                message: error.to_string(),
                request_forwarded: forwarded.load(Ordering::Acquire),
            });
        }
        drop(input);
        let mut reader = tokio::io::BufReader::new(stdout);
        let (line, status) = tokio::join!(
            async {
                let line = read_single_bounded_line_observed(
                    &mut reader,
                    MAX_NATIVE_HOST_FRAME_BYTES,
                    || record_startup(stage, "response_newline", None, None, false),
                )
                .await;
                if line.is_ok() {
                    record_startup(stage, "response_eof", None, None, false);
                }
                line
            },
            async {
                let status = child.wait().await;
                if let Ok(status) = &status {
                    record_startup(stage, "helper_exited", None, status.code(), false);
                }
                status
            }
        );
        let status = status.map_err(|error| {
            startup_io_error(stage, &error);
            HelperFailure::Io {
                message: error.to_string(),
                request_forwarded: forwarded.load(Ordering::Acquire),
            }
        })?;
        if !status.success() {
            let detail = match stderr {
                Some(task) => task.await.ok().flatten(),
                None => None,
            };
            return Err(HelperFailure::Exit {
                code: status.code(),
                request_forwarded: forwarded.load(Ordering::Acquire),
                detail,
            });
        }
        line.map_err(|error| {
            startup_io_error(stage, &error);
            HelperFailure::Io {
                message: error.to_string(),
                request_forwarded: forwarded.load(Ordering::Acquire),
            }
        })
    })
    .await;
    let line = match result {
        Ok(Ok(line)) => line,
        Ok(Err(error)) => {
            stop_and_reap(&mut child, deadline).await;
            return Err(error);
        }
        Err(_) => {
            stop_and_reap(&mut child, deadline).await;
            return Err(HelperFailure::Deadline {
                request_forwarded: request_forwarded.load(Ordering::Acquire),
            });
        }
    };
    if line.is_empty() || line.len() > MAX_NATIVE_HOST_FRAME_BYTES {
        stop_and_reap(&mut child, deadline).await;
        return Err(HelperFailure::Io {
            message: "Server helper returned no bounded readiness frame".into(),
            request_forwarded: true,
        });
    }
    String::from_utf8(line).map_err(|error| HelperFailure::Io {
        message: error.to_string(),
        request_forwarded: true,
    })
}

async fn verify_shutdown_artifact_until(
    manifest: ServerComponentManifest,
    ready: ServerReady,
    trusted_resource_root: PathBuf,
    server_executable: PathBuf,
    process: Arc<PinnedProcess>,
    deadline: TokioInstant,
) -> Result<(), ExactServerShutdownRefusal> {
    let verification = tokio::task::spawn_blocking(move || {
        if process.ensure_alive().is_err() {
            return Err(ExactServerShutdownRefusal::StaleIncarnation);
        }
        let provenance = process
            .provenance()
            .map_err(|_| ExactServerShutdownRefusal::ProcessInspectionFailed)?;
        if provenance != ready.process {
            return Err(ExactServerShutdownRefusal::StaleIncarnation);
        }
        verify_shutdown_artifact(
            &manifest,
            &ready,
            &trusted_resource_root,
            &server_executable,
        )
        .map_err(|failure| match failure {
            ArtifactVerificationFailure::Invalid => {
                ExactServerShutdownRefusal::HelperArtifactInvalid
            }
            ArtifactVerificationFailure::Io => ExactServerShutdownRefusal::VerificationIoFailure,
        })
    });
    match timeout_at(deadline, verification).await {
        Err(_) => Err(ExactServerShutdownRefusal::VerificationDeadline),
        Ok(Err(_)) => Err(ExactServerShutdownRefusal::VerificationInterrupted),
        Ok(Ok(result)) => result,
    }
}

async fn stop_and_reap(child: &mut tokio::process::Child, deadline: TokioInstant) {
    let _ = child.start_kill();
    let _ = timeout_at(deadline, child.wait()).await;
}

pub(super) fn parse_line<T: serde::de::DeserializeOwned>(line: &str) -> io::Result<T> {
    serde_json::from_str(line.trim_end()).map_err(io::Error::other)
}

impl super::NativeHostClient {
    pub(super) async fn request_exact_server_shutdown(
        self: &Arc<Self>,
        expected: ServerReady,
        api: &super::super::ServerApiAttachmentSlot,
        slot: &NativeHostClientSlot,
    ) -> ExactServerShutdownResult {
        if !supports_shutdown_completion_protocol(expected.protocol) {
            return ExactServerShutdownResult::RefusedBeforeDispatch(
                ExactServerShutdownRefusal::IncompatibleServerProtocol,
            );
        }
        if self.retired.load(Ordering::Acquire)
            || !slot.is_current(self)
            || self.ready != expected
            || self.connection.generation == 0
            || self.connection.identity != expected.identity
            || self.connection.server_epoch != expected.epoch
        {
            return ExactServerShutdownResult::RefusedBeforeDispatch(
                ExactServerShutdownRefusal::StaleIncarnation,
            );
        }

        let deadline = TokioInstant::now() + BOOTSTRAP_DEADLINE;
        if let Err(reason) = verify_shutdown_artifact_until(
            self.manifest.clone(),
            expected.clone(),
            self.resource_root.clone(),
            self.server_executable.clone(),
            self.server_process.clone(),
            deadline,
        )
        .await
        {
            return ExactServerShutdownResult::RefusedBeforeDispatch(reason);
        }

        let request = AttachRequest {
            protocol: BOOTSTRAP_PROTOCOL,
            api_version: expected.api_version,
            build: expected.build.clone(),
            identity: expected.identity.clone(),
            challenge: uuid::Uuid::new_v4().to_string(),
            operation: BootstrapOperation::Shutdown,
            expected: Some(ServerExpectation {
                process: expected.process.clone(),
                epoch: expected.epoch.clone(),
            }),
        };
        let api_ready = expected.clone();
        let helper = invoke_helper_until(
            &self.server_executable,
            "attach",
            &request,
            deadline,
            || {
                if self.retired.load(Ordering::Acquire)
                    || !slot.is_current(self)
                    || api
                        .current()
                        .is_none_or(|attachment| attachment.ready() != &api_ready)
                    || self.server_process.ensure_alive().is_err()
                {
                    Err(HelperFailure::PreDispatch(
                        ExactServerShutdownRefusal::StaleIncarnation,
                    ))
                } else {
                    Ok(())
                }
            },
        )
        .await;
        let line = match helper {
            Err(HelperFailure::PreDispatch(reason)) => {
                return ExactServerShutdownResult::RefusedBeforeDispatch(reason);
            }
            Err(HelperFailure::Deadline {
                request_forwarded: false,
            }) => {
                return ExactServerShutdownResult::RefusedBeforeDispatch(
                    ExactServerShutdownRefusal::HelperDeadlineBeforeDispatch,
                );
            }
            Err(HelperFailure::Deadline {
                request_forwarded: true,
            }) => {
                return ExactServerShutdownResult::UncertainAfterDispatch(
                    ExactServerShutdownUncertainty::HelperDeadlineAfterDispatch,
                );
            }
            Ok(line) => line,
            Err(error) if !error.request_forwarded() => {
                return ExactServerShutdownResult::RefusedBeforeDispatch(
                    ExactServerShutdownRefusal::HelperUnavailable,
                );
            }
            Err(_) => {
                return ExactServerShutdownResult::UncertainAfterDispatch(
                    ExactServerShutdownUncertainty::HelperOutcomeUnknown,
                );
            }
        };
        let reply: AttachReply = match parse_line(&line) {
            Ok(reply) => reply,
            Err(_) => {
                return ExactServerShutdownResult::UncertainAfterDispatch(
                    ExactServerShutdownUncertainty::InvalidReply,
                );
            }
        };
        if !crate::crash::exit::is_exact_server_shutdown_reply(&expected, &request, &reply) {
            return ExactServerShutdownResult::UncertainAfterDispatch(
                ExactServerShutdownUncertainty::InvalidReply,
            );
        }

        let process = self.server_process.clone();
        let remaining = deadline.saturating_duration_since(TokioInstant::now());
        if remaining.is_zero() {
            return ExactServerShutdownResult::UncertainAfterDispatch(
                ExactServerShutdownUncertainty::ServerExitUnobserved,
            );
        }
        match tokio::time::timeout_at(
            deadline,
            tokio::task::spawn_blocking(move || process.wait_exit(remaining)),
        )
        .await
        {
            Ok(Ok(Ok(()))) => ExactServerShutdownResult::Confirmed {
                server: expected,
                client_generation: self.connection.generation,
            },
            _ => ExactServerShutdownResult::UncertainAfterDispatch(
                ExactServerShutdownUncertainty::ServerExitUnobserved,
            ),
        }
    }
}

#[cfg(test)]
mod framing_tests {
    use super::*;

    #[tokio::test]
    async fn newline_observation_never_replaces_exact_eof_validation() {
        let (mut writer, stream) = tokio::io::duplex(64);
        let (observed, newline) = tokio::sync::oneshot::channel();
        let read = tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(stream);
            read_single_bounded_line_observed(&mut reader, 32, || {
                let _ = observed.send(());
            })
            .await
        });
        writer.write_all(b"opaque\n").await.unwrap();
        newline.await.unwrap();
        assert!(!read.is_finished());
        writer.write_all(b"trailing").await.unwrap();
        writer.shutdown().await.unwrap();
        assert_eq!(
            read.await.unwrap().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
