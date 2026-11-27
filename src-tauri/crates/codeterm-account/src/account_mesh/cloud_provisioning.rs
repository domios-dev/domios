use std::collections::BTreeMap;
use std::fmt;
use std::net::{Ipv4Addr, SocketAddrV4};

use codeterm_util::cloud_endpoint::provider_proxy_host_allowed;
pub use codeterm_util::cloud_endpoint::{CloudEndpoint, CLOUD_INGRESS_PORT, PROVIDER_PROXY_PORT};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

pub const ROUTE_CLOUD_CREATE: &str = "createManagedCloudMachine";
pub const ROUTE_CLOUD_WAKE: &str = "wakeManagedCloudMachine";
pub const ROUTE_CLOUD_SLEEP: &str = "sleepManagedCloudMachine";
pub const ROUTE_CLOUD_ARCHIVE: &str = "archiveManagedCloudMachineNow";
pub const ROUTE_CLOUD_DELETE: &str = "deleteManagedCloudMachine";
pub const ROUTE_CLOUD_WARM_PHASES: &str = "recordWarmBootstrapPhases";
pub const ROUTE_CLOUD_LEASE_RENEW: &str = "renewManagedCloudLease";
pub const ROUTE_CLOUD_USAGE: &str = "getManagedCloudUsage";
pub const ROUTE_CLOUD_REDEEM_PROMO: &str = "redeemCloudCreditPromo";
pub const ROUTE_CLOUD_CHECKOUT: &str = "createCloudCreditPackCheckout";
pub const ROUTE_CLOUD_VOLUME_RESIZE: &str = "resizeManagedCloudMachineVolume";

pub const DOMAIN_CLOUD_CREATE: &str = "domios.cloud-create.v1";
pub const DOMAIN_CLOUD_WAKE: &str = "domios.cloud-wake.v1";
pub const DOMAIN_CLOUD_SLEEP: &str = "domios.cloud-sleep.v1";
pub const DOMAIN_CLOUD_ARCHIVE: &str = "domios.cloud-archive.v1";
pub const DOMAIN_CLOUD_DELETE: &str = "domios.cloud-delete.v1";
pub const DOMAIN_CLOUD_WARM_PHASES: &str = "domios.cloud-warm-bootstrap-phases.v1";
pub const DOMAIN_CLOUD_LEASE_RENEW: &str = "domios.cloud-lease-renew.v1";
pub const DOMAIN_CLOUD_VOLUME_RESIZE: &str = "domios.cloud-volume-resize.v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WireCloudEndpoint {
    DedicatedIpv4 {
        ipv4: String,
        port: u16,
    },
    ProviderProxy {
        host: String,
        port: u16,
        scheme: String,
    },
}

