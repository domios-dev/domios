use serde::{Deserialize, Serialize};

use super::transcript::Party;
use crate::types::PeerAddress;

pub const LINK_PROTOCOL_VERSION: u8 = 2;

pub const LINK_HELLO_PATH: &str = "/api/mesh/link/hello";
pub const LINK_CONFIRM_PATH: &str = "/api/mesh/link/confirm";
pub const LINK_COMMIT_PATH: &str = "/api/mesh/link/commit";

/// Carries the sender's current signed membership on announces so account connections renew P2P.
pub const MEMBERSHIP_HEADER: &str = "x-domios-account-membership";

/// Handshake routes authenticate by membership and transcript proof, never by a pairing key.
pub const LINK_PATHS: [&str; 3] = [LINK_HELLO_PATH, LINK_CONFIRM_PATH, LINK_COMMIT_PATH];

/// Metadata a side reveals about itself; addresses never carry authority.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartyDescriptor {
    #[serde(flatten)]
    pub party: Party,
    pub instance_name: String,
    pub api_port: u16,
    #[serde(default)]
    pub addresses: Vec<PeerAddress>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkHello {
    #[serde(default)]
    pub protocol_version: u8,
    pub signed_registry: Option<codeterm_entitlements::DetachedGrant>,
    pub link_id: String,
    pub initiator: PartyDescriptor,
    pub membership: String,
    pub nonce: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkHelloReply {
    #[serde(default)]
    pub protocol_version: u8,
    pub signed_registry: Option<codeterm_entitlements::DetachedGrant>,
    pub link_id: String,
    pub responder: PartyDescriptor,
    pub membership: String,
    pub nonce: String,
    pub installation_signature: String,
    pub mesh_signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkConfirm {
    pub link_id: String,
    pub installation_signature: String,
    pub mesh_signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkConfirmReply {
    pub confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkCommit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_registry: Option<codeterm_entitlements::DetachedGrant>,
    pub link_id: String,
    pub mesh_signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkCommitReply {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_registry: Option<codeterm_entitlements::DetachedGrant>,
    pub committed: bool,
}

/// Paired-edge view for the account Mesh snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
pub struct AccountMeshPair {
    pub instance_id: String,
    pub installation_id: Option<String>,
    pub admission: String,
    pub online: bool,
    pub name: String,
}
