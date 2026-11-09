use crate::account_mesh::{AdmissionRefusal, LinkTarget, VerifiedMembership};
use crate::MeshManager;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const ENDPOINT_RECORD_VERSION: u8 = 1;
pub const MAX_ENDPOINT_RECORDS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointAddress {
    pub tailnet_ip: String,
    pub api_port: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointClaim {
    pub version: u8,
    pub installation_id: String,
    pub sequence: u64,
    pub membership: String,
    pub endpoint: Option<EndpointAddress>,
}

impl EndpointClaim {
    pub fn digest(&self) -> Result<[u8; 32], AdmissionRefusal> {
        let bytes = serde_json::to_vec(self).map_err(|_| AdmissionRefusal::AuthRefused)?;
        let mut digest = Sha256::new();
        digest.update(b"domios-mesh-endpoint-v1\0");
        digest.update(bytes);
        Ok(digest.finalize().into())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedEndpointRecord {
    pub claim: EndpointClaim,
    pub signature: String,
}

#[derive(Default)]
pub(crate) struct EndpointRecords {
    pub(crate) records: BTreeMap<String, SignedEndpointRecord>,
}

impl MeshManager {
    pub fn verify_endpoint_record(
        &self,
        record: &SignedEndpointRecord,
    ) -> Result<VerifiedMembership, AdmissionRefusal> {
        let claim = &record.claim;
        if claim.version != ENDPOINT_RECORD_VERSION || claim.sequence == 0 {
            return Err(AdmissionRefusal::AuthRefused);
        }
        if claim.membership.len() > 16384 || record.signature.len() > 2048 {
            return Err(AdmissionRefusal::AuthRefused);
        }
        if let Some(endpoint) = &claim.endpoint {
            if endpoint.api_port == 0 || !crate::types::is_tailscale_ip(&endpoint.tailnet_ip) {
                return Err(AdmissionRefusal::AuthRefused);
            }
        }
        let authority = self
            .account_authority()
            .ok_or(AdmissionRefusal::Unauthenticated)?;
        let membership = authority.verify_membership(&claim.membership, self.now())?;
        let state = self.account_state();
        let matches_registry = state.registry_members.as_ref().is_some_and(|members| {
            members
                .get(&claim.installation_id)
                .is_some_and(|(key, generation, slot)| {
                    key == &membership.key_id
                        && *generation == membership.key_generation
                        && *slot == membership.slot_revision
                })
        });
        if !matches_registry
            || state.signed_in_account() != Some(membership.account_id.as_str())
            || membership.installation_id != claim.installation_id
            || !membership.is_current(self.now())
            || state.revoked_installations.contains(&claim.installation_id)
            || state
                .unavailable_installations
                .contains(&claim.installation_id)
            || !authority.verify_possession(&membership, &claim.digest()?, &record.signature)
        {
            return Err(AdmissionRefusal::AuthRefused);
        }
        Ok(membership)
    }

    pub(crate) fn receive_endpoint_record(
        &mut self,
        record: SignedEndpointRecord,
    ) -> Result<bool, AdmissionRefusal> {
        self.verify_endpoint_record(&record)?;
        let members = self
            .account_state()
            .registry_members
            .clone()
            .unwrap_or_default();
        self.endpoint_records
            .records
            .retain(|id, _| members.contains_key(id));
        let id = &record.claim.installation_id;
        if let Some(known) = self.endpoint_records.records.get(id) {
            if record.claim.sequence <= known.claim.sequence {
                return Ok(false);
            }
        } else if self.endpoint_records.records.len() >= MAX_ENDPOINT_RECORDS {
            return Err(AdmissionRefusal::AuthRefused);
        }
        let peers: Vec<_> = self
            .paired_peers
            .keys()
            .filter(|peer| {
                self.link_record(peer)
                    .is_some_and(|link| link.installation_id() == Some(id.as_str()))
            })
            .cloned()
            .collect();
        for peer in peers {
            if let Some(peer) = self.paired_peers.get_mut(&peer) {
                peer.addresses = record
                    .claim
                    .endpoint
                    .as_ref()
                    .map(|endpoint| vec![crate::types::PeerAddress::from_ip(&endpoint.tailnet_ip)])
                    .unwrap_or_default();
                if let Some(endpoint) = &record.claim.endpoint {
                    peer.api_port = endpoint.api_port;
                } else {
                    peer.online = false;
                }
            }
        }
        self.endpoint_records.records.insert(id.clone(), record);
        self.bump_gate();
        Ok(true)
    }

    pub fn sign_endpoint_record(
        &mut self,
        endpoint: Option<EndpointAddress>,
        sequence: u64,
        membership: &str,
    ) -> Result<SignedEndpointRecord, AdmissionRefusal> {
        let own = self
            .account_state()
            .current_membership(self.now())
            .ok_or(AdmissionRefusal::Unauthenticated)?;
        if own.token != membership {
            return Err(AdmissionRefusal::Cancelled);
        }
        let claim = EndpointClaim {
            version: ENDPOINT_RECORD_VERSION,
            installation_id: own.verified.installation_id.clone(),
            sequence,
            membership: membership.to_owned(),
            endpoint,
        };
        let signature = self
            .account_authority()
            .ok_or(AdmissionRefusal::Unauthenticated)?
            .sign_possession(&claim.digest()?)?;
        let record = SignedEndpointRecord { claim, signature };
        if !self.receive_endpoint_record(record.clone())? {
            return Err(AdmissionRefusal::AuthRefused);
        }
        Ok(record)
    }

    pub fn endpoint_records(&self) -> Vec<SignedEndpointRecord> {
        self.endpoint_records
            .records
            .values()
            .filter(|record| self.verify_endpoint_record(record).is_ok())
            .cloned()
            .collect()
    }

    pub fn has_endpoint_record(&self, installation_id: &str) -> bool {
        self.endpoint_records
            .records
            .get(installation_id)
            .is_some_and(|record| self.verify_endpoint_record(record).is_ok())
    }

    pub fn endpoint_target(&self, installation_id: &str) -> Option<LinkTarget> {
        let record = self.endpoint_records.records.get(installation_id)?;
        self.verify_endpoint_record(record).ok()?;
        let endpoint = record.claim.endpoint.as_ref()?;
        Some(LinkTarget {
            installation_id: installation_id.to_owned(),
            ip: endpoint.tailnet_ip.clone(),
            api_port: endpoint.api_port,
        })
    }
}

pub async fn receive_endpoint_records(
    mesh: &crate::MeshState,
    records: Vec<SignedEndpointRecord>,
) -> Vec<SignedEndpointRecord> {
    let mut accepted = Vec::new();
    for record in records.into_iter().take(MAX_ENDPOINT_RECORDS) {
        let prepared = {
            let manager = mesh.lock();
            manager
                .verify_endpoint_record(&record)
                .ok()
                .and_then(|membership| {
                    let newer = manager
                        .endpoint_records
                        .records
                        .get(&record.claim.installation_id)
                        .is_none_or(|known| record.claim.sequence > known.claim.sequence);
                    newer
                        .then(|| {
                            manager
                                .account_authority()
                                .map(|authority| (authority, membership))
                        })
                        .flatten()
                })
        };
        let Some((authority, membership)) = prepared else {
            continue;
        };
        let Ok(digest) = record.claim.digest() else {
            continue;
        };
        if authority
            .accept_endpoint_sequence(&membership, record.claim.sequence, digest)
            .await
            != Ok(true)
        {
            continue;
        }
        if mesh.lock().receive_endpoint_record(record.clone()) == Ok(true) {
            accepted.push(record);
        }
    }
    accepted
}
