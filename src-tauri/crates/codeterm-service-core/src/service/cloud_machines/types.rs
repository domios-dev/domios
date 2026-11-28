use std::collections::BTreeMap;

pub use codeterm_util::cloud_endpoint::{CloudEndpoint, CLOUD_INGRESS_PORT, PROVIDER_PROXY_PORT};
use serde::{Deserialize, Serialize};

use super::storage::{CloudStorageTier, CloudStorageUsageSummary, AUX_KEY_STORAGE_TIER};

pub const DEFAULT_IDLE_SLEEP_MINUTES: u32 = 15;
pub const DEFAULT_LEASE_RENEW_LEAD_MS: u64 = 120_000;
pub const CLOUD_WAKE_SEND_PREFIX: &str = "cloud-wake:";
pub const COST_WARNING_TEXT: &str =
    "This machine costs money while awake; reuse it before creating another and let it sleep when idle.";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudLifecycleState {
    Creating,
    Enrolling,
    Awake,
    Draining,
    Asleep,
    Waking,
    OutOfCredits,
    Failed,
    Deleted,
}

impl CloudLifecycleState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Creating => "creating",
            Self::Enrolling => "enrolling",
            Self::Awake => "awake",
            Self::Draining => "draining",
            Self::Asleep => "asleep",
            Self::Waking => "waking",
            Self::OutOfCredits => "out_of_credits",
            Self::Failed => "failed",
            Self::Deleted => "deleted",
        }
    }

    pub fn counts_against_limit(self) -> bool {
        !matches!(self, Self::Deleted)
    }

    pub fn is_awake(self) -> bool {
        matches!(self, Self::Awake)
    }

    pub fn needs_wake_for_use(self) -> bool {
        matches!(self, Self::Asleep | Self::Draining | Self::Waking)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudSleepMode {
    Pause,
    #[default]
    Stop,
    StopBilled,
    DeleteRecreate,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CloudRate {
    pub amount: f64,
    pub currency: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credits_per_hour: Option<f64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingProvisioningOp {
    Create,
    Wake,
    Sleep,
    Archive,
    Delete,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PendingProvisioningRequest {
    pub op: PendingProvisioningOp,
    pub request_id: String,
    pub started_at_ms: u64,
    #[serde(default)]
    pub unconfirmed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CloudUsageSize {
    pub id: String,
    pub cpu: u32,
    pub ram_gb: f64,
    pub hourly_rate_cents: u64,
    pub credits_per_hour: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CloudUsagePack {
    pub pack_id: String,
    pub price_cents: u64,
    pub hours: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CloudPlanAllowances {
    pub free: f64,
    pub pro: f64,
    pub max: f64,
    pub sponsor: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CloudUsageSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warnings:
        Option<codeterm_account::account_mesh::cloud_provisioning::WireCloudCreditWarnings>,
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
    pub sizes: Option<Vec<CloudUsageSize>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packs: Option<Vec<CloudUsagePack>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purchased_expiry_days: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowance_hours: Option<CloudPlanAllowances>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<CloudStorageUsageSummary>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CloudCheckoutSession {
    pub url: String,
    pub session_id: String,
    pub pack_id: String,
    pub credits: f64,
    pub amount_cents: u64,
    pub currency: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CloudPromoRedemption {
    pub schema_version: u8,
    pub status: String,
    pub already_redeemed: bool,
    pub hours: f64,
    pub expires_at: Option<u64>,
    pub remaining: f64,
    pub purchased: f64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CloudOwnershipReceipt {
    pub provider_machine_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floating_ip_id: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub auxiliary_resource_ids: BTreeMap<String, String>,
    pub controller_id: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CloudMachineRecord {
    pub machine_id: String,
    pub name: String,
    pub provider: String,
    pub size: String,
    pub state: CloudLifecycleState,
    #[serde(default)]
    pub sleep_mode: CloudSleepMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_instance_id: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "endpoint_addr",
        deserialize_with = "codeterm_util::cloud_endpoint::deserialize_optional_cloud_endpoint"
    )]
    pub endpoint: Option<CloudEndpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_per_hour: Option<CloudRate>,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_awake_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_owner_interaction_at_ms: Option<u64>,
    #[serde(default)]
    pub accumulated_awake_secs: u64,
    pub period_start_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_expires_at_ms: Option<u64>,
    #[serde(default)]
    pub exhaustion_pending: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_since_ms: Option<u64>,
    #[serde(default)]
    pub apply_unconfirmed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_request: Option<PendingProvisioningRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub ownership: CloudOwnershipReceipt,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CloudCostSummary {
    pub kind: String,
    pub machine_id: String,
    pub name: String,
    pub provider: String,
    pub size: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_per_hour: Option<CloudRate>,
    pub state: CloudLifecycleState,
    pub sleep_mode: CloudSleepMode,
    pub uptime_secs: u64,
    pub billing_notice: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CloudReceipt {
    pub kind: String,
    pub machine_id: String,
    pub name: String,
    pub provider: String,
    pub size: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_per_hour: Option<CloudRate>,
    pub state: CloudLifecycleState,
    pub sleep_mode: CloudSleepMode,
    pub uptime_secs: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_instance_id: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "endpoint_addr",
        deserialize_with = "codeterm_util::cloud_endpoint::deserialize_optional_cloud_endpoint"
    )]
    pub endpoint: Option<CloudEndpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_expires_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_tier: Option<CloudStorageTier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_at_ms: Option<u64>,
    pub billing_notice: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_phases: Option<super::warm_bootstrap::CloudCreatePhases>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warm_bootstrap: Option<super::warm_bootstrap::CloudWarmBootstrapSummary>,
}

impl CloudMachineRecord {
    pub fn uptime_secs(&self, now_ms: u64) -> u64 {
        let live_secs = match (self.state, self.last_awake_at_ms) {
            (CloudLifecycleState::Awake | CloudLifecycleState::Draining, Some(since)) => {
                now_ms.saturating_sub(since) / 1_000
            }
            _ => 0,
        };
        self.accumulated_awake_secs.saturating_add(live_secs)
    }

    /// Confirmed backend machine id, or `None` for local-only records that have not yet received
    /// or adopted a backend id. Never falls back to the local `machine_id`.
    pub fn provider_machine_id(&self) -> Option<&str> {
        let id = self.ownership.provider_machine_id.trim();
        (!id.is_empty()).then_some(id)
    }

    /// Creation request id associated with this record, if any.
    pub fn create_request_id(&self) -> Option<&str> {
        self.pending_request
            .as_ref()
            .filter(|p| p.op == PendingProvisioningOp::Create)
            .map(|p| p.request_id.trim())
            .filter(|id| !id.is_empty())
            .or_else(|| {
                self.ownership
                    .labels
                    .get(super::state::LABEL_CREATE_REQUEST_ID)
                    .map(|id| id.trim())
                    .filter(|id| !id.is_empty())
            })
    }

    pub fn cost_summary(&self, now_ms: u64) -> CloudCostSummary {
        CloudCostSummary {
            kind: "cloud".into(),
            machine_id: self.machine_id.clone(),
            name: self.name.clone(),
            provider: self.provider.clone(),
            size: self.size.clone(),
            rate_per_hour: self.rate_per_hour.clone(),
            state: self.state,
            sleep_mode: self.sleep_mode,
            uptime_secs: self.uptime_secs(now_ms),
            billing_notice: COST_WARNING_TEXT.into(),
        }
    }

    pub fn receipt(&self, now_ms: u64, warning: Option<String>) -> CloudReceipt {
        self.receipt_with_warm(now_ms, warning, None, None)
    }

    pub fn receipt_with_warm(
        &self,
        now_ms: u64,
        warning: Option<String>,
        create_phases: Option<super::warm_bootstrap::CloudCreatePhases>,
        warm_bootstrap: Option<super::warm_bootstrap::CloudWarmBootstrapSummary>,
    ) -> CloudReceipt {
        CloudReceipt {
            kind: "cloud".into(),
            machine_id: self.machine_id.clone(),
            name: self.name.clone(),
            provider: self.provider.clone(),
            size: self.size.clone(),
            rate_per_hour: self.rate_per_hour.clone(),
            state: self.state,
            sleep_mode: self.sleep_mode,
            uptime_secs: self.uptime_secs(now_ms),
            installation_id: self.installation_id.clone(),
            peer_instance_id: self.peer_instance_id.clone(),
            endpoint: self.endpoint.clone(),
            lease_expires_at_ms: self.lease_expires_at_ms,
            storage_tier: self
                .ownership
                .auxiliary_resource_ids
                .get(AUX_KEY_STORAGE_TIER)
                .and_then(|raw| CloudStorageTier::from_aux_str(raw)),
            created_at_ms: Some(self.created_at_ms),
            last_seen_at_ms: Some(
                self.last_awake_at_ms
                    .into_iter()
                    .chain(self.last_owner_interaction_at_ms)
                    .chain(std::iter::once(self.created_at_ms))
                    .max()
                    .unwrap_or(self.created_at_ms),
            ),
            billing_notice: COST_WARNING_TEXT.into(),
            warning: warning.or_else(|| {
                self.last_error
                    .as_deref()
                    .and_then(|error| error.strip_prefix(super::controller_wake::WAKE_UNVERIFIED))
                    .map(str::to_owned)
            }),
            create_phases,
            warm_bootstrap,
        }
    }

    pub fn matches_selector(&self, selector: &str) -> bool {
        let target = selector.trim();
        !target.is_empty()
            && (self.machine_id.eq_ignore_ascii_case(target)
                || self.name.eq_ignore_ascii_case(target)
                || self
                    .ownership
                    .provider_machine_id
                    .eq_ignore_ascii_case(target)
                || self
                    .installation_id
                    .as_deref()
                    .is_some_and(|id| id.eq_ignore_ascii_case(target))
                || self
                    .peer_instance_id
                    .as_deref()
                    .is_some_and(|id| id.eq_ignore_ascii_case(target)))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CloudError {
    LimitReached {
        limit: u32,
        active_count: u32,
    },
    LeaseRequired,
    CreditsExhausted,
    PromoInvalidCode,
    PromoExpired,
    PromoDisabled,
    PromoExhausted,
    PromoSpendCapExceeded,
    CheckoutInvalid,
    CheckoutBelowMin,
    CheckoutAboveMax,
    CheckoutTerritory,
    CheckoutUnavailable(String),
    ImplicitCreateRefused,
    MachineNotFound(String),
    MachineAmbiguous(String),
    MachineSleeping {
        machine_id: String,
    },
    MachineBusy {
        machine_id: String,
        reason: String,
    },
    MachineOutOfCredits {
        machine_id: String,
    },
    InvalidTransition {
        from: CloudLifecycleState,
        event: &'static str,
    },
    DeleteConfirmationRequired {
        machine_id: String,
        warning: String,
    },
    VolumeUpgradeConfirmationRequired {
        machine_id: String,
        new_size_gb: u32,
        warning: String,
    },
    DiskFullLocal {
        machine_id: String,
        detail: String,
    },
    ProviderRefused(String),
    ProviderUnconfirmed(String),
    ProviderUnavailable(String),
    Conflict(String),
    RateLimited {
        retry_after_seconds: u64,
    },
    EnrollmentFailed(String),
    ApplyUnconfirmed {
        machine_id: String,
        installation_id: String,
    },
    WakeDeadlineExceeded {
        machine_id: String,
    },
    SendAlreadyClaimed(String),
    Internal(String),
}

impl CloudError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::LimitReached { .. } => "limit_reached",
            Self::LeaseRequired => "lease_required",
            Self::CreditsExhausted | Self::MachineOutOfCredits { .. } => "credits_exhausted",
            Self::PromoInvalidCode => "invalid_code",
            Self::PromoExpired => "expired",
            Self::PromoDisabled => "disabled",
            Self::PromoExhausted => "exhausted",
            Self::PromoSpendCapExceeded => "spend_cap_exceeded",
            Self::CheckoutInvalid => "invalid",
            Self::CheckoutBelowMin => "below_min",
            Self::CheckoutAboveMax => "above_max",
            Self::CheckoutTerritory => "territory",
            Self::CheckoutUnavailable(_) => "unavailable",
            Self::ImplicitCreateRefused => "implicit_create_refused",
            Self::MachineNotFound(_) => "machine_not_found",
            Self::MachineAmbiguous(_) => "machine_ambiguous",
            Self::MachineSleeping { .. } => "machine_sleeping",
            Self::MachineBusy { .. } => "machine_busy",
            Self::InvalidTransition { .. } => "invalid_transition",
            Self::DeleteConfirmationRequired { .. } => "delete_confirmation_required",
            Self::VolumeUpgradeConfirmationRequired { .. } => {
                "volume_upgrade_confirmation_required"
            }
            Self::DiskFullLocal { .. } => "disk_full_local",
            Self::ProviderRefused(_) => "provider_refused",
            Self::ProviderUnconfirmed(_) => "provider_unconfirmed",
            Self::ProviderUnavailable(_) => "provider_unavailable",
            Self::Conflict(_) => "conflict",
            Self::RateLimited { .. } => "rate_limited",
            Self::EnrollmentFailed(_) => "enrollment_failed",
            Self::ApplyUnconfirmed { .. } => "apply_unconfirmed",
            Self::WakeDeadlineExceeded { .. } => "wake_deadline_exceeded",
            Self::SendAlreadyClaimed(_) => "send_already_claimed",
            Self::Internal(_) => "cloud_internal_error",
        }
    }

    pub fn bytes_written(&self) -> Option<bool> {
        match self {
            Self::MachineSleeping { .. }
            | Self::MachineOutOfCredits { .. }
            | Self::CreditsExhausted
            | Self::LeaseRequired
            | Self::WakeDeadlineExceeded { .. } => Some(false),
            _ => None,
        }
    }

    pub fn is_uncertain_provisioning_outcome(&self) -> bool {
        matches!(
            self,
            Self::ProviderUnconfirmed(_) | Self::ProviderUnavailable(_)
        )
    }

    pub fn is_retryable_wake_error(&self) -> bool {
        matches!(self, Self::ProviderUnconfirmed(_))
    }
}

impl std::fmt::Display for CloudError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LimitReached {
                limit,
                active_count,
            } => {
                write!(
                    f,
                    "cloud machine limit reached ({active_count}/{limit}); reuse an existing cloud machine or delete one before creating another"
                )
            }
            Self::LeaseRequired => {
                write!(
                    f,
                    "a paid cloud credit lease is required before starting or creating a cloud machine"
                )
            }
            Self::CreditsExhausted => {
                write!(f, "Out of credits: top up to wake or create a cloud machine")
            }
            Self::PromoInvalidCode => write!(f, "Promo code is not valid"),
            Self::PromoExpired => write!(f, "Promo code has expired"),
            Self::PromoDisabled => write!(f, "Promo code is disabled"),
            Self::PromoExhausted => write!(f, "Promo code has no redemptions remaining"),
            Self::PromoSpendCapExceeded => {
                write!(f, "Monthly cloud credit spend cap exceeded")
            }
            Self::CheckoutInvalid => write!(f, "Cloud credit checkout request is not valid"),
            Self::CheckoutBelowMin => write!(f, "Cloud credit top-up amount is below the minimum"),
            Self::CheckoutAboveMax => write!(f, "Cloud credit top-up amount exceeds the maximum"),
            Self::CheckoutTerritory => {
                write!(f, "Cloud credit purchases are not available in this territory")
            }
            Self::CheckoutUnavailable(detail) => {
                write!(f, "Cloud credit checkout unavailable: {detail}")
            }
            Self::ImplicitCreateRefused => {
                write!(
                    f,
                    "no existing cloud machine is available for reuse"
                )
            }
            Self::MachineNotFound(target) => write!(f, "cloud machine not found: {target}"),
            Self::MachineAmbiguous(target) => {
                write!(
                    f,
                    "cloud machine selector is ambiguous: {target}; use the exact machine id"
                )
            }
            Self::MachineSleeping { machine_id } => {
                write!(
                    f,
                    "cloud machine {machine_id} is draining or sleeping; nothing written"
                )
            }
            Self::MachineBusy { machine_id, reason } => {
                write!(
                    f,
                    "cloud machine {machine_id} is busy and cannot sleep: {reason}"
                )
            }
            Self::MachineOutOfCredits { machine_id } => {
                write!(
                    f,
                    "Out of credits: top up to wake cloud machine {machine_id}"
                )
            }
            Self::InvalidTransition { from, event } => {
                write!(
                    f,
                    "invalid cloud machine state transition from {} on {event}",
                    from.as_str()
                )
            }
            Self::DeleteConfirmationRequired { warning, .. } => f.write_str(warning),
            Self::VolumeUpgradeConfirmationRequired { warning, .. } => f.write_str(warning),
            Self::DiskFullLocal { machine_id, detail } => {
                write!(f, "cloud machine {machine_id} disk is full: {detail}")
            }
            Self::ProviderRefused(detail) => write!(f, "cloud provisioning refused: {detail}"),
            Self::ProviderUnconfirmed(detail) => {
                write!(f, "cloud provisioning outcome unconfirmed: {detail}")
            }
            Self::ProviderUnavailable(detail) => write!(f, "cloud backend unavailable: {detail}"),
            Self::Conflict(detail) => write!(f, "cloud provisioning conflict: {detail}"),
            Self::RateLimited {
                retry_after_seconds,
            } => write!(
                f,
                "cloud provisioning rate-limited; retry after {retry_after_seconds}s"
            ),
            Self::EnrollmentFailed(detail) => write!(f, "cloud enrollment failed: {detail}"),
            Self::ApplyUnconfirmed {
                machine_id,
                installation_id,
            } => write!(
                f,
                "cloud enrollment Apply outcome unconfirmed for {machine_id} (installation {installation_id}); awaiting enrolled channel status without re-applying"
            ),
            Self::WakeDeadlineExceeded { machine_id } => {
                write!(
                    f,
                    "cloud machine {machine_id} did not become ready before the wake deadline"
                )
            }
            Self::SendAlreadyClaimed(detail) => f.write_str(detail),
            Self::Internal(detail) => f.write_str(detail),
        }
    }
}

impl std::error::Error for CloudError {}
