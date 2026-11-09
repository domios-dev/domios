use super::membership::VerifiedMembership;
use super::wire::{MeshManagementRow, RegistryRefusal};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(rename = "AccountMeshPresence")]
pub enum Presence {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "AccountMeshBinding")]
pub enum Binding {
    #[default]
    None,
    Current,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "AccountMeshTraffic")]
pub enum Traffic {
    #[default]
    Open,
    BlockedSignedOut,
    /// Signed in but refused a slot by the account's machine limit; account installations stay listed.
    #[serde(rename = "plan_limit_locked")]
    PlanLimitLocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "AccountMeshCleanup")]
pub enum Cleanup {
    #[default]
    None,
    Pending,
    Confirmed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "AccountMeshPublication")]
pub enum Publication {
    #[default]
    None,
    NotReady,
    Published,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "AccountMeshEnrollment")]
pub enum Enrollment {
    #[default]
    Idle,
    Enrolling,
    Enrolled,
    Refused,
}

/// Server refusal reasons plus desktop ones; only `mesh.machines` opens the plans modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(tag = "reason")]
pub enum AccountMeshRefusal {
    #[serde(rename = "installation_unverified")]
    InstallationUnverified,
    #[serde(rename = "cutoff_after_current_session")]
    CutoffAfterCurrentSession,
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
    #[serde(rename = "key_conflict")]
    KeyConflict,
    #[serde(rename = "generation_mismatch")]
    GenerationMismatch,
    #[serde(rename = "challenge_invalid")]
    ChallengeInvalid,
    #[serde(rename = "enrollment_budget")]
    EnrollmentBudget {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        retry_after_seconds: Option<u64>,
    },
    #[serde(rename = "proof_invalid")]
    ProofInvalid,
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
            RegistryRefusal::Cancelled => Self::Cancelled,
            RegistryRefusal::Revoked => Self::Revoked,
            RegistryRefusal::InstallationUnverified => Self::InstallationUnverified,
            RegistryRefusal::CutoffAfterCurrentSession => Self::CutoffAfterCurrentSession,
            RegistryRefusal::Invalid => Self::Invalid,
            RegistryRefusal::Unauthenticated => Self::Unauthenticated,
            RegistryRefusal::ReauthRequired => Self::ReauthRequired,
            RegistryRefusal::AppCheck => Self::AppCheck,
            RegistryRefusal::Forbidden => Self::Forbidden,
            RegistryRefusal::Conflict => Self::Conflict,
            RegistryRefusal::KeyConflict => Self::KeyConflict,
            RegistryRefusal::GenerationMismatch => Self::GenerationMismatch,
            RegistryRefusal::ChallengeInvalid => Self::ChallengeInvalid,
            RegistryRefusal::EnrollmentBudget {
                retry_after_seconds,
            } => Self::EnrollmentBudget {
                retry_after_seconds,
            },
            RegistryRefusal::ProofInvalid => Self::ProofInvalid,
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

impl AccountMeshRefusal {
    pub(super) fn retires_membership(self) -> bool {
        matches!(
            self,
            Self::MachineLimit { .. } | Self::Forbidden | Self::Revoked | Self::ReauthRequired
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "AccountMeshEnrollTrigger")]
pub enum EnrollTrigger {
    /// Verified sign-in or signed-in restore.
    Automatic,
    /// The user asked again (retry / Add).
    Explicit,
    Lease,
    Network,
}

/// One `mesh.machines` refusal owned by the request that hit it; the plans modal consumes it once.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(rename = "AccountMeshLimitNotice")]
pub struct LimitNotice {
    pub request_id: String,
    pub refusal: AccountMeshRefusal,
    pub trigger: EnrollTrigger,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
pub struct SelfInstallation {
    pub installation_id: String,
    pub key_id: String,
    pub key_generation: u64,
    pub slot_revision: u64,
    pub metadata_revision: u64,
    pub display_name: Option<String>,
}

/// A registry read is a stale observation for display, never link authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(rename = "AccountMeshRegistryObservation")]
pub struct RegistryObservation {
    pub observed_at: u64,
    pub registry_revision: u64,
    pub rows: Vec<MeshManagementRow>,
    pub cursor: Option<super::wire::MeshCursor>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistrySecurity {
    pub revision: u64,
    pub digest: String,
}

impl RegistryObservation {
    pub fn security_view(&self) -> Option<RegistrySecurity> {
        if self.cursor.is_some() {
            return None;
        }
        let rows: BTreeMap<_, _> = self
            .rows
            .iter()
            .map(|row| {
                (
                    &row.installation_id,
                    (row.active, &row.key_id, row.key_generation, row.presence),
                )
            })
            .collect();
        let bytes = serde_json::to_vec(&rows).ok()?;
        Some(RegistrySecurity {
            revision: self.registry_revision,
            digest: super::key::sha256_hex(&bytes),
        })
    }
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
    #[serde(skip)]
    pub unavailable_installations: BTreeSet<String>,
    pub cleanup: Cleanup,
    pub publication: Publication,
    pub publication_failure: Option<AccountMeshRefusal>,
    pub retry_at: Option<u64>,
    pub enrollment: Enrollment,
    pub last_refusal: Option<AccountMeshRefusal>,
    pub limit_notice: Option<LimitNotice>,
    pub self_installation: Option<SelfInstallation>,
    pub registry: Option<RegistryObservation>,
    #[serde(skip)]
    pub registry_security: Option<RegistrySecurity>,
    #[serde(skip)]
    pub signed_registry: Option<super::registry_snapshot::VerifiedRegistry>,
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

