use serde::de::DeserializeOwned;
use serde::Serialize;

use super::ledger::LinkAdmission;
use super::ports::{AdmissionRefusal, BoxFuture, LinkAuthorizationRequest, LinkPermitExpectation};
use super::protocol::{
    AccountMeshPair, AdoptCommit, AdoptCommitReply, AdoptOffer, AdoptOfferReply, LinkApproval,
    LinkApprovalReply, LinkCommit, LinkCommitReply, LinkConfirm, LinkConfirmReply, LinkHello,
    LinkHelloReply, PartyDescriptor, ADOPT_COMMIT_PATH, ADOPT_OFFER_PATH, LINK_APPROVAL_PATH,
    LINK_COMMIT_PATH, LINK_CONFIRM_PATH, LINK_HELLO_PATH,
};
use super::transcript::{
    self, possession_digest, Party, Purpose, COMMIT_ROLE, INITIATOR_ROLE, RESPONDER_ROLE,
};
use crate::manager::{AdmissionContext, LinkOrigin};
use crate::types::PeerAddress;
use crate::MeshState;

/// One direct P2P request; never relayed through a backend.
pub trait LinkTransport: Send + Sync {
    fn post(
        &self,
        ip: &str,
        port: u16,
        path: &'static str,
        bearer: Option<String>,
        body: serde_json::Value,
    ) -> BoxFuture<'_, Result<serde_json::Value, AdmissionRefusal>>;
}

pub struct HttpLinkTransport;

impl LinkTransport for HttpLinkTransport {
    fn post(
        &self,
        ip: &str,
        port: u16,
        path: &'static str,
        bearer: Option<String>,
        body: serde_json::Value,
    ) -> BoxFuture<'_, Result<serde_json::Value, AdmissionRefusal>> {
        let url = format!(
            "http://{}{}",
            std::net::SocketAddr::new(parse_ip(ip), port),
            path
        );
        Box::pin(async move {
            let mut request = crate::transport::shared_http_client()
                .post(&url)
                .json(&body);
            if path == LINK_APPROVAL_PATH {
                request = request.timeout(std::time::Duration::from_secs(
                    super::approval::APPROVAL_TTL_SECS + 10,
                ));
            }
            if let Some(token) = bearer {
                request = request.header("Authorization", format!("Bearer {token}"));
            }
            let response = request
                .send()
                .await
                .map_err(|_| AdmissionRefusal::PeerUnreachable)?;
            let status = response.status();
            let value: serde_json::Value = response
                .json()
                .await
                .map_err(|_| AdmissionRefusal::handshake("peer reply unreadable"))?;
            if status.is_success() {
                return Ok(value);
            }
            if status.as_u16() == 404 {
                return Err(AdmissionRefusal::handshake(
                    "peer does not support account links",
                ));
            }
            Err(serde_json::from_value(value)
                .unwrap_or_else(|_| AdmissionRefusal::handshake(format!("peer refused: {status}"))))
        })
    }
}

fn parse_ip(ip: &str) -> std::net::IpAddr {
    ip.parse()
        .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED))
}

fn encode<T: Serialize>(value: &T) -> Result<serde_json::Value, AdmissionRefusal> {
    serde_json::to_value(value).map_err(|e| AdmissionRefusal::handshake(e.to_string()))
}

fn decode<T: DeserializeOwned>(value: serde_json::Value) -> Result<T, AdmissionRefusal> {
    serde_json::from_value(value).map_err(|_| AdmissionRefusal::handshake("malformed peer reply"))
}

fn fence(mesh: &MeshState, context: &AdmissionContext) -> Result<(), AdmissionRefusal> {
    mesh.lock().fence_admission(context)
}

pub async fn link_installation(
    mesh: &MeshState,
    installation_id: &str,
) -> Result<AccountMeshPair, AdmissionRefusal> {
    link_installation_with(mesh, &HttpLinkTransport, installation_id).await
}

