use super::admission::{EnrolledProof, IngressRole, VerifiedIngressPeer};
use super::dial::{open, ClientStream};
use super::enrolled_wire::{ChannelIo, EncryptedStream};
use super::{CloudRoute, IngressError};
use crate::account_mesh::{
    ports::BoxFuture, transcript::Party, AdmissionRefusal, VerifiedMembership,
};
use crate::{MeshManager, MeshState};
use codeterm_entitlements::DetachedGrant;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};

pub(super) const PATH: &str = "/cloud/enrolled/v1";

pub trait RenewalReceiver: Send + Sync {
    fn enrolled(&self) -> bool;
    fn apply<'a>(
        &'a self,
        membership: &'a DetachedGrant,
        registry: &'a DetachedGrant,
    ) -> BoxFuture<'a, Result<(), IngressError>>;
}

pub(super) struct Runtime {
    pub mesh: MeshState,
    pub renewals: Arc<dyn RenewalReceiver>,
    pub dispatcher: Arc<dyn super::ReverseDispatcher>,
    pub outbox: tokio::sync::OnceCell<Arc<crate::cloud_outbox::CloudOutbox>>,
    pub policy: Arc<super::reverse::InstalledPolicy>,
}

#[derive(Serialize, Deserialize)]
struct Introduction {
    party: Party,
    signature: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Registry {
        signed_registry: DetachedGrant,
    },
    Renewal {
        membership: DetachedGrant,
        signed_registry: DetachedGrant,
    },
    Attach,
    Endpoint,
    Relay {
        command: crate::types::RelayCommand,
        signed_token: String,
    },
}

impl From<AdmissionRefusal> for IngressError {
    fn from(_: AdmissionRefusal) -> Self {
        Self::Rejected
    }
}

fn intro_digest(binding: &[u8; 32], role: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"domios-cloud-enrolled-introduction-v1\0");
    digest.update(role);
    digest.update(binding);
    digest.finalize().into()
}

fn verify_intro(
    mesh: &MeshManager,
    id: &str,
    digest: &[u8; 32],
    signature: &str,
) -> Result<(), IngressError> {
    let local = mesh.admission_context()?;
    let state = mesh.account_state();
    let (key, _, _) = state
        .registry_members
        .as_ref()
        .and_then(|m| m.get(id))
        .ok_or(IngressError::Rejected)?;
    if signature.len() > 2048
        || state.revoked_installations.contains(id)
        || state.unavailable_installations.contains(id)
        || !local
            .authority
            .verify_key_possession(key, digest, signature)
    {
        return Err(IngressError::Rejected);
    }
    Ok(())
}

pub struct EnrolledChannel<S = ClientStream> {
    pub(super) stream: EncryptedStream<S>,
    pub(super) mesh: MeshState,
    pub(super) peer: VerifiedIngressPeer,
}

impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> EnrolledChannel<S> {
    pub async fn send_relay(
        mut self,
        command: crate::types::RelayCommand,
        signed_token: String,
    ) -> Result<crate::types::RelayResponse, crate::transport::TransportError> {
        use crate::transport::{TransportError, TransportErrorKind};
        self.admit().map_err(|_| {
            TransportError::new(TransportErrorKind::NothingSent, "cloud admission closed")
        })?;
        let result = tokio::time::timeout(Duration::from_secs(120), async {
            self.stream
                .write(&Request::Relay {
                    command,
                    signed_token,
                })
                .await?;
            let reply = self.stream.read().await?;
            self.admit()?;
            Ok::<_, IngressError>(reply)
        })
        .await;
        match result {
            Ok(Ok(reply)) => Ok(reply),
            _ => Err(TransportError::new(
                TransportErrorKind::SubmitUnconfirmed,
                "cloud relay receipt unavailable",
            )),
        }
    }
    pub async fn publish_endpoint(mut self, expected: &CloudRoute) -> Result<(), IngressError> {
        use crate::endpoint_gossip::{
            receive_endpoint_records, EndpointAddress, SignedEndpointRecord,
        };
        self.admit()?;
        let record: SignedEndpointRecord = tokio::time::timeout(Duration::from_secs(30), async {
            self.stream.write(&Request::Endpoint).await?;
            self.stream.read().await
        })
        .await
        .map_err(|_| IngressError::Unavailable)??;
        self.admit()?;
        if record.claim.installation_id != self.peer.remote_membership().installation_id
            || record.claim.endpoint != Some(EndpointAddress::from(expected.clone()))
        {
            return Err(IngressError::Rejected);
        }
        let accepted = receive_endpoint_records(&self.mesh, vec![record.clone()]).await;
        self.admit()?;
        if accepted.is_empty()
            && self
                .mesh
                .lock()
                .verified_endpoint_record(&record.claim.installation_id)
                != Some(&record)
        {
            return Err(IngressError::Rejected);
        }
        if !accepted.is_empty() {
            crate::presence_notice::broadcast_endpoint_record(&self.mesh, record).await;
        }
        Ok(())
    }
    pub(super) fn admit(&self) -> Result<(), IngressError> {
        self.peer
            .peer_for_dispatch(&self.mesh.lock(), &self.stream.binding)?;
        Ok(())
    }

    pub async fn receive_reverse(
        mut self,
        dedupe: &crate::cloud_outbox::ReceiverDedupe,
        dispatcher: &dyn super::ReverseDispatcher,
    ) -> Result<(), IngressError> {
        self.admit()?;
        self.stream.write(&Request::Attach).await?;
        super::reverse::receive(self, dedupe, dispatcher).await
    }

    pub fn peer_membership(&self) -> Result<&VerifiedMembership, IngressError> {
        self.admit()?;
        Ok(self.peer.remote_membership())
    }

    pub fn peer_instance_id(&self) -> Result<&str, IngressError> {
        self.admit()?;
        Ok(&self.peer.remote_party().instance_id)
    }

    pub async fn renew(self) -> Result<(), IngressError> {
        let target = self.peer_membership()?;
        let authority = self
            .mesh
            .lock()
            .account_authority()
            .ok_or(IngressError::Rejected)?;
        let (membership, registry) = authority.renew_cloud(target).await?;
        self.push_renewal(membership, registry).await
    }

    pub async fn push_registry(
        mut self,
        signed_registry: DetachedGrant,
    ) -> Result<(), IngressError> {
        self.admit()?;
        tokio::time::timeout(Duration::from_secs(30), async {
            self.stream
                .write(&Request::Registry { signed_registry })
                .await?;
            self.stream.read::<()>().await?;
            self.admit()
        })
        .await
        .map_err(|_| IngressError::Unavailable)?
    }

    pub async fn push_renewal(
        mut self,
        membership: DetachedGrant,
        signed_registry: DetachedGrant,
    ) -> Result<(), IngressError> {
        self.admit()?;
        tokio::time::timeout(Duration::from_secs(30), async {
            self.stream
                .write(&Request::Renewal {
                    membership: membership.clone(),
                    signed_registry,
                })
                .await?;
            let proof: EnrolledProof = self.stream.read().await?;
            if !proof.presents(&membership) {
                return Err(IngressError::Rejected);
            }
            let mut mesh = self.mesh.lock();
            self.peer.peer_for_dispatch(&mesh, &self.stream.binding)?;
            let peer = mesh.verify_ingress_proof(
                &self.peer.remote_membership().installation_id,
                IngressRole::Responder,
                &self.stream.binding,
                &proof,
            )?;
            peer.link(&mut mesh, &proof)?;
            Ok(())
        })
        .await
        .map_err(|_| IngressError::Unavailable)?
    }
}

pub async fn connect_enrolled(
    mesh: MeshState,
    route: CloudRoute,
    installation_id: &str,
) -> Result<EnrolledChannel, IngressError> {
    tokio::time::timeout(Duration::from_secs(30), async {
        initiate(mesh, open(&route, PATH).await?, installation_id).await
    })
    .await
    .map_err(|_| IngressError::Unavailable)?
}

