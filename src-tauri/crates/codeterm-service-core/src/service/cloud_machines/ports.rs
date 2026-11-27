use std::any::Any;
use std::sync::Arc;

use async_trait::async_trait;
use zeroize::Zeroizing;

use super::held_sends::{FlushWriteOutcome, HeldCloudSend};
use super::idle_drain::{DrainToken, PeerBusySnapshot};
use super::storage::{
    CloudColdWakeParams, CloudDeleteMode, CloudDeleteReceipt, CloudStorageTier,
    CloudVolumeResizeReply,
};
use super::types::{
    CloudCheckoutSession, CloudEndpoint, CloudError, CloudMachineRecord, CloudPromoRedemption,
    CloudRate, CloudSleepMode, CloudUsageSummary,
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

pub type CloudRouteSpec = CloudEndpoint;

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
    pub endpoint: CloudEndpoint,
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
    async fn delete_with_mode(
        &self,
        machine_id: &str,
        request_id: &str,
        mode: CloudDeleteMode,
        _archive_id: Option<&str>,
    ) -> Result<CloudDeleteReceipt, CloudError> {
        match mode {
            CloudDeleteMode::Permanent => {
                self.delete(machine_id, request_id).await?;
                Ok(CloudDeleteReceipt {
                    machine_id: machine_id.to_owned(),
                    storage_tier: CloudStorageTier::Purged,
                    archive_id: None,
                    purge_at: None,
                    snapshot_residual: None,
                })
            }
            CloudDeleteMode::Recoverable => Err(CloudError::ProviderUnavailable(
                "recoverable cloud delete is unavailable".into(),
            )),
        }
    }
    async fn wake_cold(
        &self,
        _machine_id: &str,
        _request_id: &str,
        _params: &CloudColdWakeParams,
    ) -> Result<ProvisionedMachine, CloudError> {
        Err(CloudError::ProviderUnavailable(
            "cold cloud machine restore wake is unavailable".into(),
        ))
    }
    async fn resize_volume(
        &self,
        _machine_id: &str,
        _new_size_gb: u32,
        _request_id: &str,
    ) -> Result<CloudVolumeResizeReply, CloudError> {
        Err(CloudError::ProviderUnavailable(
            "cloud volume resize is unavailable".into(),
        ))
    }
    async fn renew_lease(
        &self,
        machine_id: &str,
        lease_id: &str,
        request_id: &str,
    ) -> Result<ProvisionedLease, CloudError>;
    async fn usage(&self) -> Result<CloudUsageSummary, CloudError>;
    /// List backend-known cloud machines for reconciling local-only and uncertain-create records.
    async fn list_backend_machines(
        &self,
    ) -> Result<Vec<super::reconcile::BackendMachineEntry>, CloudError> {
        let summary = self.usage().await?;
        Ok(super::reconcile::backend_machines_from_usage(&summary))
    }
    async fn redeem_promo(&self, _code: &str) -> Result<CloudPromoRedemption, CloudError> {
        Err(CloudError::ProviderUnavailable(
            "promo code redemption unavailable".into(),
        ))
    }
    async fn create_checkout(
        &self,
        _pack_id: &str,
        _amount_cents: Option<u64>,
    ) -> Result<CloudCheckoutSession, CloudError> {
        Err(CloudError::CheckoutUnavailable(
            "cloud credit checkout unavailable".into(),
        ))
    }
    /// Authoritative backend timestamp-only write for `seed_started` / `warm_console_ready`
    /// alongside `last_create_phases`. Returns `Ok(Some(phases))` when backed by Firebase.
    async fn record_warm_bootstrap_phases(
        &self,
        _machine_id: &str,
        _phase: super::warm_bootstrap::WarmBootstrapPhaseEvent,
        _request_id: &str,
    ) -> Result<Option<super::warm_bootstrap::BackendCreatePhases>, CloudError> {
        Ok(None)
    }
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
        endpoint: &CloudEndpoint,
    ) -> Result<(), CloudError>;
    async fn publish_direct_endpoint(
        &self,
        record: &CloudMachineRecord,
        endpoint: &CloudEndpoint,
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

    /// Post-enrollment warm first-console bootstrap over the authenticated same-account peer channel.
    /// Returns `Ok(None)` when the adapter does not perform warm seeding (e.g. unit-test stubs).
    async fn apply_warm_bootstrap(
        &self,
        _record: &CloudMachineRecord,
        _creation_generation: &str,
    ) -> Result<Option<super::warm_bootstrap::WarmBootstrapApplyResult>, CloudError> {
        Ok(None)
    }

    /// Queries or prepares per-provider owner sign-in card bindings on the target machine.
    async fn prepare_provider_signin(
        &self,
        _record: &CloudMachineRecord,
        _request: &super::warm_bootstrap::PrepareProviderSignInRequest,
    ) -> Result<super::warm_bootstrap::PrepareProviderSignInResponse, CloudError> {
        Ok(super::warm_bootstrap::PrepareProviderSignInResponse::default())
    }
}