/// Initiator of a NEW link: registry target, mutual nonce, both online permits, then persist.
pub async fn link_installation_with(
    mesh: &MeshState,
    transport: &dyn LinkTransport,
    installation_id: &str,
) -> Result<AccountMeshPair, AdmissionRefusal> {
    let (context, local) = {
        let manager = mesh.lock();
        let context = manager.admission_context()?;
        let local = manager.local_descriptor(context.party.clone());
        (context, local)
    };
    if installation_id == context.party.installation_id {
        return Err(AdmissionRefusal::handshake("cannot link this installation"));
    }
    let target = context
        .authority
        .resolve_link_target(installation_id)
        .await?;
    fence(mesh, &context)?;
    if target.installation_id != installation_id || target.api_port == 0 {
        return Err(AdmissionRefusal::handshake(
            "registry returned another installation",
        ));
    }

    let link_id = uuid::Uuid::new_v4().to_string();
    let nonce = transcript::fresh_nonce();
    let hello = LinkHello {
        link_id: link_id.clone(),
        initiator: local,
        membership: context.membership.token.clone(),
        nonce: nonce.clone(),
    };
    let reply: LinkHelloReply = decode(
        transport
            .post(
                &target.ip,
                target.api_port,
                LINK_HELLO_PATH,
                None,
                encode(&hello)?,
            )
            .await?,
    )?;
    fence(mesh, &context)?;
    if reply.link_id != link_id || reply.responder.party.installation_id != installation_id {
        return Err(AdmissionRefusal::handshake(
            "peer answered for another link",
        ));
    }
    let responder = reply.responder.party.clone();
    let remote = {
        let manager = mesh.lock();
        manager.check_link_identity(&responder.instance_id, &responder.mesh_public_key)?;
        verify_membership(
            &context,
            &reply.membership,
            &responder.installation_id,
            manager.now(),
        )?
    };
    let digest = transcript::transcript(
        Purpose::Link,
        &context.party,
        &responder,
        &context.account_id,
        &nonce,
        &reply.nonce,
    );
    verify_responder_proof(
        &context,
        &responder,
        &remote.verified,
        &digest,
        &reply.installation_signature,
        &reply.mesh_signature,
    )?;

    let approval = LinkApproval {
        link_id: link_id.clone(),
        installation_signature: context
            .authority
            .sign_possession(&possession_digest(&digest, INITIATOR_ROLE))?,
        mesh_signature: transcript::sign_mesh(&context.private_key, &digest, INITIATOR_ROLE)
            .map_err(AdmissionRefusal::handshake)?,
    };
    let approved: LinkApprovalReply = decode(
        transport
            .post(
                &target.ip,
                target.api_port,
                LINK_APPROVAL_PATH,
                None,
                encode(&approval)?,
            )
            .await?,
    )?;
    fence(mesh, &context)?;
    if !approved.approved {
        return Err(AdmissionRefusal::Declined);
    }
    let permit = context
        .authority
        .authorize_link(LinkAuthorizationRequest {
            transcript: digest,
            local_installation_id: context.party.installation_id.clone(),
            peer_installation_id: responder.installation_id.clone(),
            account_id: context.account_id.clone(),
            link_nonce: link_id.clone(),
        })
        .await?;
    fence(mesh, &context)?;
    let confirm = LinkConfirm {
        link_id: link_id.clone(),
        installation_signature: context
            .authority
            .sign_possession(&possession_digest(&digest, INITIATOR_ROLE))?,
        mesh_signature: transcript::sign_mesh(&context.private_key, &digest, INITIATOR_ROLE)
            .map_err(AdmissionRefusal::handshake)?,
        permit,
    };
    let confirmed: LinkConfirmReply = decode(
        transport
            .post(
                &target.ip,
                target.api_port,
                LINK_CONFIRM_PATH,
                None,
                encode(&confirm)?,
            )
            .await?,
    )?;

    {
        let manager = mesh.lock();
        manager.fence_admission(&context)?;
        context.authority.verify_link_permit(
            &confirmed.permit,
            &LinkPermitExpectation {
                transcript: digest,
                holder_installation_id: responder.installation_id.clone(),
                peer_installation_id: context.party.installation_id.clone(),
                account_id: context.account_id.clone(),
            },
            manager.now(),
        )?;
    }
    let commit = LinkCommit {
        link_id: link_id.clone(),
        mesh_signature: transcript::sign_mesh(&context.private_key, &digest, COMMIT_ROLE)
            .map_err(AdmissionRefusal::handshake)?,
    };
    let committed: LinkCommitReply = decode(
        transport
            .post(
                &target.ip,
                target.api_port,
                LINK_COMMIT_PATH,
                None,
                encode(&commit)?,
            )
            .await?,
    )?;
    if !committed.committed {
        return Err(AdmissionRefusal::handshake("peer did not commit the link"));
    }

    let mut manager = mesh.lock();
    manager.fence_admission(&context)?;
    let endpoint = [PeerAddress::from_ip(&target.ip)];
    let descriptor = PartyDescriptor {
        api_port: target.api_port,
        ..reply.responder
    };
    manager.admit_new_link(
        &descriptor,
        &endpoint,
        remote,
        &context.account_id,
        LinkOrigin::LocalAdd {
            removal_watermark: context.removal_watermark,
        },
        &link_id,
    )?;
    manager
        .account_mesh_pairs()
        .into_iter()
        .find(|pair| pair.instance_id == responder.instance_id)
        .ok_or_else(|| AdmissionRefusal::handshake("link not recorded"))
}

