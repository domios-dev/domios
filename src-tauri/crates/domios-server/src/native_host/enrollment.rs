use super::transport::{NativeHostEndpoint, Session};
use crate::bootstrap::process::PinnedProcess;
use codeterm_types::{
    native_host::{
        NativeHostChallenge, NativeHostCompletion, NativeHostConnection, NativeHostEnrollment,
        NativeHostEnrollmentRequest, NativeHostOperation, NativeHostRejection, NativeHostResult,
        NativeHostScope, NativeHostUncertainty, NATIVE_HOST_PROTOCOL,
    },
    native_update::{
        NativeUpdateAttachment, NativeUpdateDispatchFuture, NativeUpdateDispatchOutcome,
        NativeUpdateExecutor, NativeUpdateObservationFuture, NativeUpdateObservationOutcome,
        NativeUpdateRefusal, NativeUpdateRequest, NativeUpdateResult, NativeUpdateUncertainty,
    },
    server_bootstrap::{
        ProcessProvenance, ServerComponentManifest, ServerReady, API_VERSION, BOOTSTRAP_PROTOCOL,
        SERVER_COMPONENT_SCHEMA,
    },
};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::File,
    io::{self, Read},
    path::Path,
    sync::Arc,
};

pub(super) type UpdateCompletion = NativeHostCompletion<NativeHostResult<NativeUpdateResult>>;

#[derive(Default)]
struct NativeUpdateExecution {
    request: Option<NativeUpdateRequest>,
    attachment: Option<NativeUpdateAttachment>,
    last: Option<NativeUpdateResult>,
}

#[derive(Clone)]
struct AttachedNativeUpdateExecutor {
    endpoint: NativeHostEndpoint<NativeUpdateRequest, NativeUpdateResult>,
    session: Arc<Session<NativeUpdateRequest, NativeUpdateResult>>,
    attachment: NativeUpdateAttachment,
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
                .filter(|session| !session.retired.load(std::sync::atomic::Ordering::Acquire))
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
        attachment: NativeUpdateAttachment {
            connection: session.connection.clone(),
            client_process: session.client_process.clone(),
        },
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
    fn captured_attachment(&self) -> Option<NativeUpdateAttachment> {
        self.is_current().then(|| self.attachment.clone())
    }

    fn dispatch(
        &self,
        request: NativeUpdateRequest,
        attachment: NativeUpdateAttachment,
    ) -> NativeUpdateDispatchFuture {
        let executor = self.clone();
        Box::pin(async move { executor.dispatch_once(request, attachment).await })
    }

    fn observe(
        &self,
        operation_id: codeterm_types::native_update::NativeUpdateOperationId,
        after_sequence: u64,
    ) -> NativeUpdateObservationFuture {
        let executor = self.clone();
        Box::pin(async move { executor.observe_only(operation_id, after_sequence).await })
    }
}

