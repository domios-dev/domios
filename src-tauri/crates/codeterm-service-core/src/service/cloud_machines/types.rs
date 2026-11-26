use std::collections::BTreeMap;
use std::net::SocketAddrV4;

use serde::{Deserialize, Serialize};

pub const DEFAULT_IDLE_SLEEP_MINUTES: u32 = 15;
pub const DEFAULT_LEASE_RENEW_LEAD_MS: u64 = 120_000;
pub const CLOUD_INGRESS_PORT: u16 = 7687;
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CloudUsageSummary {
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_addr: Option<SocketAddrV4>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_addr: Option<SocketAddrV4>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_expires_at_ms: Option<u64>,
    pub billing_notice: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
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

    pub fn backend_machine_id(&self) -> &str {
        if self.ownership.provider_machine_id.is_empty() {
            &self.machine_id
        } else {
            &self.ownership.provider_machine_id
        }
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
            endpoint_addr: self.endpoint_addr,
            lease_expires_at_ms: self.lease_expires_at_ms,
            billing_notice: COST_WARNING_TEXT.into(),
            warning,
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
            Self::ImplicitCreateRefused => "implicit_create_refused",
            Self::MachineNotFound(_) => "machine_not_found",
            Self::MachineAmbiguous(_) => "machine_ambiguous",
            Self::MachineSleeping { .. } => "machine_sleeping",
            Self::MachineBusy { .. } => "machine_busy",
            Self::InvalidTransition { .. } => "invalid_transition",
            Self::DeleteConfirmationRequired { .. } => "delete_confirmation_required",
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
            Self::ImplicitCreateRefused => {
                write!(
                    f,
                    "no existing cloud machine is available for reuse; create one explicitly with `domios mesh cloud create` or `--cloud new`"
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
