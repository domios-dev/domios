use std::collections::BTreeMap;
use std::fmt;
use std::net::{Ipv4Addr, SocketAddrV4};

use codeterm_util::cloud_endpoint::provider_proxy_host_allowed;
pub use codeterm_util::cloud_endpoint::{CloudEndpoint, CLOUD_INGRESS_PORT, PROVIDER_PROXY_PORT};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use super::cloud_enrollment::{fixed_bytes, CloudController};
use super::state::AccountMeshRefusal;

pub const ROUTE_CLOUD_CREATE: &str = "createManagedCloudMachine";
pub const ROUTE_CLOUD_WAKE: &str = "wakeManagedCloudMachine";
pub const ROUTE_CLOUD_SLEEP: &str = "sleepManagedCloudMachine";
pub const ROUTE_CLOUD_DELETE: &str = "deleteManagedCloudMachine";
pub const ROUTE_CLOUD_LEASE_RENEW: &str = "renewManagedCloudLease";
pub const ROUTE_CLOUD_USAGE: &str = "getManagedCloudUsage";
pub const ROUTE_CLOUD_REDEEM_PROMO: &str = "redeemCloudCreditPromo";
pub const ROUTE_CLOUD_CHECKOUT: &str = "createCloudCreditPackCheckout";
pub const ROUTE_CLOUD_VOLUME_RESIZE: &str = "resizeManagedCloudMachineVolume";