pub(crate) async fn initiate<S: ChannelIo>(
    mesh: MeshState,
    socket: tokio_tungstenite::WebSocketStream<S>,
    installation_id: &str,
) -> Result<EnrolledChannel<S>, IngressError> {
    let mut stream = EncryptedStream::handshake(socket, true).await?;
    let binding = stream.binding;
    let server_signature: String = stream.read().await?;
    let introduction = {
        let manager = mesh.lock();
        verify_intro(
            &manager,
            installation_id,
            &intro_digest(&binding, b"responder"),
            &server_signature,
        )?;
        let local = manager.admission_context()?;
        Introduction {
            party: local.party,
            signature: local
                .authority
                .sign_possession(&intro_digest(&binding, b"initiator"))?,
        }
    };
    stream.write(&introduction).await?;
    let server: EnrolledProof = stream.read().await?;
    let (peer, proof) = {
        let manager = mesh.lock();
        let peer = manager.verify_ingress_proof(
            installation_id,
            IngressRole::Responder,
            &binding,
            &server,
        )?;
        let proof = manager.sign_ingress_proof(&server.party, IngressRole::Initiator, &binding)?;
        (peer, proof)
    };
    stream.write(&proof).await?;
    stream.read::<()>().await?;
    peer.link(&mut mesh.lock(), &server)?;
    Ok(EnrolledChannel { stream, mesh, peer })
}

pub(crate) async fn accept<S: ChannelIo>(
    socket: tokio_tungstenite::WebSocketStream<S>,
    mesh: MeshState,
) -> Result<(EnrolledChannel<S>, EnrolledProof), IngressError> {
    let mut stream = EncryptedStream::handshake(socket, false).await?;
    let binding = stream.binding;
    let signature = mesh
        .lock()
        .admission_context()?
        .authority
        .sign_possession(&intro_digest(&binding, b"responder"))?;
    stream.write(&signature).await?;
    let intro: Introduction = stream.read().await?;
    let proof = {
        let manager = mesh.lock();
        verify_intro(
            &manager,
            &intro.party.installation_id,
            &intro_digest(&binding, b"initiator"),
            &intro.signature,
        )?;
        manager.sign_ingress_proof(&intro.party, IngressRole::Responder, &binding)?
    };
    stream.write(&proof).await?;
    let remote: EnrolledProof = stream.read().await?;
    if intro.party != remote.party {
        return Err(IngressError::Rejected);
    }
    let peer = {
        let mut manager = mesh.lock();
        let peer = manager.verify_ingress_proof(
            &intro.party.installation_id,
            IngressRole::Initiator,
            &binding,
            &remote,
        )?;
        peer.link(&mut manager, &remote)?;
        peer
    };
    stream.write(&()).await?;
    Ok((EnrolledChannel { stream, mesh, peer }, remote))
}