fn verify_membership(
    context: &AdmissionContext,
    token: &str,
    installation_id: &str,
    now: u64,
) -> Result<super::ports::SignedMembership, AdmissionRefusal> {
    let verified = context.authority.verify_membership(token, now)?;
    if verified.account_id != context.account_id
        || verified.installation_id != installation_id
        || !verified.is_current(now)
    {
        return Err(AdmissionRefusal::handshake(
            "peer membership does not match",
        ));
    }
    Ok(super::ports::SignedMembership {
        token: token.to_string(),
        verified,
    })
}

fn verify_responder_proof(
    context: &AdmissionContext,
    responder: &Party,
    membership: &super::ports::VerifiedMembership,
    digest: &[u8; 32],
    installation_signature: &str,
    mesh_signature: &str,
) -> Result<(), AdmissionRefusal> {
    if !transcript::verify_mesh(
        &responder.mesh_public_key,
        digest,
        RESPONDER_ROLE,
        mesh_signature,
    ) {
        return Err(AdmissionRefusal::handshake("responder mesh proof invalid"));
    }
    if !context.authority.verify_possession(
        membership,
        &possession_digest(digest, RESPONDER_ROLE),
        installation_signature,
    ) {
        return Err(AdmissionRefusal::handshake(
            "responder installation proof invalid",
        ));
    }
    Ok(())
}

pub async fn adopt_peer(mesh: &MeshState, peer_id: &str) -> Result<(), AdmissionRefusal> {
    adopt_peer_with(mesh, &HttpLinkTransport, peer_id).await
}

/// Bilateral in-place adoption of an open legacy edge over its existing authenticated channel.
/// Single-flight per peer; a peer that refused is not offered again until something changes.
pub async fn adopt_peer_with(
    mesh: &MeshState,
    transport: &dyn LinkTransport,
    peer_id: &str,
) -> Result<(), AdmissionRefusal> {
    if !mesh.lock().begin_adoption(peer_id) {
        return Ok(());
    }
    let outcome = offer_adoption(mesh, transport, peer_id).await;
    mesh.lock().finish_adoption(peer_id, &outcome);
    outcome
}

