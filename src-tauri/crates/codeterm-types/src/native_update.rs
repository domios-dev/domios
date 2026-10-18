use crate::{
    native_host::NativeHostConnection,
    server_bootstrap::{
        ProcessProvenance, ServerComponentManifest, ServerExpectation, ServerIdentity, ServerReady,
        API_VERSION, BOOTSTRAP_PROTOCOL, SERVER_COMPONENT_SCHEMA,
    },
};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NativeUpdateOperationId(Uuid);

impl NativeUpdateOperationId {
    pub fn new(value: Uuid) -> Option<Self> {
        (!value.is_nil()).then_some(Self(value))
    }

    pub fn value(self) -> Uuid {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeUpdateAction {
    ClientOnly,
    Server,
    Pair,
}

impl NativeUpdateAction {
    pub fn requires_server_handoff(self) -> bool {
        matches!(self, Self::Server | Self::Pair)
    }

    pub fn requires_restart_authorization(self) -> bool {
        self.requires_server_handoff()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedServer {
    pub components: ServerComponentManifest,
    pub identity: ServerIdentity,
    pub incarnation: ServerExpectation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeUpdateAttachment {
    pub connection: NativeHostConnection,
    pub client_process: ProcessProvenance,
}

impl NativeUpdateAttachment {
    pub fn validate_for(&self, request: &NativeUpdateRequest) -> Result<(), NativeUpdateRefusal> {
        if self.connection.instance_id.trim().is_empty()
            || self.connection.generation == 0
            || !valid_process(&self.client_process)
        {
            return Err(NativeUpdateRefusal::NativeHostUnavailable);
        }
        if self.connection.identity != request.expected_server.identity
            || self.connection.server_epoch != request.expected_server.incarnation.epoch
        {
            return Err(NativeUpdateRefusal::ExpectedServerMismatch);
        }
        Ok(())
    }
}

impl PartialEq for ExpectedServer {
    fn eq(&self, other: &Self) -> bool {
        self.components == other.components
            && self.identity == other.identity
            && self.incarnation.epoch == other.incarnation.epoch
            && self.incarnation.process == other.incarnation.process
    }
}

impl Eq for ExpectedServer {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeUpdateRequest {
    pub operation_id: NativeUpdateOperationId,
    pub action: NativeUpdateAction,
    pub expected_server: ExpectedServer,
    pub staged_components: ServerComponentManifest,
    pub restart_authorized: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeUpdateStage {
    Downloading,
    Verifying,
    Applying,
    WaitingForPredecessorExit,
    WaitingForReplacementHealth,
    RestoringSessions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeUpdateUncertainty {
    CancelledAfterDispatch,
    DeadlineAfterDispatch,
    DisconnectedAfterDispatch,
    InvalidReplyAfterDispatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeUpdateRefusal {
    InvalidOperationIdentity,
    InvalidServerManifest,
    InvalidStagedManifest,
    ExpectedServerMismatch,
    RestartAuthorizationRequired,
    UnexpectedRestartAuthorization,
    ClientOnlyChangesServer,
    ServerOnlyChangesClientOrCompatibility,
    WrongOperation,
    StaleSequence,
    AlreadyInFlight,
    NativeHostUnavailable,
    DurableHandoffUnavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeUpdateDispatchOutcome {
    Reply(NativeUpdateResult),
    RefusedBeforeExecution { reason: NativeUpdateRefusal },
    UncertainAfterExecution { reason: NativeUpdateUncertainty },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "observation", content = "payload", rename_all = "snake_case")]
pub enum NativeUpdateObservationOutcome {
    Reply(NativeUpdateResult),
    NoNewEvidence,
    Unavailable { reason: NativeUpdateUncertainty },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeUpdateInstallReceipt {
    pub installed_components: ServerComponentManifest,
    pub client_process: ProcessProvenance,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeUpdateVerification {
    pub client_process: ProcessProvenance,
    pub server: ServerReady,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "payload", rename_all = "snake_case")]
pub enum NativeUpdateState {
    Admitted,
    InProgress {
        stage: NativeUpdateStage,
    },
    Uncertain {
        reason: NativeUpdateUncertainty,
    },
    /// The broker's physical installation receipt; replacement health is not implied.
    Completed {
        install: NativeUpdateInstallReceipt,
    },
    InstalledAwaitingVerification {
        install: NativeUpdateInstallReceipt,
    },
    /// The replacement client re-enrolled and the pinned server still reports ready.
    Verified {
        install: NativeUpdateInstallReceipt,
        verification: NativeUpdateVerification,
    },
    Rejected {
        reason: NativeUpdateRefusal,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeUpdateResult {
    pub operation_id: NativeUpdateOperationId,
    pub sequence: u64,
    pub connection: NativeHostConnection,
    pub state: NativeUpdateState,
}

impl NativeUpdateResult {
    pub fn validate_for(&self, request: &NativeUpdateRequest) -> Result<(), NativeUpdateRefusal> {
        request.validate()?;
        if self.operation_id != request.operation_id {
            return Err(NativeUpdateRefusal::WrongOperation);
        }
        if self.sequence == 0 {
            return Err(NativeUpdateRefusal::StaleSequence);
        }
        validate_connection(&self.connection, request)?;
        match &self.state {
            NativeUpdateState::Completed { install } => {
                validate_install(install, request)?;
            }
            NativeUpdateState::InstalledAwaitingVerification { install } => {
                validate_install(install, request)?;
            }
            NativeUpdateState::Verified {
                install,
                verification,
            } => {
                validate_install(install, request)?;
                validate_verification(verification, install, request)?;
                if request.action.requires_server_handoff() {
                    return Err(NativeUpdateRefusal::DurableHandoffUnavailable);
                }
                if self.connection.server_epoch != request.expected_server.incarnation.epoch
                    || self.connection.server_epoch != verification.server.epoch
                    || self.connection.instance_id != verification.server.instance_id
                    || self.connection.identity != verification.server.identity
                    || verification.server.process != request.expected_server.incarnation.process
                {
                    return Err(NativeUpdateRefusal::ExpectedServerMismatch);
                }
            }
            NativeUpdateState::Admitted | NativeUpdateState::Rejected { .. } => {
                if request.action.requires_server_handoff()
                    && self.connection.server_epoch != request.expected_server.incarnation.epoch
                {
                    return Err(NativeUpdateRefusal::ExpectedServerMismatch);
                }
            }
            NativeUpdateState::InProgress { .. } => {}
            NativeUpdateState::Uncertain { .. } => {}
        }
        if request.action.requires_server_handoff() {
            return Err(NativeUpdateRefusal::DurableHandoffUnavailable);
        }
        Ok(())
    }

    pub fn validate_after(
        &self,
        request: &NativeUpdateRequest,
        previous: Option<&NativeUpdateResult>,
    ) -> Result<(), NativeUpdateRefusal> {
        self.validate_for(request)?;
        if let Some(previous) = previous {
            if self.sequence <= previous.sequence
                || self.connection.generation < previous.connection.generation
            {
                return Err(NativeUpdateRefusal::StaleSequence);
            }
            if self.connection.instance_id != previous.connection.instance_id
                || self.connection.identity != previous.connection.identity
            {
                return Err(NativeUpdateRefusal::ExpectedServerMismatch);
            }
            let reconnect_transition =
                matches!(&previous.state, NativeUpdateState::Completed { .. })
                    && matches!(
                        &self.state,
                        NativeUpdateState::InstalledAwaitingVerification { .. }
                            | NativeUpdateState::Verified { .. }
                    );
            let verified_transition = matches!(&self.state, NativeUpdateState::Verified { .. })
                && !matches!(
                    &previous.state,
                    NativeUpdateState::InstalledAwaitingVerification { .. }
                );
            if (reconnect_transition || verified_transition)
                && self.connection.generation <= previous.connection.generation
            {
                return Err(NativeUpdateRefusal::StaleSequence);
            }
            if self.connection.server_epoch != previous.connection.server_epoch {
                return Err(NativeUpdateRefusal::ExpectedServerMismatch);
            }
        }
        Ok(())
    }
}

fn validate_connection(
    connection: &NativeHostConnection,
    request: &NativeUpdateRequest,
) -> Result<(), NativeUpdateRefusal> {
    if connection.instance_id.trim().is_empty()
        || connection.server_epoch.trim().is_empty()
        || connection.generation == 0
        || connection.identity != request.expected_server.identity
    {
        return Err(NativeUpdateRefusal::ExpectedServerMismatch);
    }
    if connection.server_epoch != request.expected_server.incarnation.epoch {
        return Err(NativeUpdateRefusal::ExpectedServerMismatch);
    }
    Ok(())
}

fn validate_install(
    install: &NativeUpdateInstallReceipt,
    request: &NativeUpdateRequest,
) -> Result<(), NativeUpdateRefusal> {
    validate_manifest(&install.installed_components)
        .map_err(|()| NativeUpdateRefusal::InvalidStagedManifest)?;
    if install.installed_components != request.staged_components
        || !valid_process(&install.client_process)
    {
        return Err(NativeUpdateRefusal::InvalidStagedManifest);
    }
    Ok(())
}

fn validate_verification(
    verification: &NativeUpdateVerification,
    install: &NativeUpdateInstallReceipt,
    request: &NativeUpdateRequest,
) -> Result<(), NativeUpdateRefusal> {
    let server = &verification.server;
    if !valid_process(&verification.client_process)
        || verification.client_process != install.client_process
        || server.epoch.trim().is_empty()
        || server.process.pid == 0
        || server.process.created.trim().is_empty()
        || server.process.executable.as_os_str().is_empty()
        || server.identity != request.expected_server.identity
        || server.build != request.staged_components.server_build
        || server.api_version != request.staged_components.api_version
        || server.protocol != request.staged_components.bootstrap_protocol
        || server.process.executable != request.staged_components.server_executable
    {
        return Err(NativeUpdateRefusal::InvalidStagedManifest);
    }
    if request.action == NativeUpdateAction::ClientOnly
        && (server.epoch != request.expected_server.incarnation.epoch
            || server.process != request.expected_server.incarnation.process)
    {
        return Err(NativeUpdateRefusal::ExpectedServerMismatch);
    }
    if request.action.requires_server_handoff()
        && (server.epoch == request.expected_server.incarnation.epoch
            || server.process == request.expected_server.incarnation.process)
    {
        return Err(NativeUpdateRefusal::ExpectedServerMismatch);
    }
    Ok(())
}

fn valid_process(process: &ProcessProvenance) -> bool {
    process.pid > 0
        && !process.created.trim().is_empty()
        && !process.executable.as_os_str().is_empty()
}

pub type NativeUpdateObservationFuture =
    Pin<Box<dyn Future<Output = NativeUpdateObservationOutcome> + Send>>;
pub type NativeUpdateDispatchFuture =
    Pin<Box<dyn Future<Output = NativeUpdateDispatchOutcome> + Send>>;

pub trait NativeUpdateExecutor: Send + Sync {
    /// Returns only the exact active authenticated attachment captured by this executor.
    fn captured_attachment(&self) -> Option<NativeUpdateAttachment>;

    /// Starts once after rechecking that this exact authenticated attachment is still active.
    fn dispatch(
        &self,
        request: NativeUpdateRequest,
        attachment: NativeUpdateAttachment,
    ) -> NativeUpdateDispatchFuture;

    /// Observes a later event without replaying work; the adapter binds its authenticated connection.
    fn observe(
        &self,
        operation_id: NativeUpdateOperationId,
        after_sequence: u64,
    ) -> NativeUpdateObservationFuture;
}

impl NativeUpdateRequest {
    pub fn validate(&self) -> Result<(), NativeUpdateRefusal> {
        if self.operation_id.0.is_nil() {
            return Err(NativeUpdateRefusal::InvalidOperationIdentity);
        }
        validate_manifest(&self.expected_server.components)
            .map_err(|()| NativeUpdateRefusal::InvalidServerManifest)?;
        validate_manifest(&self.staged_components)
            .map_err(|()| NativeUpdateRefusal::InvalidStagedManifest)?;
        if self.expected_server.incarnation.epoch.trim().is_empty()
            || self.expected_server.incarnation.process.pid == 0
            || self
                .expected_server
                .incarnation
                .process
                .created
                .trim()
                .is_empty()
            || self.expected_server.identity.data.as_os_str().is_empty()
            || self.expected_server.identity.config.as_os_str().is_empty()
            || self.expected_server.components.server_executable
                != self.expected_server.incarnation.process.executable
        {
            return Err(NativeUpdateRefusal::ExpectedServerMismatch);
        }
        match self.action {
            NativeUpdateAction::ClientOnly => {
                if self.restart_authorized {
                    return Err(NativeUpdateRefusal::UnexpectedRestartAuthorization);
                }
                if !same_server_artifact(&self.expected_server.components, &self.staged_components)
                {
                    return Err(NativeUpdateRefusal::ClientOnlyChangesServer);
                }
            }
            NativeUpdateAction::Server => {
                if !self.restart_authorized {
                    return Err(NativeUpdateRefusal::RestartAuthorizationRequired);
                }
                if self.expected_server.components.client_sha256
                    != self.staged_components.client_sha256
                    || self.expected_server.components.api_version
                        != self.staged_components.api_version
                    || self.expected_server.components.bootstrap_protocol
                        != self.staged_components.bootstrap_protocol
                {
                    return Err(NativeUpdateRefusal::ServerOnlyChangesClientOrCompatibility);
                }
            }
            NativeUpdateAction::Pair if !self.restart_authorized => {
                return Err(NativeUpdateRefusal::RestartAuthorizationRequired);
            }
            NativeUpdateAction::Pair => {}
        }
        Ok(())
    }

    pub fn matches_current_server(
        &self,
        ready: &ServerReady,
        installed: &ServerComponentManifest,
    ) -> bool {
        let expected = &self.expected_server;
        &expected.components == installed
            && !ready.instance_id.trim().is_empty()
            && expected.identity == ready.identity
            && expected.incarnation.epoch == ready.epoch
            && expected.incarnation.process == ready.process
            && ready.build == expected.components.server_build
            && ready.process.executable == expected.components.server_executable
            && ready.api_version == expected.components.api_version
            && ready.protocol == expected.components.bootstrap_protocol
    }
}

fn validate_manifest(manifest: &ServerComponentManifest) -> Result<(), ()> {
    if manifest.schema != SERVER_COMPONENT_SCHEMA
        || manifest.server_build.is_empty()
        || manifest.server_executable.as_os_str().is_empty()
        || manifest.api_version != API_VERSION
        || manifest.bootstrap_protocol != BOOTSTRAP_PROTOCOL
        || !valid_sha256(&manifest.server_sha256)
        || !valid_sha256(&manifest.client_sha256)
    {
        return Err(());
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn same_server_artifact(left: &ServerComponentManifest, right: &ServerComponentManifest) -> bool {
    left.server_build == right.server_build
        && left.server_executable == right.server_executable
        && left.server_sha256 == right.server_sha256
        && left.api_version == right.api_version
        && left.bootstrap_protocol == right.bootstrap_protocol
}

#[cfg(test)]
mod tests {
    use super::{
        ExpectedServer, NativeUpdateAction, NativeUpdateExecutor, NativeUpdateObservationOutcome,
        NativeUpdateOperationId, NativeUpdateRefusal, NativeUpdateRequest, NativeUpdateResult,
        NativeUpdateStage, NativeUpdateState, NativeUpdateUncertainty, NativeUpdateVerification,
    };
    use crate::{
        native_host::NativeHostConnection,
        server_bootstrap::{
            ProcessProvenance, ServerComponentManifest, ServerExpectation, ServerIdentity,
            ServerReady, API_VERSION, BOOTSTRAP_PROTOCOL, SERVER_COMPONENT_SCHEMA,
        },
    };
    use std::{future::Future, path::PathBuf, pin::Pin};
    use uuid::Uuid;

    fn components(server_build: &str, client_digest: &str) -> ServerComponentManifest {
        ServerComponentManifest {
            schema: SERVER_COMPONENT_SCHEMA,
            server_build: server_build.into(),
            server_executable: PathBuf::from("C:/Domios/domios-server.exe"),
            server_sha256: "a".repeat(64),
            client_sha256: client_digest.into(),
            api_version: API_VERSION,
            bootstrap_protocol: BOOTSTRAP_PROTOCOL,
        }
    }

    fn request(action: NativeUpdateAction, restart_authorized: bool) -> NativeUpdateRequest {
        let current = components("server-old", "b".repeat(64).as_str());
        let staged = match action {
            NativeUpdateAction::ClientOnly => components("server-old", "c".repeat(64).as_str()),
            NativeUpdateAction::Server => components("server-new", "b".repeat(64).as_str()),
            NativeUpdateAction::Pair => components("server-new", "c".repeat(64).as_str()),
        };
        let executable = current.server_executable.clone();
        NativeUpdateRequest {
            operation_id: NativeUpdateOperationId::new(Uuid::from_u128(17)).unwrap(),
            action,
            expected_server: ExpectedServer {
                components: current,
                identity: ServerIdentity {
                    data: PathBuf::from("C:/Domios/data"),
                    config: PathBuf::from("C:/Domios/config"),
                },
                incarnation: ServerExpectation {
                    process: ProcessProvenance {
                        pid: 41,
                        created: "start-41".into(),
                        executable,
                    },
                    epoch: "epoch-old".into(),
                },
            },
            staged_components: staged,
            restart_authorized,
        }
    }

    fn ready_for(request: &NativeUpdateRequest) -> ServerReady {
        let expected = &request.expected_server;
        ServerReady {
            protocol: expected.components.bootstrap_protocol,
            api_version: expected.components.api_version,
            build: expected.components.server_build.clone(),
            instance_id: "instance".into(),
            identity: expected.identity.clone(),
            epoch: expected.incarnation.epoch.clone(),
            process: expected.incarnation.process.clone(),
            api_address: "127.0.0.1:7686".parse().expect("valid test address"),
            socket_address: "socket".into(),
            bootstrap_address: "bootstrap".into(),
            resources: PathBuf::from("C:/Domios/resources"),
        }
    }

    fn connection_for(
        request: &NativeUpdateRequest,
        generation: u64,
        server_epoch: &str,
    ) -> NativeHostConnection {
        NativeHostConnection {
            instance_id: "instance".into(),
            identity: request.expected_server.identity.clone(),
            server_epoch: server_epoch.into(),
            generation,
        }
    }

    #[test]
    fn observation_outcomes_round_trip_with_finite_tags() {
        let request = request(NativeUpdateAction::ClientOnly, false);
        let reply = NativeUpdateResult {
            operation_id: request.operation_id,
            sequence: 1,
            connection: connection_for(&request, 1, &request.expected_server.incarnation.epoch),
            state: NativeUpdateState::InProgress {
                stage: NativeUpdateStage::Applying,
            },
        };
        let outcomes = [
            (NativeUpdateObservationOutcome::Reply(reply), "reply"),
            (
                NativeUpdateObservationOutcome::NoNewEvidence,
                "no_new_evidence",
            ),
            (
                NativeUpdateObservationOutcome::Unavailable {
                    reason: NativeUpdateUncertainty::InvalidReplyAfterDispatch,
                },
                "unavailable",
            ),
        ];

        for (outcome, expected_tag) in outcomes {
            let encoded = serde_json::to_value(&outcome).unwrap();
            assert_eq!(encoded["observation"], expected_tag);
            match expected_tag {
                "reply" => assert_eq!(encoded["payload"]["sequence"], 1),
                "no_new_evidence" => {
                    assert!(encoded.get("payload").is_none());
                    assert!(encoded.get("sequence").is_none());
                }
                "unavailable" => assert!(encoded["payload"].get("sequence").is_none()),
                _ => unreachable!(),
            }
            let decoded: NativeUpdateObservationOutcome = serde_json::from_value(encoded).unwrap();
            assert_eq!(decoded, outcome);
        }
    }

    struct Port;

    impl NativeUpdateExecutor for Port {
        fn captured_attachment(&self) -> Option<NativeUpdateAttachment> {
            let request = request(NativeUpdateAction::ClientOnly, false);
            Some(NativeUpdateAttachment {
                connection: connection_for(&request, 1, &request.expected_server.incarnation.epoch),
                client_process: ProcessProvenance {
                    pid: 72,
                    created: "start-72".into(),
                    executable: PathBuf::from("C:/Domios/domios.exe"),
                },
            })
        }

        fn dispatch(
            &self,
            request: NativeUpdateRequest,
            attachment: NativeUpdateAttachment,
        ) -> super::NativeUpdateDispatchFuture {
            Box::pin(async move {
                super::NativeUpdateDispatchOutcome::Reply(NativeUpdateResult {
                    operation_id: request.operation_id,
                    sequence: 1,
                    connection: attachment.connection,
                    state: NativeUpdateState::Admitted,
                })
            })
        }

        fn observe(
            &self,
            operation_id: NativeUpdateOperationId,
            after_sequence: u64,
        ) -> super::NativeUpdateObservationFuture {
            Box::pin(async move {
                super::NativeUpdateObservationOutcome::Reply(NativeUpdateResult {
                    operation_id,
                    sequence: after_sequence + 1,
                    connection: NativeHostConnection {
                        instance_id: "instance".into(),
                        identity: ServerIdentity {
                            data: PathBuf::from("C:/Domios/data"),
                            config: PathBuf::from("C:/Domios/config"),
                        },
                        server_epoch: "epoch-old".into(),
                        generation: 1,
                    },
                    state: NativeUpdateState::Uncertain {
                        reason: NativeUpdateUncertainty::DisconnectedAfterDispatch,
                    },
                })
            })
        }
    }

    #[test]
    fn only_server_replacement_requires_handoff_and_explicit_authorization() {
        assert!(!NativeUpdateAction::ClientOnly.requires_server_handoff());
        assert!(!NativeUpdateAction::ClientOnly.requires_restart_authorization());
        for action in [NativeUpdateAction::Server, NativeUpdateAction::Pair] {
            assert!(action.requires_server_handoff());
            assert!(action.requires_restart_authorization());
        }
    }

    #[test]
    fn client_only_update_pins_the_same_server_artifact_and_epoch() {
        let request = request(NativeUpdateAction::ClientOnly, false);
        let ready = ready_for(&request);
        let installed = request.expected_server.components.clone();
        assert_eq!(request.validate(), Ok(()));
        assert!(request.matches_current_server(&ready, &installed));

        let mut changed = request;
        changed.staged_components.server_build = "server-new".into();
        assert_eq!(
            changed.validate(),
            Err(NativeUpdateRefusal::ClientOnlyChangesServer)
        );
    }

    #[test]
    fn server_and_pair_updates_require_restart_authorization() {
        for action in [NativeUpdateAction::Server, NativeUpdateAction::Pair] {
            let update_request = request(action, false);
            assert_eq!(
                update_request.validate(),
                Err(NativeUpdateRefusal::RestartAuthorizationRequired)
            );
            assert_eq!(request(action, true).validate(), Ok(()));
        }
    }

    #[test]
    fn server_only_update_preserves_client_artifact_and_protocol_pair() {
        let mut request = request(NativeUpdateAction::Server, true);
        request.staged_components.client_sha256 = "c".repeat(64);
        assert_eq!(
            request.validate(),
            Err(NativeUpdateRefusal::ServerOnlyChangesClientOrCompatibility)
        );
    }

    #[test]
    fn current_server_match_requires_the_trusted_manifest_and_exact_incarnation() {
        let request = request(NativeUpdateAction::ClientOnly, false);
        let ready = ready_for(&request);
        let installed = request.expected_server.components.clone();
        assert!(request.matches_current_server(&ready, &installed));

        let mut replacement = ready;
        replacement.epoch = "epoch-new".into();
        assert!(!request.matches_current_server(&replacement, &installed));

        let mut wrong_artifact = installed;
        wrong_artifact.server_build = "other-build".into();
        assert!(!request.matches_current_server(&ready_for(&request), &wrong_artifact));
    }

    #[test]
    fn executor_port_is_object_safe_and_waits_on_operation_sequence() {
        let port: &dyn NativeUpdateExecutor = &Port;
        let request = request(NativeUpdateAction::ClientOnly, false);
        let request_id = request.operation_id;
        let attachment = port.captured_attachment().unwrap();
        attachment.validate_for(&request).unwrap();
        let first = futures_poll_ready(port.dispatch(request, attachment));
        let super::NativeUpdateDispatchOutcome::Reply(first) = first else {
            panic!("the attached test executor replies");
        };
        assert_eq!(first.sequence, 1);
        assert!(matches!(first.state, NativeUpdateState::Admitted));

        let next = futures_poll_ready(port.observe(request_id, first.sequence));
        assert_eq!(next.operation_id, request_id);
        assert_eq!(next.sequence, 2);
        assert!(matches!(
            next.state,
            NativeUpdateState::Uncertain {
                reason: NativeUpdateUncertainty::DisconnectedAfterDispatch
            }
        ));
    }

    fn futures_poll_ready<T>(mut future: Pin<Box<dyn Future<Output = T> + Send + '_>>) -> T {
        use std::task::{Context, Poll, Wake, Waker};
        struct ReadyWake;
        impl Wake for ReadyWake {
            fn wake(self: std::sync::Arc<Self>) {}
        }
        let waker = Waker::from(std::sync::Arc::new(ReadyWake));
        let mut context = Context::from_waker(&waker);
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("the test port resolves synchronously"),
        }
    }

    #[test]
    fn result_fences_operation_and_completed_artifact_identity() {
        let request = request(NativeUpdateAction::ClientOnly, false);
        let wrong = NativeUpdateResult {
            operation_id: NativeUpdateOperationId::new(Uuid::from_u128(18)).unwrap(),
            sequence: 1,
            connection: connection_for(&request, 1, "epoch-old"),
            state: NativeUpdateState::Admitted,
        };
        assert_eq!(
            wrong.validate_for(&request),
            Err(NativeUpdateRefusal::WrongOperation)
        );

        let client_process = ProcessProvenance {
            pid: 72,
            created: "start-72".into(),
            executable: PathBuf::from("C:/Domios/domios.exe"),
        };
        let completed = NativeUpdateResult {
            operation_id: request.operation_id,
            sequence: 4,
            connection: connection_for(&request, 1, "epoch-old"),
            state: NativeUpdateState::Completed {
                install: super::NativeUpdateInstallReceipt {
                    installed_components: request.staged_components.clone(),
                    client_process: client_process.clone(),
                },
            },
        };
        assert_eq!(completed.validate_for(&request), Ok(()));

        let mut wrong_install = completed;
        if let NativeUpdateState::Completed { install } = &mut wrong_install.state {
            install.installed_components.server_build = "other-build".into();
        }
        assert_eq!(
            wrong_install.validate_for(&request),
            Err(NativeUpdateRefusal::InvalidStagedManifest)
        );
    }

    #[test]
    fn verified_client_reconnect_requires_monotonic_generation_and_same_server_epoch() {
        let request = request(NativeUpdateAction::ClientOnly, false);
        let installed = super::NativeUpdateInstallReceipt {
            installed_components: request.staged_components.clone(),
            client_process: ProcessProvenance {
                pid: 72,
                created: "start-72".into(),
                executable: PathBuf::from("C:/Domios/domios.exe"),
            },
        };
        let previous = NativeUpdateResult {
            operation_id: request.operation_id,
            sequence: 4,
            connection: connection_for(&request, 9, "epoch-old"),
            state: NativeUpdateState::InstalledAwaitingVerification {
                install: installed.clone(),
            },
        };
        let verified = NativeUpdateResult {
            operation_id: request.operation_id,
            sequence: 5,
            connection: connection_for(&request, 10, "epoch-old"),
            state: NativeUpdateState::Verified {
                install: installed.clone(),
                verification: NativeUpdateVerification {
                    client_process: installed.client_process.clone(),
                    server: ready_for(&request),
                },
            },
        };
        assert_eq!(verified.validate_after(&request, Some(&previous)), Ok(()));

        let mut same_generation = verified.clone();
        same_generation.connection.generation = previous.connection.generation - 1;
        assert_eq!(
            same_generation.validate_after(&request, Some(&previous)),
            Err(NativeUpdateRefusal::StaleSequence)
        );

        let mut wrong_epoch = verified;
        wrong_epoch.connection.server_epoch = "epoch-new".into();
        assert_eq!(
            wrong_epoch.validate_for(&request),
            Err(NativeUpdateRefusal::ExpectedServerMismatch)
        );
    }

    #[test]
    fn uncertain_after_dispatch_is_not_a_terminal_completion() {
        let request = request(NativeUpdateAction::ClientOnly, false);
        let result = NativeUpdateResult {
            operation_id: request.operation_id,
            sequence: 3,
            connection: connection_for(&request, 1, "epoch-old"),
            state: NativeUpdateState::Uncertain {
                reason: NativeUpdateUncertainty::DeadlineAfterDispatch,
            },
        };
        assert_eq!(result.validate_for(&request), Ok(()));
        assert!(matches!(result.state, NativeUpdateState::Uncertain { .. }));
    }
}
