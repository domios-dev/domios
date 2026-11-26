use crate::account_mesh::{
    transcript::{self, Party, INITIATOR_ROLE, RESPONDER_ROLE},
    AccountMeshAuthority, AdmissionRefusal, VerifiedMembership,
};
use crate::MeshManager;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub enum IngressRole {
    Initiator,
    Responder,
}

impl IngressRole {
    fn label(self) -> &'static [u8] {
        match self {
            Self::Initiator => INITIATOR_ROLE,
            Self::Responder => RESPONDER_ROLE,
        }
    }

    fn digest(
        self,
        signer: &Party,
        other: &Party,
        account: &str,
        binding: &[u8; 32],
        membership: &str,
    ) -> [u8; 32] {
        let (initiator, responder) = match self {
            Self::Initiator => (signer, other),
            Self::Responder => (other, signer),
        };
        transcript::ingress_transcript(initiator, responder, account, binding, membership)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrolledProof {
    pub party: Party,
    membership: String,
    installation_signature: String,
    mesh_signature: String,
}

impl EnrolledProof {
    pub(super) fn presents(&self, grant: &codeterm_entitlements::DetachedGrant) -> bool {
        serde_json::from_str::<codeterm_entitlements::DetachedGrant>(&self.membership)
            .ok()
            .as_ref()
            == Some(grant)
    }
}

pub struct VerifiedIngressPeer {
    local: Party,
    remote: Party,
    local_membership: VerifiedMembership,
    remote_membership: VerifiedMembership,
    epoch: u64,
    authority: Arc<dyn AccountMeshAuthority>,
    access_epoch: u64,
    peer_grant_epoch: u64,
    binding: [u8; 32],
}

impl MeshManager {
    pub fn sign_ingress_proof(
        &self,
        other: &Party,
        signer_role: IngressRole,
        binding: &[u8; 32],
    ) -> Result<EnrolledProof, AdmissionRefusal> {
        let local = self.admission_context()?;
        self.check_link_identity(&other.instance_id, &other.mesh_public_key)?;
        let digest = signer_role.digest(
            &local.party,
            other,
            &local.account_id,
            binding,
            &local.membership.token,
        );
        let role = signer_role.label();
        Ok(EnrolledProof {
            installation_signature: local
                .authority
                .sign_possession(&transcript::possession_digest(&digest, role))?,
            mesh_signature: transcript::sign_mesh(&local.private_key, &digest, role)
                .map_err(AdmissionRefusal::handshake)?,
            party: local.party,
            membership: local.membership.token,
        })
    }

    pub fn verify_ingress_proof(
        &self,
        expected_installation: &str,
        signer_role: IngressRole,
        binding: &[u8; 32],
        proof: &EnrolledProof,
    ) -> Result<VerifiedIngressPeer, AdmissionRefusal> {
        if proof.party.installation_id != expected_installation
            || proof.membership.len() > 16_384
            || proof.installation_signature.len() > 2048
            || proof.mesh_signature.len() > 128
        {
            return Err(AdmissionRefusal::AuthRefused);
        }
        let local = self.admission_context()?;
        self.check_link_identity(&proof.party.instance_id, &proof.party.mesh_public_key)?;
        let remote =
            self.verify_peer_membership(&local, &proof.membership, expected_installation)?;
        let digest = signer_role.digest(
            &proof.party,
            &local.party,
            &local.account_id,
            binding,
            &proof.membership,
        );
        let role = signer_role.label();
        if !transcript::verify_mesh(
            &proof.party.mesh_public_key,
            &digest,
            role,
            &proof.mesh_signature,
        ) || !local.authority.verify_possession(
            &remote.verified,
            &transcript::possession_digest(&digest, role),
            &proof.installation_signature,
        ) {
            return Err(AdmissionRefusal::AuthRefused);
        }
        let verified = VerifiedIngressPeer {
            local: local.party,
            remote: proof.party.clone(),
            local_membership: local.membership.verified,
            remote_membership: remote.verified,
            epoch: local.account_epoch,
            binding: *binding,
            authority: local.authority,
            access_epoch: self
                .remote_access_epoch()
                .ok_or(AdmissionRefusal::Cancelled)?,
            peer_grant_epoch: self.peer_grant_epoch(&proof.party.instance_id),
        };
        verified.check_memberships(self)?;
        Ok(verified)
    }
}

impl VerifiedIngressPeer {
    pub(super) fn link(
        &self,
        mesh: &mut MeshManager,
        proof: &EnrolledProof,
    ) -> Result<(), AdmissionRefusal> {
        self.check_memberships(mesh)?;
        if let Some(record) = mesh.link_record(&self.remote.instance_id) {
            if record.installation_id() != Some(self.remote.installation_id.as_str()) {
                return Err(AdmissionRefusal::AuthRefused);
            }
            mesh.refresh_peer_membership(&self.remote.instance_id, &proof.membership);
            return self.peer_for_dispatch(mesh, &self.binding).map(|_| ());
        }
        let descriptor = crate::account_mesh::protocol::PartyDescriptor {
            party: self.remote.clone(),
            instance_name: self.remote.instance_id.clone(),
            api_port: 0,
            addresses: Vec::new(),
        };
        mesh.admit_new_link(
            &descriptor,
            &[],
            crate::account_mesh::SignedMembership {
                token: proof.membership.clone(),
                verified: self.remote_membership.clone(),
            },
            &self.local_membership.account_id,
            &uuid::Uuid::new_v4().to_string(),
        )?;
        self.peer_for_dispatch(mesh, &self.binding).map(|_| ())
    }

    pub(super) fn remote_membership(&self) -> &VerifiedMembership {
        &self.remote_membership
    }

    pub(super) fn local_id(&self) -> &str {
        &self.local.installation_id
    }

    pub(super) fn same_admission(&self, next: &Self) -> bool {
        self.local == next.local
            && self.remote == next.remote
            && self.binding == next.binding
            && self.epoch == next.epoch
            && self.access_epoch == next.access_epoch
            && self.peer_grant_epoch == next.peer_grant_epoch
            && Arc::ptr_eq(&self.authority, &next.authority)
    }

    pub async fn receive_registry_snapshot(
        &self,
        mesh: &crate::MeshState,
        binding: &[u8; 32],
        snapshot: &codeterm_entitlements::DetachedGrant,
    ) -> Result<bool, AdmissionRefusal> {
        self.peer_for_dispatch(&mesh.lock(), binding)?;
        let applied = self.authority.receive_registry_snapshot(snapshot).await?;
        // Applying the issuer's snapshot can revoke the sender or this installation.
        self.peer_for_dispatch(&mesh.lock(), binding)?;
        Ok(applied)
    }

    fn check_memberships(&self, mesh: &MeshManager) -> Result<(), AdmissionRefusal> {
        let state = mesh.account_state();
        if state.account_epoch != self.epoch
            || mesh
                .account_authority()
                .is_none_or(|a| !Arc::ptr_eq(&a, &self.authority))
            || mesh.remote_access_epoch() != Some(self.access_epoch)
            || mesh.peer_grant_epoch(&self.remote.instance_id) != self.peer_grant_epoch
            || state.signed_in_account() != Some(self.local_membership.account_id.as_str())
            || state
                .current_membership(mesh.now())
                .is_none_or(|m| m.verified != self.local_membership)
        {
            return Err(AdmissionRefusal::Cancelled);
        }
        for membership in [&self.local_membership, &self.remote_membership] {
            if !membership.is_current(mesh.now())
                || state
                    .revoked_installations
                    .contains(&membership.installation_id)
                || state
                    .unavailable_installations
                    .contains(&membership.installation_id)
            {
                return Err(AdmissionRefusal::Revoked);
            }
            mesh.check_registry_membership(membership)?;
        }
        Ok(())
    }

    pub fn peer_for_dispatch<'a>(
        &'a self,
        mesh: &MeshManager,
        binding: &[u8; 32],
    ) -> Result<&'a Party, AdmissionRefusal> {
        self.check_memberships(mesh)?;
        if binding != &self.binding
            || mesh.ident.identity.instance_id != self.local.instance_id
            || !mesh.traffic_open(&self.remote.instance_id)
            || mesh
                .link_record(&self.remote.instance_id)
                .and_then(|link| link.installation_id())
                != Some(self.remote.installation_id.as_str())
            || mesh
                .paired_peers
                .get(&self.remote.instance_id)
                .is_none_or(|peer| peer.public_key != self.remote.mesh_public_key)
        {
            return Err(AdmissionRefusal::AuthRefused);
        }
        Ok(&self.remote)
    }
}