    /// Only a membership of the signed-in account, current at `now`, admits account mesh traffic.
    pub fn current_membership(&self, now: u64) -> Option<&SignedMembership> {
        let account = self.signed_in_account()?;
        self.membership
            .as_ref()
            .filter(|m| m.verified.account_id == account && m.verified.is_current(now))
            .filter(|m| {
                self.signed_registry.as_ref().is_some_and(|registry| {
                    registry.rows.iter().any(|row| {
                        row.installation_id == m.verified.installation_id
                            && row.active
                            && row.presence == super::wire::RegistryPresence::Present
                            && row.key_id == m.verified.key_id
                            && row.key_generation == m.verified.key_generation
                            && row.slot_revision == m.verified.slot_revision
                    })
                })
            })
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryPresenceBinding {
    pub generation: u64,
    pub metadata_revision: u64,
    pub presence: super::wire::RegistryPresence,
}

/// One account's rollback fences and revocations while another account's are active.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AccountFences {
    pub peer_keys: BTreeMap<String, KeyBinding>,
    pub peer_presence: BTreeMap<String, RegistryPresenceBinding>,
    pub revoked_installations: BTreeSet<String>,
    pub revoked_generations: BTreeMap<String, u64>,
}

/// Durable lifecycle facts and server-signed registry observations keyed by opaque account id.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AccountMeshRecord {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub endpoint_sequences: BTreeMap<String, BTreeMap<String, EndpointSequence>>,
    #[serde(skip_serializing_if = "is_zero")]
    pub endpoint_sequence: u64,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub signed_registries: BTreeMap<String, codeterm_entitlements::DetachedGrant>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub terminal_refusals: BTreeMap<String, AccountMeshRefusal>,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub reauth_required_accounts: BTreeSet<String>,
    /// First account association; an account switch never rebinds saved pairs.
    pub owner: Option<String>,
    #[serde(default, skip_serializing)]
    pub admission_cache: Option<super::cache::AdmissionCache>,
    pub signed_out: bool,
    pub cleanup_pending: bool,
    pub published: bool,
    /// The server keeps one key generation per account, so switching back reuses it.
    pub keys: BTreeMap<String, KeyBinding>,
    pub slots: BTreeMap<String, u64>,
    pub revoked_installations: BTreeSet<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub local_revocations: BTreeMap<String, BTreeSet<String>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub revoked_generations: BTreeMap<String, u64>,
    /// Account owning the active fences and revocations; other accounts' are in `account_fences`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fence_account: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub account_fences: BTreeMap<String, AccountFences>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub peer_keys: BTreeMap<String, KeyBinding>,
    #[serde(default, skip_serializing)]
    pub peer_presence: BTreeMap<String, RegistryPresenceBinding>,
    #[serde(skip)]
    pub peer_presence_refresh: bool,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointSequence {
    pub sequence: u64,
    pub digest: [u8; 32],
}
