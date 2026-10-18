use codeterm_types::{
    native_host::{
        NativeHostChallenge, NativeHostCompletion, NativeHostEnrollment,
        NativeHostEnrollmentRequest, NativeHostOperation, NativeHostRejection,
        NativeHostResult, NativeHostScope, NativeHostUncertainty,
        NATIVE_HOST_PROTOCOL,
    },
    native_update::{
        NativeUpdateExecutor, NativeUpdateRefusal, NativeUpdateRequest, NativeUpdateResult,
        NativeUpdateResultFuture, NativeUpdateState, NativeUpdateUncertainty,
    },
    server_bootstrap::{
        ProcessProvenance, ServerComponentManifest, ServerReady, BOOTSTRAP_PROTOCOL, API_VERSION,
        SERVER_COMPONENT_SCHEMA,
    },
};
use crate::bootstrap::process::PinnedProcess;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::File,
    io::{self, Read},
    path::Path,
    sync::Arc,
};
use parking_lot::Mutex;
use super::transport::{NativeHostEndpoint, Session};

type UpdateCompletion = NativeHostCompletion<NativeHostResult<NativeUpdateResult>>;

#[derive(Default)]
struct NativeUpdateExecution {
    request: Option<NativeUpdateRequest>,
    last: Option<NativeUpdateResult>,
}

#[derive(Clone)]
struct AttachedNativeUpdateExecutor {
    endpoint: NativeHostEndpoint<NativeUpdateRequest, NativeUpdateResult>,
    session: Arc<Session<NativeUpdateRequest, NativeUpdateResult>>,
    connection: codeterm_types::native_host::NativeHostConnection,
    client_process: ProcessProvenance,
    ready: ServerReady,
    manifest: ServerComponentManifest,
    state: Arc<Mutex<NativeUpdateExecution>>,
}

pub(super) fn active_native_update_executor(
    endpoint: &NativeHostEndpoint<NativeUpdateRequest, NativeUpdateResult>,
) -> Result<Arc<dyn NativeUpdateExecutor>, NativeUpdateRefusal> {
    let session = {
        let active = endpoint.active.read();
        Arc::clone(
            active
                .as_ref()
                .filter(|session| {
                    !session.retired.load(std::sync::atomic::Ordering::Acquire)
                })
                .ok_or(NativeUpdateRefusal::NativeHostUnavailable)?,
        )
    };
    if !endpoint_owns_session(endpoint, &session) {
        return Err(NativeUpdateRefusal::NativeHostUnavailable);
    }
    session
        .process
        .ensure_alive()
        .map_err(|_| NativeUpdateRefusal::NativeHostUnavailable)?;
    if !endpoint_owns_session(endpoint, &session) {
        return Err(NativeUpdateRefusal::NativeHostUnavailable);
    }
    Ok(Arc::new(AttachedNativeUpdateExecutor {
        endpoint: endpoint.clone(),
        session: session.clone(),
        connection: session.connection.clone(),
        client_process: session.client_process.clone(),
        ready: endpoint.ready.clone(),
        manifest: endpoint.manifest.clone(),
        state: Arc::new(Mutex::new(NativeUpdateExecution::default())),
    }))
}

fn endpoint_owns_session(
    endpoint: &NativeHostEndpoint<NativeUpdateRequest, NativeUpdateResult>,
    session: &Arc<Session<NativeUpdateRequest, NativeUpdateResult>>,
) -> bool {
    !session.retired.load(std::sync::atomic::Ordering::Acquire)
        && endpoint
            .active_generation
            .load(std::sync::atomic::Ordering::Acquire)
            == session.connection.generation
        && endpoint
            .active
            .read()
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(active, session))
}

impl NativeUpdateExecutor for AttachedNativeUpdateExecutor {
    fn dispatch(&self, request: NativeUpdateRequest) -> NativeUpdateResultFuture {
        let executor = self.clone();
        Box::pin(async move { executor.dispatch_once(request).await })
    }

    fn observe(
        &self,
        operation_id: codeterm_types::native_update::NativeUpdateOperationId,
        after_sequence: u64,
    ) -> NativeUpdateResultFuture {
        let executor = self.clone();
        Box::pin(async move { executor.observe_only(operation_id, after_sequence).await })
    }
}

