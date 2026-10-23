use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Local account presence as published by the account lifecycle owner.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AccountPresence {
    /// No account was ever associated; upgrade without sign-in is not a sign-out.
    #[default]
    NeverSignedIn,
    SignedIn {
        account_id: String,
    },
    /// Explicit, persisted sign-out. Blocks every Mesh edge.
    SignedOut,
}

/// Same not-before tolerance as the account verifier, so a slightly fast issuer clock never blocks P2P.
pub const NOT_BEFORE_SKEW_SECONDS: u64 = 120;

/// Membership facts that the account verifier already checked
/// (issuer, audience, project, environment, epoch, subject, key, slot, signature).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedMembership {
    pub account_id: String,
    pub installation_id: String,
    /// Hex SHA-256 of the installation SPKI, identical to the server key id.
    pub key_id: String,
    pub key_generation: u64,
    pub slot_revision: u64,
    pub issued_at: u64,
    pub expires_at: u64,
}

impl VerifiedMembership {
    pub fn is_current(&self, now: u64) -> bool {
        self.issued_at <= now.saturating_add(NOT_BEFORE_SKEW_SECONDS) && now < self.expires_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedMembership {
    pub token: String,
    pub verified: VerifiedMembership,
}

/// Immutable snapshot pushed into the mesh on every account transition.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AccountMeshState {
    /// Bumps on every sign-in, sign-out, account switch and block.
    pub account_epoch: u64,
    pub presence: AccountPresence,
    pub membership: Option<SignedMembership>,
    /// Installations this machine already knows the server revoked.
    pub revoked_installations: BTreeSet<String>,
    /// Signed in, enforced, and refused a slot by the machine limit; a dormant gate never sets it.
    pub plan_limit_locked: bool,
}

impl AccountMeshState {
    pub fn signed_in_account(&self) -> Option<&str> {
        match &self.presence {
            AccountPresence::SignedIn { account_id } => Some(account_id.as_str()),
            _ => None,
        }
    }

    pub fn current_membership(&self, now: u64) -> Option<&SignedMembership> {
        let account = self.signed_in_account()?;
        self.membership
            .as_ref()
            .filter(|m| m.verified.account_id == account && m.verified.is_current(now))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "kebab-case")]
pub enum AdmissionRefusal {
    Offline,
    Unauthenticated,
    #[serde(rename = "mesh.machines")]
    MachineLimit {
        limit: u32,
        active_count: u32,
    },
    RateLimited {
        retry_after_seconds: u64,
    },
    #[serde(rename = "plan_limit_locked")]
    PlanLimitLocked,
    Revoked,
    SignedOut,
    PeerUnreachable,
    HandshakeFailed {
        detail: String,
    },
    Cancelled,
}

impl AdmissionRefusal {
    pub fn handshake(detail: impl Into<String>) -> Self {
        Self::HandshakeFailed {
            detail: detail.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Offline => "offline",
            Self::Unauthenticated => "unauthenticated",
            Self::MachineLimit { .. } => "mesh.machines",
            Self::RateLimited { .. } => "rate_limited",
            Self::PlanLimitLocked => "plan_limit_locked",
            Self::Revoked => "revoked",
            Self::SignedOut => "signed-out",
            Self::PeerUnreachable => "peer-unreachable",
            Self::HandshakeFailed { .. } => "handshake-failed",
            Self::Cancelled => "cancelled",
        }
    }
}

impl std::fmt::Display for AdmissionRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HandshakeFailed { detail } => write!(f, "handshake-failed: {detail}"),
            Self::MachineLimit {
                limit,
                active_count,
            } => write!(f, "mesh.machines: {active_count}/{limit}"),
            other => f.write_str(other.code()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkAuthorizationRequest {
    pub transcript: [u8; 32],
    pub local_installation_id: String,
    pub peer_installation_id: String,
    pub account_id: String,
    pub link_nonce: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkPermitExpectation {
    pub transcript: [u8; 32],
    pub holder_installation_id: String,
    pub peer_installation_id: String,
    pub account_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkTarget {
    pub installation_id: String,
    pub ip: String,
    pub api_port: u16,
}

/// Account-owned authority the mesh consults; implemented outside this crate.
pub trait AccountMeshAuthority: Send + Sync {
    fn verify_membership(
        &self,
        token: &str,
        now: u64,
    ) -> Result<VerifiedMembership, AdmissionRefusal>;

    fn sign_possession(&self, transcript: &[u8; 32]) -> Result<String, AdmissionRefusal>;

    fn verify_possession(
        &self,
        membership: &VerifiedMembership,
        transcript: &[u8; 32],
        signature: &str,
    ) -> bool;

    fn authorize_link(
        &self,
        request: LinkAuthorizationRequest,
    ) -> BoxFuture<'_, Result<String, AdmissionRefusal>>;

    fn verify_link_permit(
        &self,
        permit: &str,
        expected: &LinkPermitExpectation,
        now: u64,
    ) -> Result<(), AdmissionRefusal>;

    fn resolve_link_target(
        &self,
        installation_id: &str,
    ) -> BoxFuture<'_, Result<LinkTarget, AdmissionRefusal>>;
}