pub const DOMAIN_CLOUD_CREATE: &str = "domios.cloud-create.v1";
pub const DOMAIN_CLOUD_WAKE: &str = "domios.cloud-wake.v1";
pub const DOMAIN_CLOUD_SLEEP: &str = "domios.cloud-sleep.v1";
pub const DOMAIN_CLOUD_DELETE: &str = "domios.cloud-delete.v1";
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct WireCloudMachineStorageUsage {
    pub machine_id: String,
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
#[serde(deny_unknown_fields)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvisionedMachineReply {
    pub machine_id: String,
    pub provider: String,
    pub endpoint: WireCloudEndpoint,
    pub size: String,
    pub rate: WireCloudManagedRate,
    pub sleep_mode: WireCloudSleepMode,
    pub state: String,
    pub lease_id: String,
    pub lease_expires_at: u64,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SleepMachineReply {
    pub machine_id: String,
    pub sleep_mode: WireCloudSleepMode,
    pub state: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct ResizeCloudVolumeReply {
    pub machine_id: String,
    pub allocated_gb: u32,
    pub needs_restart: bool,
    pub hot_monthly_cost_cents: u64,
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
#[serde(deny_unknown_fields)]
pub struct RenewLeaseReply {
    pub machine_id: String,
    pub lease_id: String,
    pub lease_expires_at: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireCloudUsageSize {
    pub id: String,
    pub cpu: u32,
    pub ram_gb: f64,
    pub hourly_rate_cents: u64,
    pub credits_per_hour: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireCloudUsagePack {
    pub pack_id: String,
    pub price_cents: u64,
    pub hours: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireCloudPlanAllowances {
    pub free: f64,
    pub pro: f64,
    pub max: f64,
    pub sponsor: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudUsageReply {
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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

impl CloudController<'_> {
    fn ensure_current(&self) -> Result<(), AccountMeshRefusal> {
        let view = self.mesh.state();
        if view.account_epoch == self.signed.epoch
            && view.current_membership(self.mesh.now()).is_some()
        {
            Ok(())
        } else {
            Err(AccountMeshRefusal::Cancelled)
        }
    }

    pub async fn create_machine(
        &self,
        size: &str,
        controller_key: &str,
        creation_psk: &str,
        request_id: &str,
    ) -> Result<ProvisionedMachineReply, AccountMeshRefusal> {
        self.ensure_current()?;
        if fixed_bytes::<32>(controller_key).is_none()
            || fixed_bytes::<32>(creation_psk).is_none()
            || fixed_bytes::<32>(request_id).is_none()
            || size.trim().is_empty()
        {
            return Err(AccountMeshRefusal::Invalid);
        }
        let body = CreateCloudMachineBody {
            schema_version: 1,
            controller_installation_id: self.mesh.installation(),
            controller_key: controller_key.to_owned(),
            creation_psk: creation_psk.to_owned(),
            size: size.trim().to_owned(),
            request_id: request_id.to_owned(),
            timestamp: self.mesh.now(),
        };
        let message = provisioning_message(
            DOMAIN_CLOUD_CREATE,
            self.signed.trust.project(),
            &self.signed.account,
            &body,
        )
        .ok_or(AccountMeshRefusal::Invalid)?;
        let controller_signature = self.signed.key.sign_cloud_provisioning(&message);
        let request = CreateCloudMachineRequest {
            body,
            controller_signature,
        };
        let reply = self
            .mesh
            .call(
                self.mesh
                    .ports
                    .registry
                    .create_cloud_machine(&self.signed.session, &request),
            )
            .await?;
        drop(request);
        self.ensure_current()?;
        if reply.validated_endpoint().is_none() {
            return Err(AccountMeshRefusal::Unavailable);
        }
        Ok(reply)
    }

    fn build_action_request(
        &self,
        domain: &str,
        machine_id: &str,
        request_id: &str,
    ) -> Result<ManagedCloudMachineActionRequest, AccountMeshRefusal> {
        self.ensure_current()?;
        if machine_id.trim().is_empty() || fixed_bytes::<32>(request_id).is_none() {
            return Err(AccountMeshRefusal::Invalid);
        }
        let body = ManagedCloudMachineActionBody {
            schema_version: 1,
            controller_installation_id: self.mesh.installation(),
            machine_id: machine_id.trim().to_owned(),
            request_id: request_id.to_owned(),
            timestamp: self.mesh.now(),
            mode: None,
            archive_id: None,
            controller_key: None,
            creation_psk: None,
        };
        let message = provisioning_message(
            domain,
            self.signed.trust.project(),
            &self.signed.account,
            &body,
        )
        .ok_or(AccountMeshRefusal::Invalid)?;
        let controller_signature = self.signed.key.sign_cloud_provisioning(&message);
        Ok(ManagedCloudMachineActionRequest {
            body,
            controller_signature,
        })
    }

    pub async fn wake_machine(
        &self,
        machine_id: &str,
        request_id: &str,
    ) -> Result<ProvisionedMachineReply, AccountMeshRefusal> {
        let request = self.build_action_request(DOMAIN_CLOUD_WAKE, machine_id, request_id)?;
        let reply = self
            .mesh
            .call(
                self.mesh
                    .ports
                    .registry
                    .wake_cloud_machine(&self.signed.session, &request),
            )
            .await?;
        self.ensure_current()?;
        if reply.validated_endpoint().is_none() {
            return Err(AccountMeshRefusal::Unavailable);
        }
        Ok(reply)
    }

    pub async fn sleep_machine(
        &self,
        machine_id: &str,
        request_id: &str,
    ) -> Result<SleepMachineReply, AccountMeshRefusal> {
        let request = self.build_action_request(DOMAIN_CLOUD_SLEEP, machine_id, request_id)?;
        let reply = self
            .mesh
            .call(
                self.mesh
                    .ports
                    .registry
                    .sleep_cloud_machine(&self.signed.session, &request),
            )
            .await?;
        self.ensure_current()?;
        if reply.state != "asleep" || reply.machine_id != machine_id.trim() {
            return Err(AccountMeshRefusal::Unavailable);
        }
        Ok(reply)
    }

    pub async fn delete_machine(
        &self,
        machine_id: &str,
        request_id: &str,
    ) -> Result<DeleteMachineReply, AccountMeshRefusal> {
        self.delete_machine_with_mode(machine_id, request_id, None, None)
            .await
    }

    pub async fn delete_machine_with_mode(
        &self,
        machine_id: &str,
        request_id: &str,
        mode: Option<WireCloudDeleteMode>,
        archive_id: Option<&str>,
    ) -> Result<DeleteMachineReply, AccountMeshRefusal> {
        self.ensure_current()?;
        if machine_id.trim().is_empty() || fixed_bytes::<32>(request_id).is_none() {
            return Err(AccountMeshRefusal::Invalid);
        }
        let body = ManagedCloudMachineActionBody {
            schema_version: 1,
            controller_installation_id: self.mesh.installation(),
            machine_id: machine_id.trim().to_owned(),
            request_id: request_id.to_owned(),
            timestamp: self.mesh.now(),
            mode,
            archive_id: archive_id
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty()),
            controller_key: None,
            creation_psk: None,
        };
        let message = provisioning_message(
            DOMAIN_CLOUD_DELETE,
            self.signed.trust.project(),
            &self.signed.account,
            &body,
        )
        .ok_or(AccountMeshRefusal::Invalid)?;
        let controller_signature = self.signed.key.sign_cloud_provisioning(&message);
        let request = ManagedCloudMachineActionRequest {
            body,
            controller_signature,
        };
        let reply = self
            .mesh
            .call(
                self.mesh
                    .ports
                    .registry
                    .delete_cloud_machine(&self.signed.session, &request),
            )
            .await?;
        self.ensure_current()?;
        if reply.state != "deleted" || reply.machine_id != machine_id.trim() {
            return Err(AccountMeshRefusal::Unavailable);
        }
        Ok(reply)
    }

    pub async fn wake_machine_cold(
        &self,
        machine_id: &str,
        request_id: &str,
        controller_key: &str,
        creation_psk: &str,
        archive_id: Option<&str>,
    ) -> Result<ProvisionedMachineReply, AccountMeshRefusal> {
        self.ensure_current()?;
        if machine_id.trim().is_empty()
            || fixed_bytes::<32>(request_id).is_none()
            || fixed_bytes::<32>(controller_key).is_none()
            || fixed_bytes::<32>(creation_psk).is_none()
        {
            return Err(AccountMeshRefusal::Invalid);
        }
        let body = ManagedCloudMachineActionBody {
            schema_version: 1,
            controller_installation_id: self.mesh.installation(),
            machine_id: machine_id.trim().to_owned(),
            request_id: request_id.to_owned(),
            timestamp: self.mesh.now(),
            mode: None,
            archive_id: archive_id
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty()),
            controller_key: Some(controller_key.to_owned()),
            creation_psk: Some(creation_psk.to_owned()),
        };
        let message = provisioning_message(
            DOMAIN_CLOUD_WAKE,
            self.signed.trust.project(),
            &self.signed.account,
            &body,
        )
        .ok_or(AccountMeshRefusal::Invalid)?;
        let controller_signature = self.signed.key.sign_cloud_provisioning(&message);
        let request = ManagedCloudMachineActionRequest {
            body,
            controller_signature,
        };
        let reply = self
            .mesh
            .call(
                self.mesh
                    .ports
                    .registry
                    .wake_cloud_machine(&self.signed.session, &request),
            )
            .await?;
        drop(request);
        self.ensure_current()?;
        if reply.validated_endpoint().is_none() {
            return Err(AccountMeshRefusal::Unavailable);
        }
        Ok(reply)
    }

    pub async fn resize_volume(
        &self,
        machine_id: &str,
        new_size_gb: u32,
        confirmed_by_owner: bool,
        request_id: &str,
    ) -> Result<ResizeCloudVolumeReply, AccountMeshRefusal> {
        self.ensure_current()?;
        if !confirmed_by_owner
            || machine_id.trim().is_empty()
            || new_size_gb == 0
            || new_size_gb > 500
            || fixed_bytes::<32>(request_id).is_none()
        {
            return Err(AccountMeshRefusal::Invalid);
        }
        let body = ResizeCloudVolumeBody {
            schema_version: 1,
            controller_installation_id: self.mesh.installation(),
            machine_id: machine_id.trim().to_owned(),
            new_size_gb,
            confirmed_by_owner: true,
            request_id: request_id.to_owned(),
            timestamp: self.mesh.now(),
        };
        let message = provisioning_message(
            DOMAIN_CLOUD_VOLUME_RESIZE,
            self.signed.trust.project(),
            &self.signed.account,
            &body,
        )
        .ok_or(AccountMeshRefusal::Invalid)?;
        let controller_signature = self.signed.key.sign_cloud_provisioning(&message);
        let request = ResizeCloudVolumeRequest {
            body,
            controller_signature,
        };
        let reply = self
            .mesh
            .call(
                self.mesh
                    .ports
                    .registry
                    .resize_cloud_volume(&self.signed.session, &request),
            )
            .await?;
        self.ensure_current()?;
        Ok(reply)
    }

    pub async fn renew_lease(
        &self,
        machine_id: &str,
        lease_id: &str,
        request_id: &str,
    ) -> Result<RenewLeaseReply, AccountMeshRefusal> {
        self.ensure_current()?;
        if machine_id.trim().is_empty()
            || lease_id.trim().is_empty()
            || fixed_bytes::<32>(request_id).is_none()
        {
            return Err(AccountMeshRefusal::Invalid);
        }
        let body = RenewLeaseBody {
            schema_version: 1,
            controller_installation_id: self.mesh.installation(),
            machine_id: machine_id.trim().to_owned(),
            lease_id: lease_id.trim().to_owned(),
            request_id: request_id.to_owned(),
            timestamp: self.mesh.now(),
        };
        let message = provisioning_message(
            DOMAIN_CLOUD_LEASE_RENEW,
            self.signed.trust.project(),
            &self.signed.account,
            &body,
        )
        .ok_or(AccountMeshRefusal::Invalid)?;
        let controller_signature = self.signed.key.sign_cloud_provisioning(&message);
        let request = RenewLeaseRequest {
            body,
            controller_signature,
        };
        let reply = self
            .mesh
            .call(
                self.mesh
                    .ports
                    .registry
                    .renew_cloud_lease(&self.signed.session, &request),
            )
            .await?;
        self.ensure_current()?;
        Ok(reply)
    }

    pub async fn cloud_usage(&self) -> Result<CloudUsageReply, AccountMeshRefusal> {
        self.ensure_current()?;
        let reply = self
            .mesh
            .call(self.mesh.ports.registry.cloud_usage(&self.signed.session))
            .await?;
        self.ensure_current()?;
        Ok(reply)
    }

    pub async fn redeem_promo(
        &self,
        code: &str,
    ) -> Result<RedeemCloudCreditPromoReply, AccountMeshRefusal> {
        self.ensure_current()?;
        let trimmed = code.trim();
        if trimmed.is_empty() || trimmed.len() > 64 {
            return Err(AccountMeshRefusal::PromoInvalidCode);
        }
        let request = RedeemCloudCreditPromoRequest {
            code: trimmed.to_owned(),
        };
        let reply = self
            .mesh
            .call(
                self.mesh
                    .ports
                    .registry
                    .redeem_cloud_promo(&self.signed.session, &request),
            )
            .await?;
        self.ensure_current()?;
        if !reply.is_valid() {
            return Err(AccountMeshRefusal::Unavailable);
        }
        Ok(reply)
    }

    pub async fn create_checkout(
        &self,
        pack_id: &str,
        amount_cents: Option<u64>,
    ) -> Result<CreateCloudCreditPackCheckoutReply, AccountMeshRefusal> {
        self.ensure_current()?;
        let pack_id = pack_id.trim();
        if pack_id.is_empty() {
            return Err(AccountMeshRefusal::Invalid);
        }
        if pack_id == "custom" {
            if !amount_cents.is_some_and(|cents| cents > 0) {
                return Err(AccountMeshRefusal::Invalid);
            }
        } else if amount_cents.is_some() {
            return Err(AccountMeshRefusal::Invalid);
        }
        let request = CreateCloudCreditPackCheckoutRequest {
            pack_id: pack_id.to_owned(),
            amount_cents,
        };
        let reply = self
            .mesh
            .call(
                self.mesh
                    .ports
                    .registry
                    .create_cloud_checkout(&self.signed.session, &request),
            )
            .await?;
        self.ensure_current()?;
        if !reply.is_valid() {
            return Err(AccountMeshRefusal::Unavailable);
        }
        Ok(reply)
    }
}