impl AttachedNativeUpdateExecutor {
    async fn dispatch_once(&self, request: NativeUpdateRequest) -> NativeUpdateResult {
        if let Err(reason) = request.validate() {
            return self.local_result(&request, NativeUpdateState::Rejected { reason });
        }
        if !request.matches_current_server(&self.ready, &self.manifest)
            || self.connection != self.session.connection
            || self.session.client_process != self.client_process
        {
            return self.local_result(
                &request,
                NativeUpdateState::Rejected {
                    reason: NativeUpdateRefusal::ExpectedServerMismatch,
                },
            );
        }
        let already_started = {
            let mut state = self.state.lock();
            if state.request.is_some() {
                true
            } else {
                state.request = Some(request.clone());
                false
            }
        };
        if already_started {
            return self.local_result(
                &request,
                NativeUpdateState::Rejected {
                    reason: NativeUpdateRefusal::AlreadyInFlight,
                },
            );
        }
        if !self.is_current() {
            return self.local_result(
                &request,
                NativeUpdateState::Rejected {
                    reason: NativeUpdateRefusal::NativeHostUnavailable,
                },
            );
        }
        let completion = self
            .endpoint
            .request_captured(
                &self.session,
                NativeHostScope::Client,
                NativeHostOperation::NativeUpdate {
                    request: request.clone(),
                },
            )
            .await;
        self.complete(request, completion, None, None, false)
    }

    async fn observe_only(
        &self,
        operation_id: codeterm_types::native_update::NativeUpdateOperationId,
        after_sequence: u64,
    ) -> NativeUpdateResult {
        let (request, previous) = {
            let state = self.state.lock();
            (state.request.clone(), state.last.clone())
        };
        let Some(request) = request.filter(|request| request.operation_id == operation_id) else {
            return self.local_result_for_id(
                operation_id,
                NativeUpdateState::Rejected {
                    reason: NativeUpdateRefusal::WrongOperation,
                },
            );
        };
        if previous.is_none() {
            return self.local_result(
                &request,
                NativeUpdateState::Rejected {
                    reason: NativeUpdateRefusal::NativeHostUnavailable,
                },
            );
        }
        if !self.is_current() {
            return self.local_result(
                &request,
                NativeUpdateState::Uncertain {
                    reason: NativeUpdateUncertainty::DisconnectedAfterDispatch,
                },
            );
        }
        let completion = self
            .endpoint
            .request_captured(
                &self.session,
                NativeHostScope::Client,
                NativeHostOperation::ObserveNativeUpdate {
                    operation_id,
                    after_sequence,
                },
            )
            .await;
        self.complete(
            request,
            completion,
            previous.as_ref(),
            Some(after_sequence),
            true,
        )
    }

    fn is_current(&self) -> bool {
        !self.session.retired.load(std::sync::atomic::Ordering::Acquire)
            && self
                .endpoint
                .active_generation
                .load(std::sync::atomic::Ordering::Acquire)
                == self.connection.generation
            && self.session.process.ensure_alive().is_ok()
            && self.session.client_process == self.client_process
            && self.endpoint.active.read().as_ref().is_some_and(|active| {
                Arc::ptr_eq(active, &self.session)
                    && active.connection == self.connection
                    && active.client_process == self.client_process
            })
    }

    fn complete(
        &self,
        request: NativeUpdateRequest,
        completion: UpdateCompletion,
        previous: Option<&NativeUpdateResult>,
        after_sequence: Option<u64>,
        observing: bool,
    ) -> NativeUpdateResult {
        let state = match completion {
            NativeHostCompletion::Completed(NativeHostResult::NativeUpdate(result)) => {
                let validation = if result.connection != self.connection
                    || after_sequence.is_some_and(|after| result.sequence <= after)
                {
                    Err(NativeUpdateRefusal::ExpectedServerMismatch)
                } else {
                    result.validate_after(&request, previous)
                };
                match validation {
                    Ok(()) => return self.record(result),
                    Err(_) => NativeUpdateState::InProgress {
                        stage: codeterm_types::native_update::NativeUpdateStage::Applying,
                    },
                }
            }
            NativeHostCompletion::Completed(_) => NativeUpdateState::InProgress {
                stage: codeterm_types::native_update::NativeUpdateStage::Applying,
            },
            NativeHostCompletion::Rejected(NativeHostRejection::NotAttached) if observing => {
                NativeUpdateState::Uncertain {
                    reason: NativeUpdateUncertainty::DisconnectedAfterDispatch,
                }
            }
            NativeHostCompletion::Rejected(_) if observing => NativeUpdateState::InProgress {
                stage: codeterm_types::native_update::NativeUpdateStage::Applying,
            },
            NativeHostCompletion::Rejected(_) => NativeUpdateState::Rejected {
                reason: NativeUpdateRefusal::NativeHostUnavailable,
            },
            NativeHostCompletion::Uncertain(reason) => NativeUpdateState::Uncertain {
                reason: match reason {
                    NativeHostUncertainty::CancelledAfterDispatch => {
                        NativeUpdateUncertainty::CancelledAfterDispatch
                    }
                    NativeHostUncertainty::DeadlineAfterDispatch => {
                        NativeUpdateUncertainty::DeadlineAfterDispatch
                    }
                    NativeHostUncertainty::DisconnectedAfterDispatch => {
                        NativeUpdateUncertainty::DisconnectedAfterDispatch
                    }
                },
            },
        };
        self.local_result(&request, state)
    }

