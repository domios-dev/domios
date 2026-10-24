use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::ports::SignedMembership;
use crate::types::{PairedPeer, RevocationList};

/// Why a paired edge may carry traffic. Persisted beside the peer record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "admission", rename_all = "snake_case")]
pub enum LinkAdmission {
    /// One-time rollout provenance for a pair persisted before the account gate existed.
    Legacy { public_key: String },
    Adopted {
        account_id: String,
        installation_id: String,
        public_key: String,
    },
}

impl LinkAdmission {
    pub fn public_key(&self) -> &str {
        match self {
            Self::Legacy { public_key } | Self::Adopted { public_key, .. } => public_key,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Legacy { .. } => "legacy",
            Self::Adopted { .. } => "adopted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_nonce: Option<String>,
    pub admission: LinkAdmission,
    #[serde(default)]
    pub remote_membership: Option<SignedMembership>,
}

impl LinkRecord {
    pub fn installation_id(&self) -> Option<&str> {
        match &self.admission {
            LinkAdmission::Adopted {
                installation_id, ..
            } => Some(installation_id),
            LinkAdmission::Legacy { .. } => None,
        }
    }
}

/// How `peers.json` loaded; only a readable file may seed legacy provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeersLoad {
    Present,
    Missing,
    Held,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkLedger {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub disconnects: HashMap<String, crate::disconnect::SavedDisconnect>,
    /// First account this machine signed in with; never rebinds.
    #[serde(default)]
    pub owner_account: Option<String>,
    #[serde(default)]
    pub links: HashMap<String, LinkRecord>,
}

impl LinkLedger {
    /// Returns the ledger and whether it must be persisted before gates open.
    pub fn migrate(
        stored: Option<LinkLedger>,
        peers: &HashMap<String, PairedPeer>,
        revocations: &RevocationList,
        load: PeersLoad,
    ) -> (Self, bool) {
        if let Some(mut ledger) = stored {
            ledger.links.retain(|id, _| peers.contains_key(id));
            return (ledger, false);
        }
        match load {
            PeersLoad::Present => (Self::legacy_snapshot(peers, revocations), true),
            PeersLoad::Missing => (Self::default(), true),
            PeersLoad::Held => (Self::default(), false),
        }
    }

    fn legacy_snapshot(peers: &HashMap<String, PairedPeer>, revocations: &RevocationList) -> Self {
        let links = peers
            .iter()
            .filter(|(id, _)| {
                !revocations.blocked.iter().any(|b| b == *id)
                    && !revocations.revoked.iter().any(|r| r == *id)
                    && !revocations.tombstones.contains_key(id.as_str())
            })
            .map(|(id, peer)| {
                (
                    id.clone(),
                    LinkRecord {
                        link_nonce: None,
                        admission: LinkAdmission::Legacy {
                            public_key: peer.public_key.clone(),
                        },
                        remote_membership: None,
                    },
                )
            })
            .collect();
        Self {
            owner_account: None,
            links,
            disconnects: HashMap::new(),
        }
    }

    pub fn bind_owner(&mut self, account_id: &str) -> bool {
        if self.owner_account.is_some() {
            return false;
        }
        self.owner_account = Some(account_id.to_string());
        true
    }

    pub fn get(&self, peer_id: &str) -> Option<&LinkRecord> {
        self.links.get(peer_id)
    }

    pub fn remove(&mut self, peer_id: &str) -> Option<LinkRecord> {
        self.links.remove(peer_id)
    }

    pub fn clear(&mut self) {
        self.links.clear();
    }

    /// Records account authority for an edge. Monotonic: Legacy may become
    /// Adopted, never the reverse, and the pinned transport key never changes.
    pub fn admit(
        &mut self,
        peer_id: &str,
        account_id: &str,
        installation_id: &str,
        public_key: &str,
        remote: SignedMembership,
    ) -> Result<(), String> {
        if remote.verified.account_id != account_id
            || remote.verified.installation_id != installation_id
        {
            return Err("Peer membership does not match the admitted identity".into());
        }
        if let Some(existing) = self.links.get(peer_id) {
            if existing.admission.public_key() != public_key {
                return Err("Peer key differs from the paired key".into());
            }
            if let LinkAdmission::Adopted {
                account_id: known_account,
                installation_id: known_installation,
                ..
            } = &existing.admission
            {
                if known_account != account_id || known_installation != installation_id {
                    return Err("Peer is already admitted under another account".into());
                }
            }
        }
        let link_nonce = self
            .links
            .get(peer_id)
            .and_then(|record| record.link_nonce.clone());
        self.links.insert(
            peer_id.to_string(),
            LinkRecord {
                link_nonce,
                admission: LinkAdmission::Adopted {
                    account_id: account_id.to_string(),
                    installation_id: installation_id.to_string(),
                    public_key: public_key.to_string(),
                },
                remote_membership: Some(remote),
            },
        );
        Ok(())
    }

    /// Accepts a renewed peer membership only for the same adopted identity and a later expiry.
    pub fn refresh_remote_membership(&mut self, peer_id: &str, renewed: SignedMembership) -> bool {
        let Some(record) = self.links.get_mut(peer_id) else {
            return false;
        };
        let LinkAdmission::Adopted {
            account_id,
            installation_id,
            ..
        } = &record.admission
        else {
            return false;
        };
        if &renewed.verified.account_id != account_id
            || &renewed.verified.installation_id != installation_id
        {
            return false;
        }
        let newer = record
            .remote_membership
            .as_ref()
            .is_none_or(|known| renewed.verified.expires_at > known.verified.expires_at);
        if newer {
            record.remote_membership = Some(renewed);
        }
        newer
    }
}
