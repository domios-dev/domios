use serde::de::DeserializeOwned;
use serde::Serialize;

use super::ports::{AdmissionRefusal, BoxFuture, LinkAuthorizationRequest, LinkPermitExpectation};
use super::protocol::{
    AccountMeshPair, LinkCommit, LinkCommitReply, LinkConfirm, LinkConfirmReply, LinkHello,
    LinkHelloReply, PartyDescriptor, LINK_COMMIT_PATH, LINK_CONFIRM_PATH, LINK_HELLO_PATH,
};
use super::transcript::{
    self, possession_digest, Party, Purpose, COMMIT_ROLE, INITIATOR_ROLE, RESPONDER_ROLE,
};
use crate::manager::AdmissionContext;
use crate::types::PeerAddress;
use crate::MeshState;

/// One direct P2P request; never relayed through a backend.
pub trait LinkTransport: Send + Sync {
    fn post(
        &self,
        ip: &str,
        port: u16,
        path: &'static str,
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
        body: serde_json::Value,
    ) -> BoxFuture<'_, Result<serde_json::Value, AdmissionRefusal>> {
        let url = format!(
            "http://{}{}",
            match ip.parse::<std::net::IpAddr>() {
                Ok(ip) => std::net::SocketAddr::new(ip, port),
                Err(_) =>
                    return Box::pin(async {
                        Err(AdmissionRefusal::handshake("invalid account endpoint"))
                    }),
            },
            path
        );
        Box::pin(async move {
            let request = crate::transport::shared_http_client()
                .post(&url)
                .json(&body);
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
    let authority = mesh.lock().admission_context()?.authority;
    let target = authority.resolve_link_target(installation_id).await?;
    connect_target_with(mesh, transport, target).await
}

pub async fn connect_target_with(
    mesh: &MeshState,
    transport: &dyn LinkTransport,
    target: super::ports::LinkTarget,
) -> Result<AccountMeshPair, AdmissionRefusal> {
    let installation_id = target.installation_id.as_str();
    let (context, local) = {
        let manager = mesh.lock();
        let context = manager.admission_context()?;
        let local = manager.local_descriptor(context.party.clone());
        (context, local)
    };
    if installation_id == context.party.installation_id {
        return Err(AdmissionRefusal::handshake("cannot link this installation"));
    }
    fence(mesh, &context)?;
    if target.api_port == 0 {
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
