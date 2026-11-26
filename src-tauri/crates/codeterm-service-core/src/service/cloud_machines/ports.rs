use std::any::Any;
use std::net::SocketAddrV4;
use std::sync::Arc;

use async_trait::async_trait;
use zeroize::Zeroizing;

use super::held_sends::{FlushWriteOutcome, HeldCloudSend};
use super::idle_drain::{DrainToken, PeerBusySnapshot};
use super::types::{CloudError, CloudMachineRecord, CloudRate, CloudSleepMode, CloudUsageSummary};

pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CloudRouteSpec {
    Direct { address: SocketAddrV4 },
}

pub struct ProvisioningCreateRequest {
    pub size: String,
    pub name: Option<String>,
    pub controller_key: String,
    pub creation_psk: Zeroizing<String>,
    pub request_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProvisionedMachine {
    pub machine_id: String,
    pub provider: String,
    pub endpoint: SocketAddrV4,
    pub size: String,
    pub rate: CloudRate,
    pub sleep_mode: CloudSleepMode,
    pub lease_id: String,
    pub lease_expires_at_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProvisionedLease {
    pub machine_id: String,
    pub lease_id: String,
    pub lease_expires_at_ms: u64,
}

#[async_trait]
pub trait CloudProvisioningPort: Send + Sync {
    async fn create(
        &self,
        request: &ProvisioningCreateRequest,
    ) -> Result<ProvisionedMachine, CloudError>;
    async fn wake(
        &self,
        machine_id: &str,
        request_id: &str,
    ) -> Result<ProvisionedMachine, CloudError>;
    async fn sleep(&self, machine_id: &str, request_id: &str)
        -> Result<CloudSleepMode, CloudError>;
    async fn delete(&self, machine_id: &str, request_id: &str) -> Result<(), CloudError>;
    async fn renew_lease(
        &self,
        machine_id: &str,
        lease_id: &str,
        request_id: &str,
    ) -> Result<ProvisionedLease, CloudError>;
    async fn usage(&self) -> Result<CloudUsageSummary, CloudError>;
}

pub struct EnrollmentBinding {
    pub controller_key: String,
    pub provider_creation_nonce: Zeroizing<String>,
    pub opaque: Arc<dyn Any + Send + Sync>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnrollmentOutcome {
    Enrolled {
        installation_id: String,
        peer_instance_id: Option<String>,
    },
    ApplyUnconfirmed {
        installation_id: String,
    },
}

#[async_trait]
pub trait CloudEnrollmentPort: Send + Sync {
    async fn prepare_binding(&self) -> Result<EnrollmentBinding, CloudError>;
    async fn enroll(
        &self,
        route: &CloudRouteSpec,
        binding: &EnrollmentBinding,
        provider: &str,
        size: &str,
    ) -> Result<EnrollmentOutcome, CloudError>;
}

#[async_trait]
pub trait CloudPeerControlPort: Send + Sync {
    async fn read_idle(&self, record: &CloudMachineRecord) -> Result<PeerBusySnapshot, CloudError>;
    async fn enter_drain(&self, record: &CloudMachineRecord) -> Result<DrainToken, CloudError>;
    async fn reopen_drain(&self, record: &CloudMachineRecord) -> Result<(), CloudError>;
    async fn update_endpoint(
        &self,
        record: &CloudMachineRecord,
        endpoint: SocketAddrV4,
    ) -> Result<(), CloudError>;
    async fn await_peer_ready(
        &self,
        record: &CloudMachineRecord,
        deadline_ms: u64,
    ) -> Result<(), CloudError>;
    async fn flush_send(
        &self,
        record: &CloudMachineRecord,
        send: &HeldCloudSend,
    ) -> FlushWriteOutcome;
}
