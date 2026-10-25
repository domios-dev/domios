use super::membership::VerifiedMembership;
use super::wire::{MeshManagementRow, RegistryRefusal};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Presence {
    /// No explicit sign-out ever happened here; legacy continuity stays with transport.
    #[default]
    NeverSignedIn,
    SignedIn {
        account_id: String,
    },
    /// Explicit, persisted sign-out: every Mesh edge is blocked, records are kept.
    SignedOut,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SignedMembership {
    pub token: String,
    pub verified: VerifiedMembership,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Binding {
    #[default]
    None,
    Current,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Traffic {
    #[default]
    Open,
    BlockedSignedOut,
    /// Signed in but refused a slot by the account's machine limit; pairings stay listed.
    #[serde(rename = "plan_limit_locked")]
    PlanLimitLocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Cleanup {
    #[default]
    None,
    Pending,
    Confirmed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Publication {
    #[default]
    None,
    NotReady,
    Published,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Enrollment {
    #[default]
    Idle,
    Enrolling,
    Enrolled,
    Refused,
}

/// Server refusal reasons plus desktop ones; only `mesh.machines` opens the plans modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "reason")]
pub enum AccountMeshRefusal {
    #[serde(rename = "invalid")]
    Invalid,
    #[serde(rename = "unauthenticated")]
    Unauthenticated,
    #[serde(rename = "reauth_required")]
    ReauthRequired,
    #[serde(rename = "app-check")]
    AppCheck,
    #[serde(rename = "forbidden")]
    Forbidden,
    #[serde(rename = "conflict")]
    Conflict,
    #[serde(rename = "rate-limited")]
    RateLimited { retry_after_seconds: u64 },
    #[serde(rename = "mesh.machines")]
    MachineLimit { limit: u32, active_count: u32 },
    /// Local refusal of link traffic while this machine is outside the machine limit.
    #[serde(rename = "plan_limit_locked")]
    PlanLimitLocked,
    #[serde(rename = "authority-unavailable")]
    AuthorityUnavailable,
    #[serde(rename = "unavailable")]
    Unavailable,
    #[serde(rename = "offline")]
    Offline,
    #[serde(rename = "verification-unavailable")]
    VerificationUnavailable,
    #[serde(rename = "revoked")]
    Revoked,
    #[serde(rename = "signed-out")]
    SignedOut,
    #[serde(rename = "cancelled")]
    Cancelled,
}

impl From<RegistryRefusal> for AccountMeshRefusal {
    fn from(value: RegistryRefusal) -> Self {
        match value {
            RegistryRefusal::Invalid => Self::Invalid,
            RegistryRefusal::Unauthenticated => Self::Unauthenticated,
            RegistryRefusal::ReauthRequired => Self::ReauthRequired,
            RegistryRefusal::AppCheck => Self::AppCheck,
            RegistryRefusal::Forbidden => Self::Forbidden,
            RegistryRefusal::Conflict => Self::Conflict,
            RegistryRefusal::RateLimited {
                retry_after_seconds,
            } => Self::RateLimited {
                retry_after_seconds,
            },
            RegistryRefusal::MachineLimit {
                limit,
                active_count,
            } => Self::MachineLimit {
                limit,
                active_count,
            },
            RegistryRefusal::AuthorityUnavailable => Self::AuthorityUnavailable,
            RegistryRefusal::Unavailable => Self::Unavailable,
            RegistryRefusal::Offline => Self::Offline,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnrollTrigger {
    /// Verified sign-in or signed-in restore.
    Automatic,
    /// The user asked again (retry / Add).
    Explicit,
    Lease,
    Network,
}

/// One `mesh.machines` refusal owned by the request that hit it; the plans modal consumes it once.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LimitNotice {
    pub request_id: String,
    pub refusal: AccountMeshRefusal,
    pub trigger: EnrollTrigger,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SelfInstallation {
    pub installation_id: String,
    pub key_id: String,
    pub key_generation: u64,
    pub slot_revision: u64,
    pub metadata_revision: u64,
    pub display_name: Option<String>,
}

/// A registry read is a stale observation for display, never link authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RegistryObservation {
    pub observed_at: u64,
    pub registry_revision: u64,
    pub rows: Vec<MeshManagementRow>,
    pub cursor: Option<super::wire::MeshCursor>,
}

/// Immutable lifecycle snapshot; every change is a new value with explicit epochs.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize)]
pub struct LifecycleState {
    /// Bumps on every sign-in, sign-out, account switch and block.
    pub account_epoch: u64,
    pub membership_epoch: u64,
    pub presence: Presence,
    pub owner: Option<String>,
    #[serde(skip)]
    pub membership: Option<SignedMembership>,
    pub revoked_installations: BTreeSet<String>,
    pub cleanup: Cleanup,
    pub publication: Publication,
    pub publication_failure: Option<AccountMeshRefusal>,
    pub retry_at: Option<u64>,
    pub enrollment: Enrollment,
    pub last_refusal: Option<AccountMeshRefusal>,
    pub limit_notice: Option<LimitNotice>,
    pub self_installation: Option<SelfInstallation>,
    pub registry: Option<RegistryObservation>,
    pub registry_failure: Option<AccountMeshRefusal>,
}

impl LifecycleState {
    pub fn signed_in_account(&self) -> Option<&str> {
        match &self.presence {
            Presence::SignedIn { account_id } => Some(account_id),
            _ => None,
        }
    }

    pub fn traffic(&self) -> Traffic {
        match self.presence {
            Presence::SignedOut => Traffic::BlockedSignedOut,
            _ if self.plan_limit_locked() => Traffic::PlanLimitLocked,
            _ => Traffic::Open,
        }
    }

    /// Only the server's own machine-limit refusal locks; a dormant release never enrolls.
    pub fn plan_limit_locked(&self) -> bool {
        self.signed_in_account().is_some()
            && self.membership.is_none()
            && matches!(
                self.last_refusal,
                Some(AccountMeshRefusal::MachineLimit { .. })
            )
    }

    pub fn binding(&self) -> Binding {
        match (&self.owner, self.signed_in_account()) {
            (None, _) => Binding::None,
            (Some(owner), Some(account)) if owner != account => Binding::Other,
            (Some(_), _) => Binding::Current,
        }
    }

    /// Only a membership of the signed-in account, current at `now`, admits adopted traffic.
    pub fn current_membership(&self, now: u64) -> Option<&SignedMembership> {
        let account = self.signed_in_account()?;
        self.membership
            .as_ref()
            .filter(|m| m.verified.account_id == account && m.verified.is_current(now))
    }

    pub fn view(&self) -> AccountMeshView {
        AccountMeshView {
            account_epoch: self.account_epoch,
            binding: self.binding(),
            traffic: self.traffic(),
            cleanup: self.cleanup,
            self_installation: self.self_installation.clone(),
            registry: self.registry.clone(),
            registry_failure: self.registry_failure,
            enrollment: self.enrollment,
            last_refusal: self.last_refusal,
            limit_notice: self.limit_notice.clone(),
            publication: self.publication,
            publication_failure: self.publication_failure,
            retry_at: self.retry_at,
            membership_expires_at: self.membership.as_ref().map(|m| m.verified.expires_at),
        }
    }
}

/// Account-owned part of the `account_mesh_state` snapshot; transport joins `pairs`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AccountMeshView {
    pub account_epoch: u64,
    pub binding: Binding,
    pub traffic: Traffic,
    pub cleanup: Cleanup,
    #[serde(rename = "self")]
    pub self_installation: Option<SelfInstallation>,
    pub registry: Option<RegistryObservation>,
    pub registry_failure: Option<AccountMeshRefusal>,
    pub enrollment: Enrollment,
    pub last_refusal: Option<AccountMeshRefusal>,
    pub limit_notice: Option<LimitNotice>,
    pub publication: Publication,
    pub publication_failure: Option<AccountMeshRefusal>,
    pub retry_at: Option<u64>,
    pub membership_expires_at: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyBinding {
    pub key_id: String,
    pub generation: u64,
}

/// Durable lifecycle facts keyed by opaque account id. Holds no token, uid or endpoint.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AccountMeshRecord {
    /// First account association; an account switch never rebinds saved pairs.
    pub owner: Option<String>,
    pub signed_out: bool,
    pub cleanup_pending: bool,
    pub published: bool,
    /// The server keeps one key generation per account, so switching back reuses it.
    pub keys: BTreeMap<String, KeyBinding>,
    pub slots: BTreeMap<String, u64>,
    pub revoked_installations: BTreeSet<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub revoked_generations: BTreeMap<String, u64>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub peer_keys: BTreeMap<String, KeyBinding>,
}
