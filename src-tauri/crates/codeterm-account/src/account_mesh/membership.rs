use super::key::sha256_hex;
use super::wire::{MeshTarget, MESH_LINK_PERMIT_SECONDS};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use codeterm_entitlements::text::{scalar_text, valid_id, valid_installation_id};
use codeterm_entitlements::{DetachedGrant, Mode, Policy};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

pub const MEMBERSHIP_ISSUER: &str = "domios-mesh-membership";
pub const MAX_MEMBERSHIP_SECONDS: u64 = 3600;
pub const RENEW_AFTER_SECONDS: u64 = 1800;
/// Not-before leeway for a desktop clock behind the issuer; expiry is never extended. It stays
/// within one link-permit lifetime and far inside the renewal window.
pub const NOT_BEFORE_SKEW_SECONDS: u64 = 120;
const MAX_PAYLOAD_BYTES: usize = 2048;
const MAX_TOKEN_BYTES: usize = 4096;
const ACCOUNT_DOMAIN: &str = "domios.account-mesh.account.v1\0";

/// Opaque account identity for Mesh state: never the raw uid.
pub fn account_id(uid: &str) -> String {
    sha256_hex(format!("{ACCOUNT_DOMAIN}{uid}").as_bytes())
}

/// Release-pinned issuer trust; the same keys sign grants, but only a Mesh payload verifies here.
#[derive(Clone)]
pub struct MembershipTrust {
    project: String,
    env: &'static str,
    policy_epoch: String,
    keys: Vec<(String, VerifyingKey)>,
}

impl MembershipTrust {
    pub fn new(
        project: String,
        env: &'static str,
        policy_epoch: String,
        keys: Vec<(String, VerifyingKey)>,
    ) -> Self {
        Self {
            project,
            env,
            policy_epoch,
            keys,
        }
    }

