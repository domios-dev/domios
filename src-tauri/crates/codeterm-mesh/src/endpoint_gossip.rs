use crate::account_mesh::{AdmissionRefusal, LinkTarget, VerifiedMembership};
use crate::MeshManager;
use codeterm_util::cloud_endpoint::CloudEndpoint;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const ENDPOINT_RECORD_VERSION: u8 = 2;
pub const MAX_ENDPOINT_RECORDS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case", deny_unknown_fields)]
pub enum EndpointAddress {
    DirectIngress { address: std::net::SocketAddrV4 },
    ProviderProxy { host: String },
    Tailnet { tailnet_ip: String, api_port: u16 },
}

impl From<CloudEndpoint> for EndpointAddress {
    fn from(endpoint: CloudEndpoint) -> Self {
        match endpoint {
            CloudEndpoint::Direct(address) => Self::DirectIngress { address },
            CloudEndpoint::ProviderProxy { host } => Self::ProviderProxy { host },
        }
    }
}

impl EndpointAddress {
    pub fn cloud_endpoint(&self) -> Option<CloudEndpoint> {
        match self {
            Self::DirectIngress { address } => Some(CloudEndpoint::Direct(*address)),
            Self::ProviderProxy { host } => {
                Some(CloudEndpoint::ProviderProxy { host: host.clone() })
            }
            Self::Tailnet { .. } => None,
        }
    }

    pub(crate) fn tailnet(&self) -> Option<(&str, u16)> {
        match self {
            Self::Tailnet {
                tailnet_ip,
                api_port,
            } => Some((tailnet_ip, *api_port)),
            Self::DirectIngress { .. } | Self::ProviderProxy { .. } => None,
        }
    }

    fn valid(&self) -> bool {
        match self {
            Self::DirectIngress { address } => {
                crate::cloud_ingress::direct_address_allowed(*address)
            }
            Self::ProviderProxy { host } => crate::cloud_ingress::provider_proxy_host_allowed(host),
            Self::Tailnet {
                tailnet_ip,
                api_port,
            } => *api_port != 0 && crate::types::is_tailscale_ip(tailnet_ip),
        }
    }
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
        digest.update(b"domios-mesh-endpoint-v2\0");
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

pub(crate) struct VerifiedEndpointRecord {
    pub(crate) record: SignedEndpointRecord,
    membership: VerifiedMembership,
}

#[derive(Default)]
pub(crate) struct EndpointRecords {
    pub(crate) records: BTreeMap<String, VerifiedEndpointRecord>,
    pub(crate) pinned_cloud_endpoints: BTreeMap<String, CloudEndpoint>,
}

impl MeshManager {
    fn endpoint_transition_allowed(&self, id: &str, incoming: &Option<EndpointAddress>) -> bool {
        if let Some(CloudEndpoint::ProviderProxy { host: pinned_host }) =
            self.endpoint_records.pinned_cloud_endpoints.get(id)
        {
            return matches!(
                incoming,
                Some(EndpointAddress::ProviderProxy { host }) if host == pinned_host
            );
        }
        if let Some(known) = self.endpoint_records.records.get(id) {
            if let Some(EndpointAddress::ProviderProxy {
                host: existing_host,
            }) = &known.record.claim.endpoint
            {
                return matches!(
                    incoming,
                    Some(EndpointAddress::ProviderProxy { host }) if host == existing_host
                );
            }
        }
        true
    }

    pub fn pin_managed_cloud_endpoint(&mut self, installation_id: String, endpoint: CloudEndpoint) {
        if self
            .endpoint_records
            .pinned_cloud_endpoints
            .get(&installation_id)
            == Some(&endpoint)
        {
            return;
        }
        let expected = Some(EndpointAddress::from(endpoint.clone()));
        let mismatch = self
            .endpoint_records
            .records
            .get(&installation_id)
            .is_some_and(|known| known.record.claim.endpoint != expected);
        if mismatch {
            self.endpoint_records.records.remove(&installation_id);
        }
        self.endpoint_records
            .pinned_cloud_endpoints
            .insert(installation_id, endpoint);
        self.bump_gate();
    }