impl AttachedNativeUpdateExecutor {
    async fn dispatch_once(
        &self,
        request: NativeUpdateRequest,
        attachment: NativeUpdateAttachment,
    ) -> NativeUpdateDispatchOutcome {
        if let Err(reason) = attachment.validate_for(&request) {
            return NativeUpdateDispatchOutcome::RefusedBeforeExecution { reason };
        }
        if attachment != self.attachment || !self.is_current() {
            return NativeUpdateDispatchOutcome::RefusedBeforeExecution {
                reason: NativeUpdateRefusal::NativeHostUnavailable,
            };
        }
        if let Err(reason) = request.validate() {
            return NativeUpdateDispatchOutcome::RefusedBeforeExecution { reason };
        }
        if !request.matches_current_server(&self.ready, &self.manifest) {
            return NativeUpdateDispatchOutcome::RefusedBeforeExecution {
                reason: NativeUpdateRefusal::ExpectedServerMismatch,
            };
        }
        let already_started = {
            let mut state = self.state.lock();
            if state.request.is_some() {
                true
            } else {
                state.request = Some(request.clone());
                state.attachment = Some(attachment.clone());
                false
            }
        };
        if already_started {
            return NativeUpdateDispatchOutcome::RefusedBeforeExecution {
                reason: NativeUpdateRefusal::AlreadyInFlight,
            };
        }
        if self.captured_attachment().as_ref() != Some(&attachment) {
            self.clear_pre_dispatch(&request);
            return NativeUpdateDispatchOutcome::RefusedBeforeExecution {
                reason: NativeUpdateRefusal::NativeHostUnavailable,
            };
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
        let outcome = map_dispatch_completion(&self.attachment.connection, &request, completion);
        if let NativeUpdateDispatchOutcome::Reply(result) = &outcome {
            self.record(result.clone());
        }
        if matches!(
            &outcome,
            NativeUpdateDispatchOutcome::RefusedBeforeExecution { .. }
        ) {
            self.clear_pre_dispatch(&request);
        }
        outcome
    }

    async fn observe_only(
        &self,
        operation_id: codeterm_types::native_update::NativeUpdateOperationId,
        after_sequence: u64,
    ) -> NativeUpdateObservationOutcome {
        let (request, attachment, previous) = {
            let state = self.state.lock();
            (
                state.request.clone(),
                state.attachment.clone(),
                state.last.clone(),
            )
        };
        let (Some(request), Some(attachment)) = (request, attachment) else {
            return unavailable(NativeUpdateUncertainty::InvalidReplyAfterDispatch);
        };
        if request.operation_id != operation_id || attachment != self.attachment {
            return unavailable(NativeUpdateUncertainty::InvalidReplyAfterDispatch);
        }
        if !self.is_current() {
            return unavailable(NativeUpdateUncertainty::DisconnectedAfterDispatch);
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
        let outcome = map_observation_completion(
            &self.attachment.connection,
            &request,
            completion,
            previous.as_ref(),
            after_sequence,
        );
        if let NativeUpdateObservationOutcome::Reply(result) = &outcome {
            self.record(result.clone());
        }
        outcome
    }

    fn is_current(&self) -> bool {
        !self
            .session
            .retired
            .load(std::sync::atomic::Ordering::Acquire)
            && self
                .endpoint
                .active_generation
                .load(std::sync::atomic::Ordering::Acquire)
                == self.attachment.connection.generation
            && self.session.process.ensure_alive().is_ok()
            && self.session.client_process == self.attachment.client_process
            && self.endpoint.active.read().as_ref().is_some_and(|active| {
                Arc::ptr_eq(active, &self.session)
                    && active.connection == self.attachment.connection
                    && active.client_process == self.attachment.client_process
            })
    }

    fn clear_pre_dispatch(&self, request: &NativeUpdateRequest) {
        let mut execution = self.state.lock();
        if execution
            .request
            .as_ref()
            .is_some_and(|current| current.operation_id == request.operation_id)
            && execution.last.is_none()
        {
            execution.request = None;
            execution.attachment = None;
        }
    }

    fn record(&self, result: NativeUpdateResult) {
        self.state.lock().last = Some(result.clone());
    }
}

pub(super) fn map_dispatch_completion(
    connection: &NativeHostConnection,
    request: &NativeUpdateRequest,
    completion: UpdateCompletion,
) -> NativeUpdateDispatchOutcome {
    match completion {
        NativeHostCompletion::Completed(NativeHostResult::NativeUpdate(result))
            if &result.connection == connection && result.validate_after(request, None).is_ok() =>
        {
            NativeUpdateDispatchOutcome::Reply(result)
        }
        NativeHostCompletion::Uncertain(reason) => {
            NativeUpdateDispatchOutcome::UncertainAfterExecution {
                reason: map_uncertainty(reason),
            }
        }
        NativeHostCompletion::Rejected(_) => NativeUpdateDispatchOutcome::RefusedBeforeExecution {
            reason: NativeUpdateRefusal::NativeHostUnavailable,
        },
        NativeHostCompletion::Completed(_) => {
            NativeUpdateDispatchOutcome::UncertainAfterExecution {
                reason: NativeUpdateUncertainty::InvalidReplyAfterDispatch,
            }
        }
    }
}

pub(super) fn map_observation_completion(
    connection: &NativeHostConnection,
    request: &NativeUpdateRequest,
    completion: UpdateCompletion,
    previous: Option<&NativeUpdateResult>,
    after_sequence: u64,
) -> NativeUpdateObservationOutcome {
    match completion {
        NativeHostCompletion::Completed(NativeHostResult::NativeUpdateObservation(
            NativeUpdateObservationOutcome::Reply(result),
        )) if &result.connection == connection
            && result.sequence > after_sequence
            && result.validate_after(request, previous).is_ok() =>
        {
            NativeUpdateObservationOutcome::Reply(result)
        }
        NativeHostCompletion::Completed(NativeHostResult::NativeUpdateObservation(
            NativeUpdateObservationOutcome::NoNewEvidence,
        )) => NativeUpdateObservationOutcome::NoNewEvidence,
        NativeHostCompletion::Completed(NativeHostResult::NativeUpdateObservation(
            NativeUpdateObservationOutcome::Unavailable { reason },
        )) => NativeUpdateObservationOutcome::Unavailable { reason },
        NativeHostCompletion::Uncertain(reason) => unavailable(map_uncertainty(reason)),
        NativeHostCompletion::Rejected(NativeHostRejection::NotAttached) => {
            unavailable(NativeUpdateUncertainty::DisconnectedAfterDispatch)
        }
        NativeHostCompletion::Rejected(_)
        | NativeHostCompletion::Completed(NativeHostResult::NativeUpdate(_))
        | NativeHostCompletion::Completed(_) => {
            unavailable(NativeUpdateUncertainty::InvalidReplyAfterDispatch)
        }
    }
}

fn unavailable(reason: NativeUpdateUncertainty) -> NativeUpdateObservationOutcome {
    NativeUpdateObservationOutcome::Unavailable { reason }
}

fn map_uncertainty(reason: NativeHostUncertainty) -> NativeUpdateUncertainty {
    match reason {
        NativeHostUncertainty::CancelledAfterDispatch => {
            NativeUpdateUncertainty::CancelledAfterDispatch
        }
        NativeHostUncertainty::DeadlineAfterDispatch => {
            NativeUpdateUncertainty::DeadlineAfterDispatch
        }
        NativeHostUncertainty::DisconnectedAfterDispatch => {
            NativeUpdateUncertainty::DisconnectedAfterDispatch
        }
    }
}

pub(crate) struct EnrollmentPolicy {
    pub challenge: NativeHostChallenge,
    pub client_identity: codeterm_types::server_bootstrap::ClientIdentity,
    pub resources: std::path::PathBuf,
    pub allowed_view_origins: HashSet<String>,
}

pub(crate) struct VerifiedClientProcess(ProcessProvenance);

#[cfg(test)]
impl VerifiedClientProcess {
    pub(super) fn for_enrollment_fixture(provenance: ProcessProvenance) -> Self {
        Self(provenance)
    }
}

pub(crate) fn verify_component(
    manifest: &ServerComponentManifest,
    ready: &ServerReady,
    trusted_resource_root: &Path,
) -> io::Result<()> {
    let resource_root = trusted_resource_root.canonicalize()?;
    let ready_resource_root = ready.resources.canonicalize()?;
    if resource_root != ready_resource_root || !resource_root.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Server resource root differs from the verified ready record",
        ));
    }
    if manifest.schema != SERVER_COMPONENT_SCHEMA
        || manifest.api_version != API_VERSION
        || manifest.bootstrap_protocol != BOOTSTRAP_PROTOCOL
        || manifest.server_build != ready.build
        || ready.api_version != manifest.api_version
        || ready.protocol != manifest.bootstrap_protocol
        || !valid_digest(&manifest.server_sha256)
        || (!matches!(
            manifest.client_identity,
            codeterm_types::server_bootstrap::ClientIdentity::None
        ) && codeterm_process_identity::client_identity::validate_client_identity(
            &manifest.client_identity,
        )
        .is_err())
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Native host component record does not match the running server",
        ));
    }
    let executable = codeterm_process_identity::client_identity::resolve_server_executable(
        &resource_root,
        &manifest.server_executable,
        &manifest.client_identity,
    )?;
    let ready_executable = ready.process.executable.canonicalize()?;
    if executable != ready_executable {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Server executable differs from the confined component path",
        ));
    }
    let server_digest = sha256_file(&executable)?;
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
    if codeterm_process_identity::client_identity::validate_client_identity(&policy.client_identity)
        .is_err()
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Client artifact digest is invalid",
        ));
    }
    process.ensure_alive()?;
    let provenance = process.provenance()?;
    #[cfg(not(target_os = "macos"))]
    let identity_matches = match &policy.client_identity {
        codeterm_types::server_bootstrap::ClientIdentity::Sha256 { sha256 } => {
            sha256_file(&process_image_path(&provenance))? == *sha256
        }
        _ => false,
    };
    #[cfg(target_os = "macos")]
    let identity_matches = match &policy.client_identity {
        codeterm_types::server_bootstrap::ClientIdentity::MacosDesignatedRequirement {
            requirement,
            team_id,
            identifier,
        } => {
            codeterm_process_identity::code_signature_macos::verify_macos_client(
                process,
                &policy.resources,
                requirement,
                team_id,
                identifier,
            )?;
            true
        }
        _ => false,
    };
    if !identity_matches || process.provenance()? != provenance {
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
    if policy.challenge.protocol != NATIVE_HOST_PROTOCOL || request.protocol != NATIVE_HOST_PROTOCOL
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
    let started = std::time::Instant::now();
    let mut bytes = 0u64;
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        hash.update(&buffer[..count]);
    }
    let digest = format!("{:x}", hash.finalize());
    log::debug!(
        "[boot-operation] native integrity hash bytes={} elapsed_ms={}",
        bytes,
        started.elapsed().as_millis()
    );
    Ok(digest)
}

#[cfg(not(target_os = "macos"))]
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
