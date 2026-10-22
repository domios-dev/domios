pub mod apply;

use crate::{
    native_host::NativeHostConnection,
    server_bootstrap::{
        ProcessProvenance, ServerComponentManifest, ServerExpectation, ServerIdentity, ServerReady,
        API_VERSION, BOOTSTRAP_PROTOCOL, SERVER_COMPONENT_SCHEMA,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    future::Future,
    path::{Component, Path},
    pin::Pin,
};
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
        || !resource_executable_matches(
            &server.resources,
            &request.staged_components.server_executable,
            &server.process.executable,
        )
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
            || !valid_process(&self.expected_server.incarnation.process)
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
                if self.expected_server.components.client_identity
                    != self.staged_components.client_identity
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
            && resource_executable_matches(
                &ready.resources,
                &expected.components.server_executable,
                &ready.process.executable,
            )
            && ready.api_version == expected.components.api_version
            && ready.protocol == expected.components.bootstrap_protocol
    }
}

fn validate_manifest(manifest: &ServerComponentManifest) -> Result<(), ()> {
    if manifest.schema != SERVER_COMPONENT_SCHEMA
        || manifest.server_build.is_empty()
        || !valid_server_executable(&manifest.server_executable)
        || manifest.api_version != API_VERSION
        || manifest.bootstrap_protocol != BOOTSTRAP_PROTOCOL
        || !valid_sha256(&manifest.server_sha256)
        || !valid_client_identity(&manifest.client_identity)
    {
        return Err(());
    }
    Ok(())
}

fn is_strict_resource_relative_path(path: &Path) -> bool {
    let mut components = path.components();
    matches!(components.next(), Some(Component::Normal(_)))
        && components.all(|component| matches!(component, Component::Normal(_)))
}

fn valid_server_executable(path: &Path) -> bool {
    if cfg!(target_os = "macos") {
        path == Path::new("../MacOS/domios-server")
    } else {
        is_strict_resource_relative_path(path)
    }
}

fn valid_client_identity(identity: &crate::server_bootstrap::ClientIdentity) -> bool {
    use crate::server_bootstrap::ClientIdentity;
    match identity {
        ClientIdentity::None => false,
        ClientIdentity::Sha256 { sha256 } => !cfg!(target_os = "macos") && valid_sha256(sha256),
        ClientIdentity::MacosDesignatedRequirement { requirement, team_id, identifier } => {
            cfg!(target_os = "macos")
                && team_id.len() == 10
                && team_id.bytes().all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
                && identifier == "com.domios.app"
                && requirement == &format!("anchor apple generic and identifier \"com.domios.app\" and certificate leaf[subject.OU] = \"{team_id}\"")
        }
    }
}

fn resource_executable_matches(root: &Path, relative: &Path, executable: &Path) -> bool {
    if !is_clean_absolute_path(root) || !valid_server_executable(relative) {
        return false;
    }
    if cfg!(target_os = "macos") {
        root.file_name().is_some_and(|name| name == "Resources")
            && root
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "Contents")
            && root
                .parent()
                .and_then(Path::parent)
                .and_then(Path::file_name)
                .is_some_and(|name| name == "Domios.app")
            && root
                .parent()
                .is_some_and(|contents| contents.join("MacOS/domios-server") == executable)
    } else {
        root.join(relative) == executable
    }
}

fn is_clean_absolute_path(path: &Path) -> bool {
    path.is_absolute()
        && path.components().all(|component| {
            matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::Normal(_)
            )
        })
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn same_server_artifact(left: &ServerComponentManifest, right: &ServerComponentManifest) -> bool {
    left.server_build == right.server_build
        && left.server_executable == right.server_executable
        && left.server_sha256 == right.server_sha256
        && left.api_version == right.api_version
        && left.bootstrap_protocol == right.bootstrap_protocol
}

#[cfg(test)]
mod tests;
