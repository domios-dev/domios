use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::ports::SignedMembership;

/// Authenticated account provenance for a runtime connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "admission", rename_all = "snake_case")]
pub enum LinkAdmission {
    Account {
        account_id: String,
        installation_id: String,
        public_key: String,
    },
}

impl LinkAdmission {
    pub fn public_key(&self) -> &str {
        match self {
            Self::Account { public_key, .. } => public_key,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Account { .. } => "account",
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
            LinkAdmission::Account {
                installation_id, ..
            } => Some(installation_id),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkLedger {
    #[serde(default)]
    pub links: HashMap<String, LinkRecord>,
}

impl LinkLedger {
    pub fn get(&self, peer_id: &str) -> Option<&LinkRecord> {
        self.links.get(peer_id)
    }

    pub fn remove(&mut self, peer_id: &str) -> Option<LinkRecord> {
        self.links.remove(peer_id)
    }

    pub fn clear(&mut self) {
        self.links.clear();
    }

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
            let LinkAdmission::Account {
                account_id: known_account,
                installation_id: known_installation,
                ..
            } = &existing.admission;
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
                admission: LinkAdmission::Account {
                    account_id: account_id.to_string(),
                    installation_id: installation_id.to_string(),
                    public_key: public_key.to_string(),
                },
                remote_membership: Some(remote),
            },
        );
        Ok(())
    }

    /// Accepts a renewed peer membership only for the same account identity and a later expiry.
    pub fn refresh_remote_membership(&mut self, peer_id: &str, renewed: SignedMembership) -> bool {
        let Some(record) = self.links.get_mut(peer_id) else {
            return false;
        };
        let LinkAdmission::Account {
            account_id,
            installation_id,
            ..
        } = &record.admission;
        if &renewed.verified.account_id != account_id
            || &renewed.verified.installation_id != installation_id
        {
            return false;
        }
        let newer = record.remote_membership.as_ref().is_none_or(|known| {
            renewed.verified.key_generation > known.verified.key_generation
                || (renewed.verified.key_generation == known.verified.key_generation
                    && renewed.verified.key_id == known.verified.key_id
                    && renewed.verified.slot_revision >= known.verified.slot_revision
                    && renewed.verified.expires_at > known.verified.expires_at)
        });
        if newer {
            record.remote_membership = Some(renewed);
        }
        newer
    }
}
