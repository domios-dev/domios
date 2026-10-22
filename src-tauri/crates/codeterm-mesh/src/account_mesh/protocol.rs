use serde::{Deserialize, Serialize};

use super::transcript::Party;
use crate::types::PeerAddress;

pub const LINK_HELLO_PATH: &str = "/api/mesh/link/hello";
pub const LINK_CONFIRM_PATH: &str = "/api/mesh/link/confirm";
pub const LINK_COMMIT_PATH: &str = "/api/mesh/link/commit";
pub const ADOPT_OFFER_PATH: &str = "/api/mesh/adopt/offer";
pub const ADOPT_COMMIT_PATH: &str = "/api/mesh/adopt/commit";

/// Carries the sender's current signed membership on announces so adopted edges renew P2P.
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
    pub link_id: String,
    pub initiator: PartyDescriptor,
    pub membership: String,
    pub nonce: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkHelloReply {
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
    pub permit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkConfirmReply {
    pub permit: String,
}

/// The initiator verified the responder's permit and is about to record the pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkCommit {
    pub link_id: String,
    pub mesh_signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkCommitReply {
    pub committed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdoptOffer {
    pub adoption_id: String,
    pub installation_id: String,
    pub membership: String,
    pub nonce: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdoptOfferReply {
    pub adoption_id: String,
    pub installation_id: String,
    pub membership: String,
    pub nonce: String,
    pub installation_signature: String,
    pub mesh_signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdoptCommit {
    pub adoption_id: String,
    pub installation_signature: String,
    pub mesh_signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdoptCommitReply {
    pub adopted: bool,
}

/// Paired-edge view for the account Mesh snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountMeshPair {
    pub instance_id: String,
    pub installation_id: Option<String>,
    pub admission: String,
    pub online: bool,
    pub name: String,
}
