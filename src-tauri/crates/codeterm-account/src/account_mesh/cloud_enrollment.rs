use super::key::{jcs_sha256_hex, sha256_hex};
use super::membership::{
    signed_payload_limit, verify_membership, MembershipExpectation, MembershipTrust,
    VerifiedMembership,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use codeterm_entitlements::DetachedGrant;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

pub const MAX_ENROLL_APPLY_BYTES: usize = 60 * 1024;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Discovery {
    pub cloud_installation_id: String,
    pub cloud_installation_key: String,
    pub mesh_instance_id: String,
    pub mesh_public_key: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentBody {
    pub schema_version: u8,
    pub controller_installation_id: String,
    pub cloud_installation_id: String,
    pub cloud_installation_key: String,
    pub mesh_instance_id: String,
    pub mesh_public_key: String,
    pub bootstrap_nonce_sha256: String,
    pub channel_binding: String,
    pub provider: String,
    pub size: String,
    pub request_id: String,
    pub timestamp: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_machine_id: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentRequest {
    pub body: EnrollmentBody,
    pub controller_signature: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentReply {
    pub membership: DetachedGrant,
    pub binding: DetachedGrant,
    pub signed_registry: DetachedGrant,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Apply {
    pub request: EnrollmentRequest,
    pub reply: EnrollmentReply,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    schema_version: u8,
    iss: String,
    aud: Audience,
    policy_epoch: String,
    account_id: String,
    controller_key: String,
    body: EnrollmentBody,
    membership_sha256: String,
    signed_registry_sha256: String,
    issued_at: u64,
    expires_at: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Audience {
    project: String,
    env: String,
}

pub fn installation_key_id(raw: &[u8; 32]) -> Option<String> {
    let key = VerifyingKey::from_bytes(raw).ok()?;
    Some(sha256_hex(&super::key::spki_der(&key)))
}

pub fn nonce_commitment(nonce: &[u8; 32]) -> String {
    sha256_hex(nonce)
}

pub fn member_identity(
    membership: &DetachedGrant,
    trust: &MembershipTrust,
    account: &str,
    discovery: &Discovery,
    now: u64,
) -> Option<crate::Identity> {
    let key_id = installation_key_id(&fixed_bytes(&discovery.cloud_installation_key)?)?;
    verify_membership(
        membership,
        trust,
        &MembershipExpectation {
            account_id: account,
            installation_id: Some(&discovery.cloud_installation_id),
            key_id: Some(&key_id),
            ..Default::default()
        },
        now,
    )
    .ok()?;
    let payload = signed_payload_limit(membership, trust, 2048).ok()?;
    Some(crate::Identity {
        uid: payload.get("sub")?.as_str()?.to_owned(),
        email: None,
        display_name: None,
        photo_url: None,
        provider: "delegated_cloud".into(),
    })
}

pub fn enrollment_message(
    project: &str,
    account_id: &str,
    body: &EnrollmentBody,
) -> Option<Vec<u8>> {
    let value = serde_json::json!({"domain":"domios.cloud-enroll.v1","project":project,"account_id":account_id,"body":body});
    codeterm_entitlements::jcs::encode(&value)
        .ok()
        .map(String::into_bytes)
}

pub fn fixed_bytes<const N: usize>(value: &str) -> Option<[u8; N]> {
    if value.len() != (N * 8).div_ceil(6) {
        return None;
    }
    URL_SAFE_NO_PAD.decode(value).ok()?.try_into().ok()
}

pub struct ExpectedApply<'a> {
    pub local: &'a Discovery,
    pub installation_key_id: &'a str,
    pub controller: &'a [u8; 32],
    pub channel_binding: &'a [u8; 32],
    pub nonce: &'a [u8; 32],
}

pub fn verify_apply(
    apply: &Apply,
    expected: &ExpectedApply<'_>,
    trust: &MembershipTrust,
    now: u64,
) -> Option<VerifiedMembership> {
    let grant: Binding =
        serde_json::from_value(signed_payload_limit(&apply.reply.binding, trust, 8192).ok()?)
            .ok()?;
    let body = &apply.request.body;
    if grant.schema_version != 1
        || body.schema_version != 1
        || grant.iss != "domios-cloud-enrollment"
        || grant.aud.project != trust.project()
        || grant.aud.env != trust.mode()
        || grant.policy_epoch != trust.policy_epoch()
        || grant.body != *body
        || grant.expires_at <= now
        || grant.issued_at > now.saturating_add(120)
        || grant.expires_at <= grant.issued_at
        || grant.expires_at > grant.issued_at.saturating_add(300)
        || now.abs_diff(body.timestamp) > 300
        || grant.controller_key != URL_SAFE_NO_PAD.encode(expected.controller)
        || grant.membership_sha256 != jcs_sha256_hex(&apply.reply.membership)?
        || grant.signed_registry_sha256 != jcs_sha256_hex(&apply.reply.signed_registry)?
        || body.cloud_installation_id != expected.local.cloud_installation_id
        || body.cloud_installation_key != expected.local.cloud_installation_key
        || body.mesh_instance_id != expected.local.mesh_instance_id
        || body.mesh_public_key != expected.local.mesh_public_key
        || body.bootstrap_nonce_sha256 != sha256_hex(expected.nonce)
        || body.channel_binding != URL_SAFE_NO_PAD.encode(expected.channel_binding)
        || fixed_bytes::<32>(&body.request_id).is_none()
        || body.cloud_installation_id == body.controller_installation_id
    {
        return None;
    }
    let message = enrollment_message(trust.project(), &grant.account_id, body)?;
    let signature = Signature::from_bytes(&fixed_bytes::<64>(&apply.request.controller_signature)?);
    VerifyingKey::from_bytes(expected.controller)
        .ok()?
        .verify_strict(&message, &signature)
        .ok()?;
    let membership = verify_membership(
        &apply.reply.membership,
        trust,
        &MembershipExpectation {
            account_id: &grant.account_id,
            installation_id: Some(&body.cloud_installation_id),
            key_id: Some(expected.installation_key_id),
            ..Default::default()
        },
        now,
    )
    .ok()
    .filter(|membership| grant.expires_at <= membership.expires_at)?;
    let registry = super::registry_snapshot::verify_registry(
        &apply.reply.signed_registry,
        trust,
        &grant.account_id,
        now,
    )
    .ok()?;
    allows_member(&registry, &membership).then_some(membership)
}

pub(super) fn allows_member(
    registry: &super::registry_snapshot::VerifiedRegistry,
    member: &VerifiedMembership,
) -> bool {
    registry.account_id == member.account_id
        && !registry
            .revoked_installations
            .contains(&member.installation_id)
        && registry.rows.iter().any(|row| {
            row.installation_id == member.installation_id
                && row.active
                && row.presence == super::wire::RegistryPresence::Present
                && row.eligibility == super::wire::Eligibility::Eligible
                && row.key_id == member.key_id
                && row.key_generation == member.key_generation
                && row.slot_revision == member.slot_revision
        })
}

pub fn verify_cloud_renewal(
    membership: &DetachedGrant,
    registry: &DetachedGrant,
    trust: &MembershipTrust,
    current: &VerifiedMembership,
    known: &super::registry_snapshot::VerifiedRegistry,
    now: u64,
) -> Option<(VerifiedMembership, u64)> {
    let expected = MembershipExpectation {
        account_id: &current.account_id,
        installation_id: Some(&current.installation_id),
        key_id: Some(&current.key_id),
        key_generation: Some(current.key_generation),
        slot_revision: Some(current.slot_revision),
    };
    let renewed = verify_membership(membership, trust, &expected, now).ok()?;
    let issued =
        super::registry_snapshot::verify_registry(registry, trust, &current.account_id, now)
            .ok()?;
    if !allows_member(known, current)
        || !allows_member(&issued, &renewed)
        || renewed.issued_at < current.issued_at
        || renewed.expires_at < current.expires_at
        || issued.revision < known.revision
        || (issued.revision == known.revision && *registry != known.grant)
    {
        return None;
    }
    Some((renewed, issued.revision))
}

pub struct CloudController<'a> {
    pub(super) mesh: &'a super::AccountMesh,
    pub(super) signed: super::operations::Signed,
}

impl super::AccountMesh {
    pub async fn publish_cloud_membership(
        &self,
        account: &str,
        grant: &DetachedGrant,
        issued_registry: &DetachedGrant,
    ) -> Result<(), super::AccountMeshRefusal> {
        self.publish_cloud_record(account, grant, issued_registry, false)
            .await
    }

    pub async fn restore_cloud_membership(
        &self,
        account: &str,
        grant: &DetachedGrant,
        issued_registry: &DetachedGrant,
    ) -> Result<(), super::AccountMeshRefusal> {
        self.publish_cloud_record(account, grant, issued_registry, true)
            .await
    }

    async fn publish_cloud_record(
        &self,
        account: &str,
        grant: &DetachedGrant,
        issued_registry: &DetachedGrant,
        restoring: bool,
    ) -> Result<(), super::AccountMeshRefusal> {
        use super::{AccountMeshRefusal as R, Enrollment, Presence, SignedMembership};
        let trust = self
            .config
            .trust
            .as_ref()
            .ok_or(R::VerificationUnavailable)?;
        let installation = self.installation();
        let key_id = self.key().await?.key_id();
        let member = super::verify_enrollment_record(
            grant,
            trust,
            &MembershipExpectation {
                account_id: account,
                installation_id: Some(&installation),
                key_id: Some(&key_id),
                ..Default::default()
            },
            self.now(),
        )
        .map_err(|_| R::ProofInvalid)?;
        if !restoring && !member.is_current(self.now()) {
            return Err(R::ProofInvalid);
        }
        let issued =
            super::registry_snapshot::verify_registry(issued_registry, trust, account, self.now())?;
        if !allows_member(&issued, &member) {
            return Err(R::Revoked);
        }
        let cached = self.with(|inner| {
            if inner.store_unreadable
                || inner.record.signed_out
                || inner
                    .record
                    .owner
                    .as_deref()
                    .is_some_and(|owner| owner != account)
                || inner.record.terminal_refusals.contains_key(account)
            {
                return Err(R::Forbidden);
            }
            match &inner.state.presence {
                Presence::NeverSignedIn => {
                    inner.record.owner = Some(account.into());
                    inner.update(&self.watch, |next| {
                        next.account_epoch += 1;
                        next.owner = Some(account.into());
                        next.presence = Presence::SignedIn {
                            account_id: account.into(),
                        };
                    });
                }
                Presence::SignedIn { account_id } if account_id == account => {}
                _ => return Err(R::Forbidden),
            }
            Ok(inner.record.signed_registries.get(account).cloned())
        })?;
        if let Some(cached) = cached {
            self.receive_registry_snapshot(&cached).await?;
        }
        self.receive_registry_snapshot(issued_registry).await?;
        self.with(|inner| {
            let state = &inner.state;
            if !restoring && !member.is_current(self.now()) {
                return Err(R::ProofInvalid);
            }
            if state.signed_in_account() != Some(account)
                || state.revoked_installations.contains(&installation)
                || state.signed_registry.as_ref().is_none_or(|r| {
                    !allows_member(r, &member)
                        || r.revision < issued.revision
                        || (r.revision == issued.revision && r.grant != issued.grant)
                })
            {
                return Err(R::Revoked);
            }
            if state
                .membership
                .as_ref()
                .is_some_and(|known| known.verified.issued_at > member.issued_at)
            {
                return Ok(());
            }
            inner.update(&self.watch, |next| {
                next.membership = Some(SignedMembership {
                    token: super::membership_token(grant),
                    verified: member,
                });
                next.enrollment = Enrollment::Enrolled;
            });
            Ok(())
        })?;
        self.push().await;
        Ok(())
    }
    pub async fn cloud_controller(&self) -> Result<CloudController<'_>, super::AccountMeshRefusal> {
        let signed = self.signed(None).await?;
        if self.state().current_membership(self.now()).is_none() {
            return Err(super::AccountMeshRefusal::Forbidden);
        }
        Ok(CloudController { mesh: self, signed })
    }
}

impl CloudController<'_> {
    pub fn public_key(&self) -> [u8; 32] {
        self.signed.key.public_bytes()
    }
    pub fn sign_channel_proof(&self, proof: &[u8]) -> Option<[u8; 64]> {
        self.signed.key.sign_cloud_channel_proof(proof)
    }
    pub fn installation_id(&self) -> String {
        self.mesh.installation()
    }

    pub async fn delegate(&self, body: EnrollmentBody) -> Result<Apply, super::AccountMeshRefusal> {
        use super::AccountMeshRefusal as R;
        let current = || {
            let view = self.mesh.state();
            view.account_epoch == self.signed.epoch
                && view.current_membership(self.mesh.now()).is_some()
        };
        if !current() || body.controller_installation_id != self.mesh.installation() {
            return Err(R::Cancelled);
        }
        let message = enrollment_message(self.signed.trust.project(), &self.signed.account, &body)
            .ok_or(R::Invalid)?;
        let controller_signature = self.signed.key.sign_cloud_enrollment(&message);
        let request = EnrollmentRequest {
            body,
            controller_signature,
        };
        let reply = self
            .mesh
            .call(
                self.mesh
                    .ports
                    .registry
                    .enroll_cloud(&self.signed.session, &request),
            )
            .await?;
        if !current() {
            return Err(R::Cancelled);
        }
        Ok(Apply { request, reply })
    }

    pub fn verify(&self, apply: &Apply, expected: &ExpectedApply<'_>) -> bool {
        verify_apply(apply, expected, &self.signed.trust, self.mesh.now())
            .is_some_and(|verified| verified.account_id == self.signed.account)
    }
}