    pub fn unpin_managed_cloud_endpoint(&mut self, installation_id: &str) {
        if self
            .endpoint_records
            .pinned_cloud_endpoints
            .remove(installation_id)
            .is_some()
        {
            self.bump_gate();
        }
    }
    fn endpoint_membership_current(&self, membership: &VerifiedMembership) -> bool {
        if self.account_authority().is_none() {
            return false;
        }
        let state = self.account_state();
        let matches_registry = state.registry_members.as_ref().is_some_and(|members| {
            members
                .get(&membership.installation_id)
                .is_some_and(|(key, generation, slot)| {
                    key == &membership.key_id
                        && *generation == membership.key_generation
                        && *slot == membership.slot_revision
                })
        });
        matches_registry
            && state.signed_in_account() == Some(membership.account_id.as_str())
            && membership.is_current(self.now())
            && !state
                .revoked_installations
                .contains(&membership.installation_id)
            && !state
                .unavailable_installations
                .contains(&membership.installation_id)
    }

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
        if claim
            .endpoint
            .as_ref()
            .is_some_and(|endpoint| !endpoint.valid())
        {
            return Err(AdmissionRefusal::AuthRefused);
        }
        let authority = self
            .account_authority()
            .ok_or(AdmissionRefusal::Unauthenticated)?;
        let membership = authority.verify_membership(&claim.membership, self.now())?;
        if membership.installation_id != claim.installation_id
            || !self.endpoint_membership_current(&membership)
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
        let membership = self.verify_endpoint_record(&record)?;
        let members = self
            .account_state()
            .registry_members
            .clone()
            .unwrap_or_default();
        self.endpoint_records
            .records
            .retain(|id, _| members.contains_key(id));
        let id = &record.claim.installation_id;
        if !self.endpoint_transition_allowed(id, &record.claim.endpoint) {
            return Err(AdmissionRefusal::AuthRefused);
        }
        if let Some(known) = self.endpoint_records.records.get(id) {
            if record.claim.sequence <= known.record.claim.sequence {
                return Ok(false);
            }
        } else if self.endpoint_records.records.len() >= MAX_ENDPOINT_RECORDS {
            return Err(AdmissionRefusal::AuthRefused);
        }
        let is_cloud_endpoint = record
            .claim
            .endpoint
            .as_ref()
            .is_some_and(|endpoint| endpoint.cloud_endpoint().is_some());
        if !is_cloud_endpoint && record.claim.endpoint.is_none() {
            self.cloud_relay_ok_at.remove(id);
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
        for peer_id in peers {
            let next_cloud_online = is_cloud_endpoint
                && self.inbound_cloud_liveness_for_installation(&peer_id, id)
                    == crate::manager::InboundCloudLiveness::Online;
            if let Some(peer) = self.paired_peers.get_mut(&peer_id) {
                if let Some((ip, port)) = record
                    .claim
                    .endpoint
                    .as_ref()
                    .and_then(EndpointAddress::tailnet)
                {
                    peer.addresses = vec![crate::types::PeerAddress::from_ip(ip)];
                    peer.api_port = port;
                } else {
                    peer.addresses.clear();
                    peer.online = next_cloud_online;
                }
            }
        }
        self.endpoint_records
            .records
            .insert(id.clone(), VerifiedEndpointRecord { record, membership });
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
            .filter(|entry| self.endpoint_membership_current(&entry.membership))
            .map(|entry| entry.record.clone())
            .collect()
    }

    pub fn has_endpoint_record(&self, installation_id: &str) -> bool {
        self.verified_endpoint_record(installation_id).is_some()
    }

    pub fn cloud_ingress_endpoint(&self, installation_id: &str) -> Option<CloudEndpoint> {
        self.verified_endpoint_record(installation_id)?
            .claim
            .endpoint
            .as_ref()?
            .cloud_endpoint()
    }

    pub fn direct_ingress_address(&self, installation_id: &str) -> Option<std::net::SocketAddrV4> {
        self.cloud_ingress_endpoint(installation_id)?.direct_addr()
    }

    pub fn endpoint_target(&self, installation_id: &str) -> Option<LinkTarget> {
        self.verified_endpoint_record(installation_id)?
            .claim
            .endpoint
            .as_ref()
            .and_then(EndpointAddress::tailnet)
            .map(|(ip, port)| LinkTarget {
                installation_id: installation_id.to_owned(),
                ip: ip.to_owned(),
                api_port: port,
            })
    }