    pub fn from_policy(policy: &Policy) -> Option<Self> {
        let spec = policy.spec();
        let env = match spec.mode {
            Mode::Test => "test",
            Mode::Live => "live",
        };
        let keys = spec
            .keys
            .iter()
            .map(|key| {
                codeterm_entitlements::policy::parse_spki(&key.spki_der_base64)
                    .map(|verifying| (key.key_id.clone(), verifying))
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self::new(
            spec.project.clone(),
            env,
            spec.policy_epoch.clone(),
            keys,
        ))
    }

    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn mode(&self) -> &'static str {
        self.env
    }
    pub fn policy_epoch(&self) -> &str {
        &self.policy_epoch
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedMembership {
    pub account_id: String,
    pub installation_id: String,
    pub key_id: String,
    pub key_generation: u64,
    pub slot_revision: u64,
    pub issued_at: u64,
    pub expires_at: u64,
}

impl VerifiedMembership {
    /// Expiry is exclusive: at `expires_at` the membership no longer admits traffic.
    pub fn is_current(&self, now: u64) -> bool {
        self.issued_at <= now.saturating_add(NOT_BEFORE_SKEW_SECONDS) && now < self.expires_at
    }
    pub fn refresh_after(&self) -> u64 {
        self.expires_at
            .min(self.issued_at.saturating_add(RENEW_AFTER_SECONDS))
    }
}

/// Fields a caller already knows; `None` leaves the field to be reported, never trusted blindly.
#[derive(Clone, Copy, Debug, Default)]
pub struct MembershipExpectation<'a> {
    pub account_id: &'a str,
    pub installation_id: Option<&'a str>,
    pub key_id: Option<&'a str>,
    pub key_generation: Option<u64>,
    pub slot_revision: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipRefusal {
    Token,
    Algorithm,
    UnknownKey,
    Encoding,
    Signature,
    Malformed,
    NonCanonical,
    Issuer,
    Audience,
    Epoch,
    Subject,
    Installation,
    Key,
    Slot,
    Window,
    NotYetValid,
    Expired,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Audience {
    project: String,
    env: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    schema_version: u64,
    iss: String,
    aud: Audience,
    sub: String,
    installation_id: String,
    key_id: String,
    key_generation: u64,
    slot_revision: u64,
    authority_fingerprint: String,
    policy_epoch: String,
    issued_at: u64,
    expires_at: u64,
}

fn hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn membership_token(grant: &DetachedGrant) -> String {
    serde_json::json!({
        "payload": grant.payload,
        "signature": grant.signature,
        "key_id": grant.key_id,
        "alg": grant.alg,
    })
    .to_string()
}

pub fn parse_membership_token(token: &str) -> Result<DetachedGrant, MembershipRefusal> {
    if token.len() > MAX_TOKEN_BYTES {
        return Err(MembershipRefusal::Token);
    }
    serde_json::from_str(token).map_err(|_| MembershipRefusal::Token)
}

/// Trusted key, strict signature and RFC 8785 canonical bytes; the payload schema is the caller's.
fn signed_payload(
    grant: &DetachedGrant,
    trust: &MembershipTrust,
) -> Result<serde_json::Value, MembershipRefusal> {
    use MembershipRefusal as R;
    if grant.alg != codeterm_entitlements::verify::ED25519 {
        return Err(R::Algorithm);
    }
    let key = trust
        .keys
        .iter()
        .find(|(id, _)| *id == grant.key_id)
        .map(|(_, key)| key)
        .ok_or(R::UnknownKey)?;
    let signature = URL_SAFE_NO_PAD
        .decode(&grant.signature)
        .ok()
        .and_then(|bytes| <[u8; 64]>::try_from(bytes.as_slice()).ok())
        .ok_or(R::Encoding)?;
    let bytes = URL_SAFE_NO_PAD
        .decode(&grant.payload)
        .map_err(|_| R::Encoding)?;
    if bytes.len() > MAX_PAYLOAD_BYTES || URL_SAFE_NO_PAD.encode(&bytes) != grant.payload {
        return Err(R::Encoding);
    }
    key.verify_strict(&bytes, &Signature::from_bytes(&signature))
        .map_err(|_| R::Signature)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| R::Malformed)?;
    let canonical = codeterm_entitlements::jcs::encode(&value).map_err(|_| R::NonCanonical)?;
    if canonical.as_bytes() != bytes.as_slice() {
        return Err(R::NonCanonical);
    }
    Ok(value)
}

fn audience(
    trust: &MembershipTrust,
    aud: &Audience,
    policy_epoch: &str,
) -> Result<(), MembershipRefusal> {
    if aud.project != trust.project || aud.env != trust.env {
        return Err(MembershipRefusal::Audience);
    }
    if policy_epoch != trust.policy_epoch || !scalar_text(policy_epoch, 128) {
        return Err(MembershipRefusal::Epoch);
    }
    Ok(())
}

fn window(issued_at: u64, expires_at: u64, max: u64, now: u64) -> Result<(), MembershipRefusal> {
    if expires_at <= issued_at || expires_at - issued_at > max {
        return Err(MembershipRefusal::Window);
    }
    if now.saturating_add(NOT_BEFORE_SKEW_SECONDS) < issued_at {
        return Err(MembershipRefusal::NotYetValid);
    }
    if now >= expires_at {
        return Err(MembershipRefusal::Expired);
    }
    Ok(())
}

pub fn verify_membership(
    grant: &DetachedGrant,
    trust: &MembershipTrust,
    expect: &MembershipExpectation<'_>,
    now: u64,
) -> Result<VerifiedMembership, MembershipRefusal> {
    use MembershipRefusal as R;
    let value = signed_payload(grant, trust)?;
    let payload: Payload = serde_json::from_value(value).map_err(|_| R::Malformed)?;
    if payload.schema_version != 1 || payload.iss != MEMBERSHIP_ISSUER {
        return Err(R::Issuer);
    }
    audience(trust, &payload.aud, &payload.policy_epoch)?;
    if !valid_id(&payload.sub) || account_id(&payload.sub) != expect.account_id {
        return Err(R::Subject);
    }
    if !valid_installation_id(&payload.installation_id)
        || expect
            .installation_id
            .is_some_and(|id| id != payload.installation_id)
    {
        return Err(R::Installation);
    }
    if !hex64(&payload.key_id)
        || !hex64(&payload.authority_fingerprint)
        || payload.key_generation < 1
        || expect.key_id.is_some_and(|id| id != payload.key_id)
        || expect
            .key_generation
            .is_some_and(|generation| generation != payload.key_generation)
    {
        return Err(R::Key);
    }
    if payload.slot_revision < 1
        || expect
            .slot_revision
            .is_some_and(|slot| slot != payload.slot_revision)
    {
        return Err(R::Slot);
    }
    window(
        payload.issued_at,
        payload.expires_at,
        MAX_MEMBERSHIP_SECONDS,
        now,
    )?;
    Ok(VerifiedMembership {
        account_id: expect.account_id.to_owned(),
        installation_id: payload.installation_id,
        key_id: payload.key_id,
        key_generation: payload.key_generation,
        slot_revision: payload.slot_revision,
        issued_at: payload.issued_at,
        expires_at: payload.expires_at,
    })
}

pub const LINK_PERMIT_ISSUER: &str = "domios-mesh-link";

/// Holder is the counterpart that proved its key in the transcript; peer is the verifier itself.
#[derive(Clone, Copy, Debug)]
pub struct LinkPermitExpectation<'a> {
    pub transcript: &'a [u8; 32],
    pub holder_installation_id: &'a str,
    pub peer_installation_id: &'a str,
    pub account_id: &'a str,
    /// The verifier's own current slot; `None` checks the installation id only.
    pub peer_target: Option<&'a MeshTarget>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkPayload {
    schema_version: u64,
    iss: String,
    aud: Audience,
    sub: String,
    holder: MeshTarget,
    peer: MeshTarget,
    transcript_sha256: String,
    authority_fingerprint: String,
    policy_epoch: String,
    issued_at: u64,
    expires_at: u64,
}

/// `MeshLinkPermitPayload`: one side's own short online authorization for exactly this transcript.
pub fn verify_link_permit(
    grant: &DetachedGrant,
    trust: &MembershipTrust,
    expect: &LinkPermitExpectation<'_>,
    now: u64,
) -> Result<(), MembershipRefusal> {
    use MembershipRefusal as R;
    let value = signed_payload(grant, trust)?;
    let payload: LinkPayload = serde_json::from_value(value).map_err(|_| R::Malformed)?;
    if payload.schema_version != 1 || payload.iss != LINK_PERMIT_ISSUER {
        return Err(R::Issuer);
    }
    audience(trust, &payload.aud, &payload.policy_epoch)?;
    if !valid_id(&payload.sub) || account_id(&payload.sub) != expect.account_id {
        return Err(R::Subject);
    }
    if !hex64(&payload.authority_fingerprint) {
        return Err(R::Malformed);
    }
    if payload.holder.installation_id != expect.holder_installation_id
        || payload.peer.installation_id != expect.peer_installation_id
        || payload.holder.installation_id == payload.peer.installation_id
        || payload.transcript_sha256 != sha256_hex(expect.transcript)
    {
        return Err(R::Installation);
    }
    if expect.peer_target.is_some_and(|own| *own != payload.peer) {
        return Err(R::Slot);
    }
    window(
        payload.issued_at,
        payload.expires_at,
        MESH_LINK_PERMIT_SECONDS,
        now,
    )
}
