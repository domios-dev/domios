use std::any::Any;
use std::collections::BTreeMap;
use std::net::SocketAddrV4;
use std::sync::Arc;

use async_trait::async_trait;
use reqwest::{
    header::{HeaderName, HeaderValue},
    Url,
};
use zeroize::Zeroizing;

use super::held_sends::{FlushWriteOutcome, HeldCloudSend};
use super::idle_drain::{DrainToken, PeerBusySnapshot};
use super::state::ObservedProviderMachine;
use super::types::{
    CloudError, CloudMachineRecord, CloudOwnershipReceipt, CloudRate, CloudSleepMode,
};

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

#[derive(Clone, Debug)]
pub enum CloudRouteSpec {
    Direct {
        address: SocketAddrV4,
    },
    Preview {
        origin: Url,
        header_name: HeaderName,
        header_value: HeaderValue,
        rotates_on_restart: bool,
    },
}

#[derive(Clone, Debug)]
pub struct CloudCreateRequest {
    pub size: String,
    pub image_id: String,
    pub request_id: uuid::Uuid,
    pub labels: BTreeMap<String, String>,
    pub env: BTreeMap<String, String>,
    pub auto_stop_minutes: u32,
}

#[async_trait]
pub trait CloudBackendPort: Send + Sync {
    async fn prepare_image(&self, size: &str) -> Result<String, CloudError>;
    async fn create(
        &self,
        request: &CloudCreateRequest,
    ) -> Result<ObservedProviderMachine, CloudError>;
    async fn start(
        &self,
        provider_machine_id: &str,
        auto_stop_minutes: u32,
    ) -> Result<(), CloudError>;
    async fn stop(&self, provider_machine_id: &str) -> Result<(), CloudError>;
    async fn delete(&self, ownership: &CloudOwnershipReceipt) -> Result<(), CloudError>;
    async fn list(
        &self,
        labels: &BTreeMap<String, String>,
    ) -> Result<Vec<ObservedProviderMachine>, CloudError>;
    async fn access(
        &self,
        provider_machine_id: &str,
        port: u16,
    ) -> Result<CloudRouteSpec, CloudError>;
    async fn refresh_activity(
        &self,
        provider_machine_id: &str,
        auto_stop_minutes: u32,
    ) -> Result<(), CloudError>;
    async fn hourly_rate(&self, size: &str) -> Result<Option<CloudRate>, CloudError>;
    async fn sleep_mode(&self) -> Result<CloudSleepMode, CloudError>;
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
