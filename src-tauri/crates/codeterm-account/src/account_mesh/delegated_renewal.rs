use super::{
    verify_membership, wire::MeshTarget, AccountMesh, AccountMeshRefusal, LifecycleState,
    MembershipExpectation,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use codeterm_entitlements::DetachedGrant;
use rand::RngCore;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenewalBody {
    pub schema_version: u8,
    pub controller_installation_id: String,
    pub cloud_installation_id: String,
    pub key_generation: u64,
    pub slot_revision: u64,
    pub request_id: String,
    pub timestamp: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenewalRequest {
    pub body: RenewalBody,
    pub controller_signature: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenewalReply {
    pub membership: DetachedGrant,
    pub signed_registry: DetachedGrant,
}

fn eligible(state: &LifecycleState, target: &MeshTarget, now: u64) -> bool {
    state
        .current_membership(now)
        .is_some_and(|own| own.verified.installation_id != target.installation_id)
        && !state
            .revoked_installations
            .contains(&target.installation_id)
        && !state
            .unavailable_installations
            .contains(&target.installation_id)
        && state.signed_registry.as_ref().is_some_and(|registry| {
            registry.rows.iter().any(|row| {
                row.active
                    && row.presence == super::wire::RegistryPresence::Present
                    && row.eligibility == super::wire::Eligibility::Eligible
                    && row.installation_id == target.installation_id
                    && row.key_id == target.key_id
                    && row.key_generation == target.key_generation
                    && row.slot_revision == target.slot_revision
            })
        })
}

impl AccountMesh {
    pub async fn renew_cloud_membership(
        &self,
        target: &MeshTarget,
    ) -> Result<RenewalReply, AccountMeshRefusal> {
        let signed = self.signed(None).await?;
        let now = self.now();
        if !eligible(&self.state(), target, now) {
            return Err(AccountMeshRefusal::Forbidden);
        }
        let mut nonce = [0; 32];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let body = RenewalBody {
            schema_version: 1,
            controller_installation_id: self.installation(),
            cloud_installation_id: target.installation_id.clone(),
            key_generation: target.key_generation,
            slot_revision: target.slot_revision,
            request_id: URL_SAFE_NO_PAD.encode(nonce),
            timestamp: now,
        };
        let controller_signature = signed
            .key
            .sign_renewal(&body, signed.trust.project(), &signed.account)
            .ok_or(AccountMeshRefusal::Invalid)?;
        let request = RenewalRequest {
            body,
            controller_signature,
        };
        let reply = self
            .call(self.ports.registry.renew_cloud(&signed.session, &request))
            .await?;
        let state = self.state();
        if state.account_epoch != signed.epoch || !eligible(&state, target, self.now()) {
            return Err(AccountMeshRefusal::Cancelled);
        }
        let membership = verify_membership(
            &reply.membership,
            &signed.trust,
            &MembershipExpectation {
                account_id: &signed.account,
                installation_id: Some(&target.installation_id),
                key_id: Some(&target.key_id),
                key_generation: Some(target.key_generation),
                slot_revision: Some(target.slot_revision),
            },
            self.now(),
        )
        .map_err(|_| AccountMeshRefusal::Invalid)?;
        self.receive_registry_snapshot(&reply.signed_registry)
            .await?;
        let state = self.state();
        let now = self.now();
        if state.account_epoch != signed.epoch || !eligible(&state, target, now) {
            return Err(AccountMeshRefusal::Cancelled);
        }
        let matches_issued_slot = state
            .signed_registry
            .as_ref()
            .is_some_and(|registry| registry.grant == reply.signed_registry);
        if !matches_issued_slot || !membership.is_current(now) {
            return Err(AccountMeshRefusal::Invalid);
        }
        Ok(reply)
    }
}