    /// Signatures were checked on admission; reads re-verify only live membership and registry state.
    pub(crate) fn verified_endpoint_record(
        &self,
        installation_id: &str,
    ) -> Option<&SignedEndpointRecord> {
        self.endpoint_records
            .records
            .get(installation_id)
            .filter(|entry| self.endpoint_membership_current(&entry.membership))
            .map(|entry| &entry.record)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_direct_ingress_endpoint_for_test(
        &mut self,
        installation_id: String,
        address: std::net::SocketAddrV4,
    ) {
        self.insert_cloud_endpoint_for_test(installation_id, CloudEndpoint::Direct(address));
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_cloud_endpoint_for_test(
        &mut self,
        installation_id: String,
        endpoint: CloudEndpoint,
    ) {
        struct TestAuthority;
        impl crate::account_mesh::AccountMeshAuthority for TestAuthority {
            fn verify_membership(
                &self,
                _: &str,
                _: u64,
            ) -> Result<VerifiedMembership, AdmissionRefusal> {
                Err(AdmissionRefusal::AuthRefused)
            }
            fn sign_possession(&self, _: &[u8; 32]) -> Result<String, AdmissionRefusal> {
                Err(AdmissionRefusal::AuthRefused)
            }
            fn verify_possession(&self, _: &VerifiedMembership, _: &[u8; 32], _: &str) -> bool {
                false
            }
            fn resolve_link_target(
                &self,
                _: &str,
            ) -> crate::account_mesh::BoxFuture<'_, Result<LinkTarget, AdmissionRefusal>>
            {
                Box::pin(async { Err(AdmissionRefusal::PeerUnreachable) })
            }
        }
        if self.account_authority().is_none() {
            self.bind_account_authority(std::sync::Arc::new(TestAuthority));
        }
        let account_id = self
            .account_state()
            .signed_in_account()
            .unwrap_or("test-account")
            .to_owned();
        let expires_at = self.now().saturating_add(3600);
        let membership = VerifiedMembership {
            account_id,
            installation_id: installation_id.clone(),
            key_id: "test-key".into(),
            key_generation: 1,
            slot_revision: 1,
            issued_at: 0,
            expires_at,
        };
        let mut next_state = self.account_state().clone();
        let members = next_state
            .registry_members
            .get_or_insert_with(Default::default);
        if let Some(own) = &next_state.membership {
            members.insert(
                own.verified.installation_id.clone(),
                (
                    own.verified.key_id.clone(),
                    own.verified.key_generation,
                    own.verified.slot_revision,
                ),
            );
        }
        for peer_id in self.paired_peers.keys() {
            if let Some(remote) = self
                .link_record(peer_id)
                .and_then(|link| link.remote_membership.as_ref())
            {
                members.insert(
                    remote.verified.installation_id.clone(),
                    (
                        remote.verified.key_id.clone(),
                        remote.verified.key_generation,
                        remote.verified.slot_revision,
                    ),
                );
            }
        }
        members.insert(
            installation_id.clone(),
            (
                membership.key_id.clone(),
                membership.key_generation,
                membership.slot_revision,
            ),
        );
        self.apply_account_state(next_state);
        let record = SignedEndpointRecord {
            claim: EndpointClaim {
                version: ENDPOINT_RECORD_VERSION,
                installation_id: installation_id.clone(),
                sequence: 1,
                membership: "test-membership".into(),
                endpoint: Some(EndpointAddress::from(endpoint)),
            },
            signature: "test-signature".into(),
        };
        self.endpoint_records.records.insert(
            installation_id,
            VerifiedEndpointRecord { record, membership },
        );
        self.bump_gate();
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
                    let allowed = manager.endpoint_transition_allowed(
                        &record.claim.installation_id,
                        &record.claim.endpoint,
                    );
                    let newer = manager
                        .endpoint_records
                        .records
                        .get(&record.claim.installation_id)
                        .is_none_or(|known| record.claim.sequence > known.record.claim.sequence);
                    (allowed && newer)
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