    fn record(&self, result: NativeUpdateResult) -> NativeUpdateResult {
        self.state.lock().last = Some(result.clone());
        result
    }

    fn local_result(
        &self,
        request: &NativeUpdateRequest,
        state: NativeUpdateState,
    ) -> NativeUpdateResult {
        self.local_result_for_id(request.operation_id, state)
    }

    fn local_result_for_id(
        &self,
        operation_id: codeterm_types::native_update::NativeUpdateOperationId,
        state: NativeUpdateState,
    ) -> NativeUpdateResult {
        let mut execution = self.state.lock();
        let sequence = execution
            .last
            .as_ref()
            .map_or(1, |last| last.sequence.saturating_add(1));
        let result = NativeUpdateResult {
            operation_id,
            sequence,
            connection: self.connection.clone(),
            state,
        };
        execution.last = Some(result.clone());
        result
    }
}

pub(crate) struct EnrollmentPolicy {
    pub challenge: NativeHostChallenge,
    pub client_sha256: String,
    pub allowed_view_origins: HashSet<String>,
}

pub(crate) struct VerifiedClientProcess(ProcessProvenance);

pub(crate) fn verify_component(
    manifest: &ServerComponentManifest,
    ready: &ServerReady,
) -> io::Result<()> {
    if manifest.schema != SERVER_COMPONENT_SCHEMA
        || manifest.api_version != API_VERSION
        || manifest.bootstrap_protocol != BOOTSTRAP_PROTOCOL
        || manifest.server_build != ready.build
        || ready.api_version != manifest.api_version
        || ready.protocol != manifest.bootstrap_protocol
        || manifest.server_executable.canonicalize()? != ready.process.executable
        || !valid_digest(&manifest.server_sha256)
        || !valid_digest(&manifest.client_sha256)
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Native host component record does not match the running server",
        ));
    }
    let server_digest = sha256_file(&manifest.server_executable)?;
    if server_digest != manifest.server_sha256 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Installed server image differs from its component record",
        ));
    }
    Ok(())
}

pub(crate) fn verify_client_process(
    process: &PinnedProcess,
    policy: &EnrollmentPolicy,
) -> io::Result<VerifiedClientProcess> {
    if !valid_digest(&policy.client_sha256) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Client artifact digest is invalid",
        ));
    }
    process.ensure_alive()?;
    let provenance = process.provenance()?;
    let image = process_image_path(&provenance);
    let digest = sha256_file(&image)?;
    if digest != policy.client_sha256 || process.provenance()? != provenance {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Native host process does not match the installed client artifact",
        ));
    }
    process.ensure_alive()?;
    Ok(VerifiedClientProcess(provenance))
}

pub(crate) fn enroll(
    policy: &EnrollmentPolicy,
    request: NativeHostEnrollmentRequest,
    client_process: VerifiedClientProcess,
) -> Result<NativeHostEnrollment, NativeHostRejection> {
    let connection = &policy.challenge.connection;
    if policy.challenge.protocol != NATIVE_HOST_PROTOCOL
        || request.protocol != NATIVE_HOST_PROTOCOL
    {
        return Err(NativeHostRejection::ProtocolMismatch);
    }
    if request.challenge != policy.challenge.challenge || request.connection != *connection {
        return Err(NativeHostRejection::Unauthorized);
    }
    if request.view.as_ref().is_some_and(|view| {
        !view.is_well_formed() || !policy.allowed_view_origins.contains(&view.origin)
    }) {
        return Err(NativeHostRejection::InvalidView);
    }
    Ok(NativeHostEnrollment {
        protocol: NATIVE_HOST_PROTOCOL,
        challenge: policy.challenge.challenge.clone(),
        connection: connection.clone(),
        client_process: client_process.0,
        view: request.view,
    })
}

fn valid_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn process_image_path(provenance: &ProcessProvenance) -> std::path::PathBuf {
    #[cfg(target_os = "linux")]
    {
        std::path::PathBuf::from(format!("/proc/{}/exe", provenance.pid))
    }
    #[cfg(not(target_os = "linux"))]
    {
        provenance.executable.clone()
    }
}
