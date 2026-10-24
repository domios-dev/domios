//! Rust mirror of the server's `shared/account-mesh.ts` and `shared/online-installation-proof.ts`.

use codeterm_entitlements::DetachedGrant;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

pub const PROOF_SCHEMA_VERSION: u8 = 1;
pub const MESH_SCHEMA_VERSION: u8 = 1;
pub const MACHINE_LIMIT_REASON: &str = "mesh.machines";
pub const MESH_LINK_PERMIT_SECONDS: u64 = 120;

/// Server callable names (`ACCOUNT_MESH_ROUTES`).
pub const ROUTE_CHALLENGE: &str = "accountMeshChallenge";
pub const ROUTE_ENROLL_KEY: &str = "accountMeshEnrollKey";
pub const ROUTE_ADMISSION: &str = "accountMeshAdmission";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum ProofEndpoint {
    #[serde(rename = "enrollInstallation")]
    EnrollInstallation,
    #[serde(rename = "meshAdmission")]
    MeshAdmission,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallationProof {
    pub schema_version: u8,
    pub key_id: String,
    pub generation: u64,
    pub timestamp: u64,
    pub nonce: String,
    pub signature: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshTarget {
    pub installation_id: String,
    pub key_id: String,
    pub key_generation: u64,
    pub slot_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshCursor {
    pub after_installation_id: String,
    pub registry_revision: u64,
}

/// Canonical Tailnet IP text plus nonzero API port; no URL, host or path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshEndpointWire {
    pub tailnet_ip: String,
    pub api_port: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshEndpointObservation {
    pub tailnet_ip: String,
    pub api_port: u16,
    pub expires_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MeshAdmissionBody {
    pub schema_version: u8,
    pub installation_id: String,
    pub request_id: String,
    #[serde(flatten)]
    pub operation: MeshOperation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum MeshOperation {
    Enroll {
        expected_slot_revision: u64,
    },
    Renew {
        expected_slot_revision: u64,
    },
    Revoke {
        target: MeshTarget,
    },
    Recover {
        target: MeshTarget,
    },
    List {
        cursor: Option<MeshCursor>,
    },
    Update {
        expected_metadata_revision: u64,
        display_name: String,
    },
    Publish {
        expected_metadata_revision: u64,
        endpoint: MeshEndpointWire,
    },
    Withdraw {
        expected_metadata_revision: u64,
    },
    Link {
        expected_slot_revision: u64,
        peer: MeshTarget,
        transcript_sha256: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SignedEnvelope<T> {
    pub request: T,
    pub installation_proof: InstallationProof,
}

/// The nonce is bound to the digest of exactly this body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ChallengeRequest {
    pub endpoint: ProofEndpoint,
    pub key_id: String,
    pub body: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeResult {
    pub nonce: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyOperation {
    Enroll,
    Recover,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InstallationKeyBody {
    pub installation_id: String,
    pub public_key_spki_base64: String,
    pub operation: KeyOperation,
    pub expected_generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InstallationEnrollment {
    #[serde(flatten)]
    pub body: InstallationKeyBody,
    pub proof: InstallationProof,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyEnrollment {
    pub generation: u64,
    pub key_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipOperation {
    Enroll,
    Renew,
    Recover,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MembershipResult {
    pub operation: MembershipOperation,
    pub installation_id: String,
    pub slot_revision: u64,
    pub registry_revision: u64,
    pub membership: DetachedGrant,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevokedResult {
    pub target_installation_id: String,
    pub target_key_id: String,
    pub target_key_generation: u64,
    pub previous_slot_revision: u64,
    pub slot_revision: u64,
    pub registry_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveredResult {
    pub installation_id: String,
    pub key_id: String,
    pub key_generation: u64,
    pub slot_revision: u64,
    pub registry_revision: u64,
    pub membership: Option<DetachedGrant>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdatedResult {
    pub installation_id: String,
    pub display_name: String,
    pub previous_metadata_revision: u64,
    pub metadata_revision: u64,
    pub slot_revision: u64,
    pub registry_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedResult {
    pub installation_id: String,
    pub endpoint: MeshEndpointObservation,
    pub previous_metadata_revision: u64,
    pub metadata_revision: u64,
    pub slot_revision: u64,
    pub registry_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WithdrawnResult {
    pub installation_id: String,
    pub previous_metadata_revision: u64,
    pub metadata_revision: u64,
    pub slot_revision: u64,
    pub registry_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermitResult {
    pub installation_id: String,
    pub peer_installation_id: String,
    pub slot_revision: u64,
    pub registry_revision: u64,
    pub permit: DetachedGrant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicationState {
    None,
    Pending,
    Complete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Eligibility {
    Eligible,
    MeshAllowance,
    Revoked,
    VerificationUnavailable,
}

/// `endpoint` is a published observation on eligible unexpired rows, never online status.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshManagementRow {
    pub installation_id: String,
    pub key_id: String,
    pub key_generation: u64,
    pub slot_revision: u64,
    pub active: bool,
    pub admitted_at: u64,
    pub authenticated_at: u64,
    pub display_name: Option<String>,
    pub metadata_revision: u64,
    pub publication: PublicationState,
    pub membership_expires_at: Option<u64>,
    pub eligibility: Eligibility,
    pub endpoint: Option<MeshEndpointObservation>,
}

impl MeshManagementRow {
    pub fn target(&self) -> MeshTarget {
        MeshTarget {
            installation_id: self.installation_id.clone(),
            key_id: self.key_id.clone(),
            key_generation: self.key_generation,
            slot_revision: self.slot_revision,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageResult {
    pub registry_revision: u64,
    pub rows: Vec<MeshManagementRow>,
    pub cursor: Option<MeshCursor>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MeshResult {
    Membership(MembershipResult),
    Revoked(RevokedResult),
    Recovered(RecoveredResult),
    Updated(UpdatedResult),
    Published(PublishedResult),
    Withdrawn(WithdrawnResult),
    Permit(PermitResult),
    Page(PageResult),
}

/// A Tailnet listener address: canonical IP, nonzero port, nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshEndpoint {
    ip: IpAddr,
    port: u16,
}

impl MeshEndpoint {
    pub fn new(ip: IpAddr, port: u16) -> Option<Self> {
        (port != 0 && tailnet(ip)).then_some(Self { ip, port })
    }
    /// Accepts only the exact canonical text, so aliases never pass.
    pub fn parse(tailnet_ip: &str, port: u16) -> Option<Self> {
        let ip: IpAddr = tailnet_ip.parse().ok()?;
        (ip.to_string() == tailnet_ip)
            .then(|| Self::new(ip, port))
            .flatten()
    }
    pub fn ip(&self) -> IpAddr {
        self.ip
    }
    pub fn port(&self) -> u16 {
        self.port
    }
    pub fn wire(&self) -> MeshEndpointWire {
        MeshEndpointWire {
            tailnet_ip: self.ip.to_string(),
            api_port: self.port,
        }
    }
}

fn tailnet(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            a == 100 && (64..128).contains(&b)
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            s[0] == 0xfd7a && s[1] == 0x115c && s[2] == 0xa1e0
        }
    }
}

/// Server refusal details (`AccountMeshRefusal`) plus the desktop's own transport failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", deny_unknown_fields)]
pub enum RegistryRefusal {
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
    #[serde(rename = "authority-unavailable")]
    AuthorityUnavailable,
    #[serde(rename = "unavailable")]
    Unavailable,
    #[serde(rename = "offline")]
    Offline,
}

impl RegistryRefusal {
    /// Unknown or malformed details are an outage, never a plan-limit refusal.
    pub fn from_details(details: &serde_json::Value) -> Self {
        match serde_json::from_value::<Self>(details.clone()) {
            Ok(Self::Offline) | Err(_) => Self::Unavailable,
            Ok(Self::MachineLimit { limit: 0, .. }) => Self::Unavailable,
            Ok(Self::RateLimited {
                retry_after_seconds: 0,
            }) => Self::Unavailable,
            Ok(refusal) => refusal,
        }
    }
}