pub(super) async fn serve(
    socket: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    proxy_host: Option<String>,
    runtime: &Runtime,
) -> Result<(), IngressError> {
    if !runtime.renewals.enrolled() || !runtime.dispatcher.enabled() {
        return Err(IngressError::Rejected);
    }
    let local_address = socket
        .get_ref()
        .local_addr()
        .map_err(|_| IngressError::Unavailable)?;
    let (mut channel, remote) = tokio::time::timeout(
        Duration::from_secs(30),
        accept(socket, runtime.mesh.clone()),
    )
    .await
    .map_err(|_| IngressError::Unavailable)??;
    let request = tokio::time::timeout(Duration::from_secs(30), channel.stream.read::<Request>())
        .await
        .map_err(|_| IngressError::Unavailable)??;
    if !runtime.dispatcher.enabled() {
        return Err(IngressError::Rejected);
    }
    channel.admit()?;
    match request {
        Request::Relay {
            command,
            signed_token,
        } => {
            let sender = channel
                .peer
                .peer_for_dispatch(&channel.mesh.lock(), &channel.stream.binding)?;
            let keys = [(sender.instance_id.clone(), sender.mesh_public_key.clone())].into();
            crate::crown::verify_request_claims(&signed_token, &keys)
                .map_err(|_| IngressError::Rejected)?;
            let request = crate::cloud_outbox::OuterWireRequest {
                route: crate::cloud_outbox::OUTER_RELAY_ROUTE,
                body: serde_json::to_value(command).map_err(|_| IngressError::InvalidFrame)?,
            };
            channel.admit()?;
            let bytes = runtime
                .dispatcher
                .dispatch(request, signed_token)
                .await?
                .ok_or(IngressError::InvalidFrame)?;
            let reply: crate::types::RelayResponse =
                serde_json::from_slice(&bytes).map_err(|_| IngressError::InvalidFrame)?;
            channel.admit()?;
            channel.stream.write(&reply).await
        }
        Request::Endpoint => {
            use crate::endpoint_gossip::EndpointAddress;
            let endpoint = match proxy_host {
                Some(host) if super::provider_proxy_host_allowed(&host) => {
                    Some(EndpointAddress::ProviderProxy { host })
                }
                Some(_) => return Err(IngressError::Rejected),
                None => match local_address {
                    std::net::SocketAddr::V4(address) if super::direct_address_allowed(address) => {
                        Some(EndpointAddress::DirectIngress { address })
                    }
                    _ => return Err(IngressError::Rejected),
                },
            };
            let local = channel.mesh.lock().admission_context()?;
            let known = channel
                .mesh
                .lock()
                .verified_endpoint_record(&local.party.installation_id)
                .filter(|r| {
                    r.claim.endpoint == endpoint && r.claim.membership == local.membership.token
                })
                .cloned();
            let record = match known {
                Some(record) => record,
                None => {
                    let sequence = local
                        .authority
                        .reserve_endpoint_sequence(local.account_epoch)
                        .await?;
                    channel.admit()?;
                    channel.mesh.lock().sign_endpoint_record(
                        endpoint,
                        sequence,
                        &local.membership.token,
                    )?
                }
            };
            channel.admit()?;
            channel.stream.write(&record).await
        }
        Request::Registry { signed_registry } => {
            channel
                .peer
                .receive_registry_snapshot(&channel.mesh, &channel.stream.binding, &signed_registry)
                .await?;
            channel.stream.write(&()).await
        }
        Request::Attach => {
            let outbox = runtime.outbox().await?;
            super::reverse::serve(channel, outbox, runtime.policy.clone()).await
        }
        Request::Renewal {
            membership,
            signed_registry,
        } => {
            runtime
                .renewals
                .apply(&membership, &signed_registry)
                .await?;
            let proof = {
                let mesh = channel.mesh.lock();
                let peer = mesh.verify_ingress_proof(
                    &remote.party.installation_id,
                    IngressRole::Initiator,
                    &channel.stream.binding,
                    &remote,
                )?;
                if !channel.peer.same_admission(&peer) {
                    return Err(IngressError::Rejected);
                }
                peer.peer_for_dispatch(&mesh, &channel.stream.binding)?;
                mesh.sign_ingress_proof(
                    &remote.party,
                    IngressRole::Responder,
                    &channel.stream.binding,
                )?
            };
            channel.stream.write(&proof).await
        }
    }
}

impl Runtime {
    pub(super) async fn outbox(
        &self,
    ) -> Result<Arc<crate::cloud_outbox::CloudOutbox>, IngressError> {
        self.outbox
            .get_or_try_init(|| async {
                crate::cloud_outbox::CloudOutbox::open(
                    codeterm_paths::data_dir().join("mesh/cloud-outbox"),
                    crate::cloud_outbox::CloudOutboxConfig {
                        policy: self.policy.clone(),
                        ..Default::default()
                    },
                )
                .await
                .map(Arc::new)
                .map_err(|_| IngressError::Unavailable)
            })
            .await
            .cloned()
    }
}
