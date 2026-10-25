use super::state::{AccountMeshRecord, AccountMeshRefusal, LifecycleState};
use super::wire::{
    ChallengeRequest, InstallationEnrollment, KeyEnrollment, MeshAdmissionBody, MeshEndpoint,
    MeshResult, RegistryRefusal, SignedEnvelope,
};
use crate::{Secret, Session};
use async_trait::async_trait;
use std::io;

pub type RegistryResult<T> = std::result::Result<T, RegistryRefusal>;

/// Server registry seam; the HTTP adapter binds the reviewed callable names.
#[async_trait]
pub trait MeshRegistry: Send + Sync {
    async fn challenge(
        &self,
        session: &Session,
        request: &ChallengeRequest,
    ) -> RegistryResult<String>;
    async fn enroll_key(
        &self,
        session: &Session,
        request: &InstallationEnrollment,
    ) -> RegistryResult<KeyEnrollment>;
    async fn admission(
        &self,
        session: &Session,
        envelope: &SignedEnvelope<MeshAdmissionBody>,
    ) -> RegistryResult<MeshResult>;
}

/// Current verified session, read on demand so no token outlives the account that owns it.
#[async_trait]
pub trait SessionSource: Send + Sync {
    async fn session(&self) -> Result<Option<Session>, AccountMeshRefusal>;
}

/// Protected storage for the installation key seed (the existing account credential store).
#[async_trait]
pub trait InstallationKeyStore: Send + Sync {
    async fn load(&self) -> crate::Result<Option<Secret>>;
    async fn save(&self, secret: &Secret) -> crate::Result<()>;
}

#[async_trait]
pub trait AccountMeshStore: Send + Sync {
    async fn load(&self) -> io::Result<AccountMeshRecord>;
    async fn save(&self, record: &AccountMeshRecord) -> io::Result<()>;
}

/// The local listener's address once it is actually reachable from the Tailnet.
pub trait EndpointSource: Send + Sync {
    fn ready_endpoint(&self) -> Option<MeshEndpoint>;
}

/// Transport-side effects the account lifecycle triggers but never performs itself.
#[async_trait]
pub trait MeshTransport: Send + Sync {
    async fn apply(&self, state: &LifecycleState);
    async fn installation_revoked(&self, installation_id: &str);
    async fn presence_changed(&self, metadata_revision: u64);
}

pub trait Clock: Send + Sync {
    fn now(&self) -> u64;
}

pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

/// Registry used until a reviewed callable binding exists: every operation is an outage.
pub struct UnavailableRegistry;
#[async_trait]
impl MeshRegistry for UnavailableRegistry {
    async fn challenge(&self, _: &Session, _: &ChallengeRequest) -> RegistryResult<String> {
        Err(RegistryRefusal::Unavailable)
    }
    async fn enroll_key(
        &self,
        _: &Session,
        _: &InstallationEnrollment,
    ) -> RegistryResult<KeyEnrollment> {
        Err(RegistryRefusal::Unavailable)
    }
    async fn admission(
        &self,
        _: &Session,
        _: &SignedEnvelope<MeshAdmissionBody>,
    ) -> RegistryResult<MeshResult> {
        Err(RegistryRefusal::Unavailable)
    }
}
