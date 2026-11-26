use super::super::account_mesh::NeighborsExt;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use codeterm_account::account_mesh::{
    cloud_enrollment::{
        fixed_bytes, installation_key_id, nonce_commitment, Discovery, EnrollmentBody,
        ExpectedApply, MAX_ENROLL_APPLY_BYTES,
    },
    AccountMeshRefusal,
};
use codeterm_mesh::cloud_ingress::{
    connect_enrollment, CloudRoute, EnrollmentError, EnrollmentOperation, IngressError,
};
use codeterm_service_core::service::ServiceContext;
use rand::RngCore;
use serde::Deserialize;
use zeroize::Zeroizing;

pub struct CreationBinding {
    controller: [u8; 32],
    nonce: Zeroizing<[u8; 32]>,
}

impl CreationBinding {
    pub async fn new(service: &ServiceContext) -> Result<Self, ControllerError> {
        let mesh = service
            .account_mesh()
            .await
            .map_err(|_| ControllerError::Unavailable)?;
        let controller = mesh
            .mesh
            .cloud_controller()
            .await
            .map_err(ControllerError::Account)?
            .public_key();
        let mut nonce = Zeroizing::new([0u8; 32]);
        rand::rngs::OsRng.fill_bytes(&mut nonce[..]);
        Ok(Self { controller, nonce })
    }
    pub fn controller_key(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.controller)
    }
    pub fn provider_creation_nonce(&self) -> Zeroizing<String> {
        Zeroizing::new(URL_SAFE_NO_PAD.encode(&*self.nonce))
    }
}

#[derive(Debug)]
pub enum ControllerError {
    Account(AccountMeshRefusal),
    Ingress(IngressError),
    Enrollment(EnrollmentError),
    Unavailable,
    InvalidEvidence,
    ApplyUnconfirmed { installation_id: String },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Applied {
    signed_in: bool,
    installation_id: String,
}

pub async fn enroll(
    service: &ServiceContext,
    route: CloudRoute,
    binding: &CreationBinding,
    provider: &str,
    size: &str,
) -> Result<String, ControllerError> {
    let mesh = service
        .account_mesh()
        .await
        .map_err(|_| ControllerError::Unavailable)?;
    let controller = mesh
        .mesh
        .cloud_controller()
        .await
        .map_err(ControllerError::Account)?;
    if controller.public_key() != binding.controller {
        return Err(ControllerError::InvalidEvidence);
    }
    let mut client =
        connect_enrollment(route.clone(), &binding.nonce, binding.controller, |proof| {
            controller
                .sign_channel_proof(proof.as_bytes())
                .ok_or(IngressError::Rejected)
        })
        .await
        .map_err(ControllerError::Ingress)?;
    let bytes = client
        .request(EnrollmentOperation::Discover, &[])
        .await
        .map_err(ControllerError::Enrollment)?;
    let local: Discovery =
        serde_json::from_slice(&bytes).map_err(|_| ControllerError::InvalidEvidence)?;
    let remote = *client.session.remote_installation_key();
    let exporter = *client.session.channel_binding();
    if local.cloud_installation_key != URL_SAFE_NO_PAD.encode(remote)
        || fixed_bytes::<32>(&local.mesh_public_key).is_none()
        || local.cloud_installation_id.is_empty()
        || local.mesh_instance_id.is_empty()
    {
        return Err(ControllerError::InvalidEvidence);
    }
    let mut request_id = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut request_id);
    let body = EnrollmentBody {
        schema_version: 1,
        controller_installation_id: controller.installation_id(),
        cloud_installation_id: local.cloud_installation_id.clone(),
        cloud_installation_key: local.cloud_installation_key.clone(),
        mesh_instance_id: local.mesh_instance_id.clone(),
        mesh_public_key: local.mesh_public_key.clone(),
        bootstrap_nonce_sha256: nonce_commitment(&binding.nonce),
        channel_binding: URL_SAFE_NO_PAD.encode(exporter),
        provider: provider.into(),
        size: size.into(),
        request_id: URL_SAFE_NO_PAD.encode(request_id),
        timestamp: codeterm_service_core::service::neighbor_account::now(),
        provider_machine_id: None,
    };
    let apply = controller
        .delegate(body)
        .await
        .map_err(ControllerError::Account)?;
    let key_id = installation_key_id(&remote).ok_or(ControllerError::InvalidEvidence)?;
    if !controller.verify(
        &apply,
        &ExpectedApply {
            local: &local,
            installation_key_id: &key_id,
            controller: &binding.controller,
            channel_binding: &exporter,
            nonce: &binding.nonce,
        },
    ) {
        return Err(ControllerError::InvalidEvidence);
    }
    let bytes = serde_json::to_vec(&apply).map_err(|_| ControllerError::InvalidEvidence)?;
    if bytes.len() > MAX_ENROLL_APPLY_BYTES {
        return Err(ControllerError::InvalidEvidence);
    }
    mesh.mesh
        .receive_registry_snapshot(&apply.reply.signed_registry)
        .await
        .map_err(ControllerError::Account)?;
    let uncertain = || ControllerError::ApplyUnconfirmed {
        installation_id: local.cloud_installation_id.clone(),
    };
    let result = match client.request(EnrollmentOperation::Apply, &bytes).await {
        Ok(result) => result,
        Err(_) => {
            let channel = enrolled_channel(service, route, &local.cloud_installation_id)
                .await
                .map_err(|_| uncertain())?;
            channel.peer_membership().map_err(|_| uncertain())?;
            return Ok(local.cloud_installation_id);
        }
    };
    let ack: Applied = serde_json::from_slice(&result).map_err(|_| uncertain())?;
    if !ack.signed_in || ack.installation_id != local.cloud_installation_id {
        return Err(uncertain());
    }
    Ok(ack.installation_id)
}

async fn enrolled_channel(
    service: &ServiceContext,
    route: CloudRoute,
    id: &str,
) -> Result<codeterm_mesh::cloud_ingress::EnrolledChannel, ControllerError> {
    let mesh = service
        .try_ext::<crate::MeshStateHandle>()
        .cloned()
        .ok_or(ControllerError::Unavailable)?;
    codeterm_mesh::cloud_ingress::connect_enrolled(mesh, route, id)
        .await
        .map_err(ControllerError::Ingress)
}

pub async fn publish_direct_endpoint(
    service: &ServiceContext,
    route: CloudRoute,
    id: &str,
) -> Result<(), ControllerError> {
    let channel = match enrolled_channel(service, route.clone(), id).await {
        Ok(channel) => channel,
        Err(original @ ControllerError::Ingress(IngressError::TlsFailed)) => return Err(original),
        Err(original) => {
            let mesh = service
                .try_ext::<crate::MeshStateHandle>()
                .ok_or(ControllerError::Unavailable)?;
            match codeterm_mesh::cloud_ingress::recover_expired_cloud(mesh, route.clone(), id).await
            {
                Ok(()) => enrolled_channel(service, route.clone(), id).await?,
                Err(IngressError::Rejected) => return Err(original),
                Err(error) => return Err(ControllerError::Ingress(error)),
            }
        }
    };
    channel
        .publish_endpoint(&route)
        .await
        .map_err(ControllerError::Ingress)
}
