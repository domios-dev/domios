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
    path::{Component, Path, PathBuf},
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

pub(super) const BOOTSTRAP_DEADLINE: Duration = Duration::from_secs(30);

pub(in crate::host_ports) async fn read_single_bounded_line<R>(
    reader: &mut R,
    limit: usize,
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
            }
        )
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
            Self::Exit { code, .. } => {
                write!(formatter, "Server helper exited with status {code:?}")
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

pub(super) fn valid_server_relative_path(path: &Path) -> bool {
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
        || !valid_server_relative_path(&manifest.server_executable)
    {
        return Err(ArtifactVerificationFailure::Invalid);
    }
    let installed = resource_root
        .join(&manifest.server_executable)
        .canonicalize()
        .map_err(|_| ArtifactVerificationFailure::Io)?;
    let helper = server_executable
        .canonicalize()
        .map_err(|_| ArtifactVerificationFailure::Io)?;
    let process_image = ready
        .process
        .executable
        .canonicalize()
        .map_err(|_| ArtifactVerificationFailure::Io)?;
    if !installed.starts_with(&resource_root) || installed != helper || installed != process_image {
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
) -> Result<AttachReply, String> {
    let deadline = TokioInstant::now() + BOOTSTRAP_DEADLINE;
    let inspect = attach_request(install);
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
            parse_line(&line).map_err(|error| error.to_string())?,
            true,
        )
        .map_err(|error| error.to_string()),
        Err(error) if error.allows_initial_server_launch() => {
            match invoke_helper_until(
                &install.server_executable,
                "launch",
                &install.launch_options,
                deadline,
                || Ok(()),
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
                Err(error) => Err(error.to_string()),
            }
        }
        Err(error) => Err(error.to_string()),
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
    validate_ready(install, &reply.ready)?;
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
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
        child_command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = child_command.spawn().map_err(|error| HelperFailure::Io {
        message: format!("Server helper could not start: {error}"),
        request_forwarded: false,
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
            return Err(HelperFailure::Io {
                message: error.to_string(),
                request_forwarded: forwarded.load(Ordering::Acquire),
            });
        }
        drop(input);
        let mut reader = tokio::io::BufReader::new(stdout);
        let line = read_single_bounded_line(&mut reader, MAX_NATIVE_HOST_FRAME_BYTES).await;
        let status = child.wait().await.map_err(|error| HelperFailure::Io {
            message: error.to_string(),
            request_forwarded: forwarded.load(Ordering::Acquire),
        })?;
        if !status.success() {
            return Err(HelperFailure::Exit {
                code: status.code(),
                request_forwarded: forwarded.load(Ordering::Acquire),
            });
        }
        line.map_err(|error| HelperFailure::Io {
            message: error.to_string(),
            request_forwarded: forwarded.load(Ordering::Acquire),
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