impl WireCloudEndpoint {
    pub fn validate(&self) -> Option<CloudEndpoint> {
        match self {
            Self::DedicatedIpv4 { ipv4, port } => {
                if *port != CLOUD_INGRESS_PORT {
                    return None;
                }
                let ip: Ipv4Addr = ipv4.parse().ok()?;
                Some(CloudEndpoint::Direct(SocketAddrV4::new(ip, *port)))
            }
            Self::ProviderProxy { host, port, scheme } => {
                if *port != PROVIDER_PROXY_PORT
                    || scheme != "wss"
                    || !provider_proxy_host_allowed(host)
                {
                    return None;
                }
                Some(CloudEndpoint::ProviderProxy { host: host.clone() })
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireCloudSleepMode {
    Pause,
    Stop,
    StopBilled,
    DeleteRecreate,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireCloudManagedRate {
    pub amount: f64,
    pub currency: String,
    pub basis: String,
    pub credits_per_hour: f64,
}

impl WireCloudManagedRate {
    pub fn is_valid(&self) -> bool {
        self.basis == "managed"
            && self.amount.is_finite()
            && self.amount >= 0.0
            && !self.currency.trim().is_empty()
            && self.credits_per_hour.is_finite()
            && self.credits_per_hour > 0.0
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateCloudMachineBody {
    pub schema_version: u8,
    pub controller_installation_id: String,
    pub controller_key: String,
    pub creation_psk: String,
    pub size: String,
    pub request_id: String,
    pub timestamp: u64,
}

impl Drop for CreateCloudMachineBody {
    fn drop(&mut self) {
        self.creation_psk.zeroize();
    }
}

impl fmt::Debug for CreateCloudMachineBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreateCloudMachineBody")
            .field("schema_version", &self.schema_version)
            .field(
                "controller_installation_id",
                &self.controller_installation_id,
            )
            .field("controller_key", &self.controller_key)
            .field("creation_psk", &"<redacted>")
            .field("size", &self.size)
            .field("request_id", &self.request_id)
            .field("timestamp", &self.timestamp)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateCloudMachineRequest {
    pub body: CreateCloudMachineBody,
    pub controller_signature: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireCloudStorageTier {
    Hot,
    Evicting,
    Cold,
    RecoverableDeleted,
    Purged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireCloudDeleteMode {
    Permanent,
    Recoverable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireProviderSnapshotResidual {
    pub provider: String,
    pub snapshots_remaining: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub earliest_expiry_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_expiry_at: Option<u64>,
    pub auto_backup_enabled: bool,
    pub individual_delete_supported: bool,
    pub erasure_state: String,
    pub checked_at: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireCloudMachineStorageUsage {
    pub machine_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<u64>,
    pub tier: WireCloudStorageTier,
    pub allocated_gb: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_gb: Option<f64>,
    pub hot_monthly_cost_cents: u64,
    pub hot_accrued_cents: f64,
    pub cold_included: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hot_until_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cold_since_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purge_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_bytes: Option<u64>,
    pub archive_generation: u64,
    pub disk_full: bool,
    pub volume_upgrade_max_gb: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_residual: Option<WireProviderSnapshotResidual>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireCloudStorageUsageSummary {
    pub hot_gb_month_rate_cents: u64,
    pub hot_days: f64,
    pub purge_days: f64,
    pub purge_warning_days: [f64; 2],
    pub cold_archive_included: bool,
    pub total_allocated_gb: u32,
    pub total_used_bytes: u64,
    pub total_hot_monthly_cost_cents: u64,
    #[serde(default)]
    pub machines: Vec<WireCloudMachineStorageUsage>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedCloudMachineActionBody {
    pub schema_version: u8,
    pub controller_installation_id: String,
    pub machine_id: String,
    pub request_id: String,
    pub timestamp: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<WireCloudDeleteMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creation_psk: Option<String>,
}

impl Drop for ManagedCloudMachineActionBody {
    fn drop(&mut self) {
        if let Some(psk) = self.creation_psk.as_mut() {
            psk.zeroize();
        }
    }
}

impl fmt::Debug for ManagedCloudMachineActionBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ManagedCloudMachineActionBody")
            .field("schema_version", &self.schema_version)
            .field(
                "controller_installation_id",
                &self.controller_installation_id,
            )
            .field("machine_id", &self.machine_id)
            .field("request_id", &self.request_id)
            .field("timestamp", &self.timestamp)
            .field("mode", &self.mode)
            .field("archive_id", &self.archive_id)
            .field("controller_key", &self.controller_key)
            .field(
                "creation_psk",
                &self.creation_psk.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedCloudMachineActionRequest {
    pub body: ManagedCloudMachineActionBody,
    pub controller_signature: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireCloudCreditWarnings {
    #[serde(default)]
    pub epoch: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pct80_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pct95_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exhausted_at: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProvisionedMachineReply {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warnings: Option<WireCloudCreditWarnings>,
    pub machine_id: String,
    pub provider: String,
    pub endpoint: WireCloudEndpoint,
    pub size: String,
    pub rate: WireCloudManagedRate,
    pub sleep_mode: WireCloudSleepMode,
    pub state: String,
    pub lease_id: String,
    pub lease_expires_at: u64,
    #[serde(
        default,
        deserialize_with = "nonzero_timestamp",
        skip_serializing_if = "Option::is_none"
    )]
    pub ingress_ready_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cold_restore: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_generation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_installation_revoked: Option<bool>,
}

impl ProvisionedMachineReply {
    pub fn validated_endpoint(&self) -> Option<CloudEndpoint> {
        if self.state != "awake"
            || self.machine_id.trim().is_empty()
            || self.provider.trim().is_empty()
            || self.size.trim().is_empty()
            || self.lease_id.trim().is_empty()
            || !self.rate.is_valid()
        {
            return None;
        }
        self.endpoint.validate()
    }
}

fn nonzero_timestamp<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u64>, D::Error> {
    Ok(Option::<u64>::deserialize(deserializer)?.filter(|time| *time != 0))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SleepMachineReply {
    pub machine_id: String,
    pub sleep_mode: WireCloudSleepMode,
    pub state: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveMachineReply {
    pub machine_id: String,
    pub state: String,
    pub storage_tier: WireCloudStorageTier,
    pub archive_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cold_since_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purge_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_generation: Option<u64>,
    #[serde(default)]
    pub already_archived: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_residual: Option<WireProviderSnapshotResidual>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteMachineReply {
    pub machine_id: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_tier: Option<WireCloudStorageTier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purge_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_residual: Option<WireProviderSnapshotResidual>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResizeCloudVolumeBody {
    pub schema_version: u8,
    pub controller_installation_id: String,
    pub machine_id: String,
    pub new_size_gb: u32,
    pub confirmed_by_owner: bool,
    pub request_id: String,
    pub timestamp: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResizeCloudVolumeRequest {
    pub body: ResizeCloudVolumeBody,
    pub controller_signature: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResizeCloudVolumeReply {
    pub machine_id: String,
    pub allocated_gb: u32,
    pub needs_restart: bool,
    pub hot_monthly_cost_cents: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireWarmBootstrapPhase {
    SeedStarted,
    WarmConsoleReady,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireCloudPhaseTimestamps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allocated: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipv4_bound: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enrolled: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_ready: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed_started: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warm_console_ready: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordWarmBootstrapPhasesBody {
    pub schema_version: u8,
    pub controller_installation_id: String,
    pub machine_id: String,
    pub phase: WireWarmBootstrapPhase,
    pub request_id: String,
    pub timestamp: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordWarmBootstrapPhasesRequest {
    pub body: RecordWarmBootstrapPhasesBody,
    pub controller_signature: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecordWarmBootstrapPhasesReply {
    pub machine_id: String,
    pub last_create_phases: WireCloudPhaseTimestamps,
    #[serde(default)]
    pub warm_bootstrap_delta_secs: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenewLeaseBody {
    pub schema_version: u8,
    pub controller_installation_id: String,
    pub machine_id: String,
    pub lease_id: String,
    pub request_id: String,
    pub timestamp: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenewLeaseRequest {
    pub body: RenewLeaseBody,
    pub controller_signature: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenewLeaseReply {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warnings: Option<WireCloudCreditWarnings>,
    pub machine_id: String,
    pub lease_id: String,
    pub lease_expires_at: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireCloudUsageSize {
    pub id: String,
    pub cpu: u32,
    pub ram_gb: f64,
    pub hourly_rate_cents: u64,
    pub credits_per_hour: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireCloudUsagePack {
    pub pack_id: String,
    pub price_cents: u64,
    pub hours: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireCloudPlanAllowances {
    pub free: f64,
    pub pro: f64,
    pub max: f64,
    pub sponsor: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CloudUsageReply {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warnings: Option<WireCloudCreditWarnings>,
    pub used: f64,
    pub allowance: f64,
    pub purchased: f64,
    pub remaining: f64,
    pub cap: f64,
    #[serde(default)]
    pub burn_per_size: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period_end_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_min_cents: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_max_cents: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sizes: Option<Vec<WireCloudUsageSize>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packs: Option<Vec<WireCloudUsagePack>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purchased_expiry_days: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowance_hours: Option<WireCloudPlanAllowances>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<WireCloudStorageUsageSummary>,
}

/// Wire request for `createCloudCreditPackCheckout` (`shared/cloud-credits.ts`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateCloudCreditPackCheckoutRequest {
    pub pack_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount_cents: Option<u64>,
}

/// Wire reply for `createCloudCreditPackCheckout` (`shared/cloud-credits.ts::CreateCloudCreditPackCheckoutReply`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreateCloudCreditPackCheckoutReply {
    pub url: String,
    pub session_id: String,
    pub pack_id: String,
    pub credits: f64,
    pub amount_cents: u64,
    pub currency: String,
}

impl CreateCloudCreditPackCheckoutReply {
    pub fn is_valid(&self) -> bool {
        url::Url::parse(&self.url)
            .is_ok_and(|url| url.scheme() == "https" && url.host_str().is_some())
            && !self.session_id.trim().is_empty()
            && !self.pack_id.trim().is_empty()
            && self.credits.is_finite()
            && self.credits >= 0.0
            && self.amount_cents > 0
            && !self.currency.trim().is_empty()
    }
}

/// Typed refusal detail for `createCloudCreditPackCheckout`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", deny_unknown_fields)]
pub enum CloudCheckoutRefusal {
    #[serde(rename = "invalid")]
    Invalid,
    #[serde(rename = "spend_cap_exceeded")]
    SpendCapExceeded {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cap_cents: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spent_cents: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pending_cents: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pack_cents: Option<u64>,
    },
    #[serde(rename = "below_min")]
    BelowMin,
    #[serde(rename = "above_max")]
    AboveMax,
    #[serde(rename = "territory")]
    Territory,
    #[serde(rename = "unavailable")]
    Unavailable,
    #[serde(rename = "rate_limited")]
    RateLimited {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_after_seconds: Option<u64>,
    },
}

/// Wire request for `redeemCloudCreditPromo` (`shared/cloud-credits.ts::RedeemCloudCreditPromoRequest`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedeemCloudCreditPromoRequest {
    pub code: String,
}

/// Wire reply for `redeemCloudCreditPromo` (`shared/cloud-credits.ts::RedeemCloudCreditPromoReply`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RedeemCloudCreditPromoReply {
    pub schema_version: u8,
    pub status: String,
    pub already_redeemed: bool,
    pub hours: f64,
    pub expires_at: Option<u64>,
    pub remaining: f64,
    pub purchased: f64,
}

impl RedeemCloudCreditPromoReply {
    pub fn is_valid(&self) -> bool {
        self.schema_version == 1
            && self.status == "redeemed"
            && self.hours.is_finite()
            && self.hours >= 0.0
            && self.remaining.is_finite()
            && self.remaining >= 0.0
            && self.purchased.is_finite()
            && self.purchased >= 0.0
    }
}

/// Typed refusal detail matching `shared/cloud-credits.ts::PromoRedeemRefusal`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", deny_unknown_fields)]
pub enum PromoRedeemRefusal {
    #[serde(rename = "invalid_code")]
    InvalidCode,
    #[serde(rename = "expired")]
    Expired,
    #[serde(rename = "disabled")]
    Disabled,
    #[serde(rename = "exhausted")]
    Exhausted,
    #[serde(rename = "spend_cap_exceeded")]
    SpendCapExceeded,
    #[serde(rename = "rate_limited")]
    RateLimited {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_after_seconds: Option<u64>,
    },
}

/// Typed refusal detail matching `shared/cloud-provisioning.ts::CloudProvisioningRefusal`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", deny_unknown_fields)]
pub enum CloudProvisioningRefusal {
    #[serde(rename = "invalid")]
    Invalid,
    #[serde(rename = "unauthenticated")]
    Unauthenticated,
    #[serde(rename = "app-check")]
    AppCheck,
    #[serde(rename = "session_revoked")]
    SessionRevoked,
    #[serde(rename = "forbidden")]
    Forbidden,
    #[serde(rename = "revoked")]
    Revoked,
    #[serde(rename = "proof_invalid")]
    ProofInvalid,
    #[serde(rename = "cloud.machines")]
    CloudMachines { limit: u32, active_count: u32 },
    #[serde(rename = "lease_required")]
    LeaseRequired,
    #[serde(rename = "credits_exhausted")]
    CreditsExhausted,
    #[serde(rename = "not_found")]
    NotFound,
    #[serde(rename = "conflict")]
    Conflict,
    #[serde(rename = "machine_busy")]
    MachineBusy,
    #[serde(rename = "replay")]
    Replay,
    #[serde(rename = "rate-limited")]
    RateLimited { retry_after_seconds: u64 },
    #[serde(rename = "endpoint_kind_unsupported")]
    EndpointKindUnsupported,
    #[serde(rename = "provider_refused")]
    ProviderRefused {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_reason: Option<String>,
    },
    #[serde(rename = "provider_unavailable")]
    ProviderUnavailable,
    #[serde(rename = "provider_unconfirmed")]
    ProviderUnconfirmed,
    #[serde(rename = "provider_starting")]
    ProviderStarting,
    #[serde(rename = "authority-unavailable")]
    AuthorityUnavailable,
    #[serde(rename = "unavailable")]
    Unavailable,
}

pub fn provisioning_message<T: Serialize>(
    domain: &str,
    project: &str,
    account_id: &str,
    body: &T,
) -> Option<Vec<u8>> {
    let value = serde_json::json!({
        "domain": domain,
        "project": project,
        "account_id": account_id,
        "body": body,
    });
    codeterm_entitlements::jcs::encode(&value)
        .ok()
        .map(String::into_bytes)
}