async fn offer_adoption(
    mesh: &MeshState,
    transport: &dyn LinkTransport,
    peer_id: &str,
) -> Result<(), AdmissionRefusal> {
    let (context, peer_key, ip, port, bearer) = {
        let manager = mesh.lock();
        let context = manager.admission_context()?;
        manager.check_adoptable(peer_id, &context)?;
        if !matches!(
            manager.link_record(peer_id).map(|record| &record.admission),
            Some(LinkAdmission::Legacy { .. })
        ) {
            return Ok(());
        }
        let peer = manager
            .paired_peers
            .get(peer_id)
            .ok_or_else(|| AdmissionRefusal::handshake("peer is not paired"))?;
        let address = peer
            .addresses
            .iter()
            .min_by_key(|address| crate::types::address_tier(address))
            .ok_or(AdmissionRefusal::PeerUnreachable)?
            .ip
            .clone();
        let bearer = manager
            .sign_request_for(peer_id)
            .map_err(AdmissionRefusal::handshake)?;
        (
            context,
            peer.public_key.clone(),
            address,
            peer.api_port,
            bearer,
        )
    };
    let adoption_id = uuid::Uuid::new_v4().to_string();
    let nonce = transcript::fresh_nonce();
    let offer = AdoptOffer {
        adoption_id: adoption_id.clone(),
        installation_id: context.party.installation_id.clone(),
        membership: context.membership.token.clone(),
        nonce: nonce.clone(),
    };
    let reply: AdoptOfferReply = decode(
        transport
            .post(&ip, port, ADOPT_OFFER_PATH, Some(bearer), encode(&offer)?)
            .await?,
    )?;
    fence(mesh, &context)?;
    if reply.adoption_id != adoption_id {
        return Err(AdmissionRefusal::handshake(
            "peer answered for another adoption",
        ));
    }
    let now = mesh.lock().now();
    let remote = verify_membership(&context, &reply.membership, &reply.installation_id, now)?;
    let responder = Party {
        instance_id: peer_id.to_string(),
        mesh_public_key: peer_key,
        installation_id: reply.installation_id.clone(),
    };
    let digest = transcript::transcript(
        Purpose::Adopt,
        &context.party,
        &responder,
        &context.account_id,
        &nonce,
        &reply.nonce,
    );
    verify_responder_proof(
        &context,
        &responder,
        &remote.verified,
        &digest,
        &reply.installation_signature,
        &reply.mesh_signature,
    )?;
    let commit = AdoptCommit {
        adoption_id,
        installation_signature: context
            .authority
            .sign_possession(&possession_digest(&digest, INITIATOR_ROLE))?,
        mesh_signature: transcript::sign_mesh(&context.private_key, &digest, INITIATOR_ROLE)
            .map_err(AdmissionRefusal::handshake)?,
    };
    let bearer = mesh
        .lock()
        .sign_request_for(peer_id)
        .map_err(AdmissionRefusal::handshake)?;
    let committed: AdoptCommitReply = decode(
        transport
            .post(&ip, port, ADOPT_COMMIT_PATH, Some(bearer), encode(&commit)?)
            .await?,
    )?;
    if !committed.adopted {
        return Err(AdmissionRefusal::handshake("peer did not adopt"));
    }
    mesh.lock()
        .commit_local_adoption(peer_id, &context, &reply.installation_id, remote)
}

/// Run after every applied account transition: renew peers' view of our membership,
/// then offer adoption on each open legacy edge. Old peers simply refuse the offer.
pub async fn on_account_state_applied(mesh: MeshState) {
    let _ = crate::broadcast_announce(&mesh, None).await;
    let legacy: Vec<String> = {
        let manager = mesh.lock();
        if manager.admission_context().is_err() {
            return;
        }
        manager
            .paired_peers
            .keys()
            .filter(|id| manager.is_legacy_link(id) && manager.traffic_open(id))
            .cloned()
            .collect()
    };
    for peer_id in legacy {
        if let Err(refusal) = adopt_peer(&mesh, &peer_id).await {
            log::debug!(
                "[mesh/account] adoption of {} deferred: {}",
                peer_id,
                refusal
            );
        }
    }
}
