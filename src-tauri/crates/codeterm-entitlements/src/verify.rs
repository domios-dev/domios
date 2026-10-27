use crate::jcs;
use crate::payload::{GrantPayload, PayloadDefect, MAX_PAYLOAD_BYTES};
use crate::policy::Policy;
use crate::text::valid_installation_id;
use crate::verdict::{TimeState, Verdict};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::Signature;
use serde::Deserialize;
use std::cmp::Ordering;

pub const ED25519: &str = "Ed25519";
/// Bounded tolerance for a desktop clock behind the issuer.
pub const MAX_CLOCK_SKEW_SECS: u64 = 300;
const SIGNATURE_BYTES: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetachedGrant {
    pub payload: String,
    pub signature: String,
    pub key_id: String,
    pub alg: String,
}

/// The authenticated session this desktop runs under; a grant must name exactly this pair.
#[derive(Clone, Copy, Debug)]
pub struct Binding<'a> {
    pub uid: &'a str,
    pub installation_id: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Algorithm,
    UnknownKey,
    SignatureEncoding,
    SignatureLength,
    PayloadEncoding,
    PayloadSize,
    Signature,
    Malformed,
    NonCanonical,
    Schema,
    Payload(PayloadDefect),
    Audience,
    PolicyVersion,
    PolicyEpoch,
    Enforcement,
    Subject,
    Installation,
    Revision,
    RevisionConflict,
    IssuedInFuture,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Facet {
    #[default]
    NotEvaluated,
    Passed,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevisionOrder {
    First,
    Higher,
    Equal,
    Lower,
}

/// Which pipeline stages ran and how they ended; a refusal leaves later stages `NotEvaluated`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Facets {
    pub authenticated_issued_at: Option<u64>,
    pub trusted_key: Facet,
    pub mathematical_signature: Facet,
    pub canonical_payload: Facet,
    pub context_matches: Facet,
    pub time_state: Option<TimeState>,
    pub revision: Option<RevisionOrder>,
}

impl Facets {
    pub fn revision_newer(&self) -> Facet {
        match self.revision {
            None => Facet::NotEvaluated,
            Some(RevisionOrder::First | RevisionOrder::Higher) => Facet::Passed,
            Some(RevisionOrder::Equal | RevisionOrder::Lower) => Facet::Failed,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejected {
    pub refusal: Refusal,
    pub facets: Facets,
}

/// A grant that passed every check; only the verifier constructs one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedGrant {
    pub(crate) payload: GrantPayload,
    pub(crate) bytes: Vec<u8>,
    pub(crate) key_id: String,
}

impl VerifiedGrant {
    pub fn payload(&self) -> &GrantPayload {
        &self.payload
    }

    pub fn canonical_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn next_transition(&self, now: u64) -> Option<u64> {
        crate::verdict::next_transition(&self.payload, now)
    }

    /// The cached verdict moves only with time; a missing fresh grant never changes it.
    pub fn verdict(&self, now: u64) -> Verdict {
        Verdict::of(&self.payload, now)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accepted {
    pub grant: VerifiedGrant,
    pub verdict: Verdict,
    pub facets: Facets,
}

pub fn verify(
    grant: &DetachedGrant,
    policy: &Policy,
    binding: &Binding<'_>,
    cached: Option<&VerifiedGrant>,
    now: u64,
) -> Result<Accepted, Rejected> {
    let mut facets = Facets::default();
    let refuse = |refusal: Refusal, facets: Facets| Err(Rejected { refusal, facets });

    let bytes = match signed_bytes(grant, policy, MAX_PAYLOAD_BYTES, &mut facets) {
        Ok(bytes) => bytes,
        Err(refusal) => return refuse(refusal, facets),
    };

    let payload = match parse_canonical(&bytes) {
        Ok(payload) => payload,
        Err(refusal) => {
            facets.canonical_payload = Facet::Failed;
            return refuse(refusal, facets);
        }
    };
    facets.canonical_payload = Facet::Passed;
    facets.authenticated_issued_at = Some(payload.issued_at);

    if let Err(refusal) = check_context(&payload, policy, binding) {
        facets.context_matches = Facet::Failed;
        return refuse(refusal, facets);
    }
    if payload.issued_at > now.saturating_add(MAX_CLOCK_SKEW_SECS) {
        facets.context_matches = Facet::Failed;
        return refuse(Refusal::IssuedInFuture, facets);
    }
    facets.context_matches = Facet::Passed;

    let verdict = Verdict::of(&payload, now);
    facets.time_state = Some(verdict.state);

    let candidate = VerifiedGrant {
        payload,
        bytes,
        key_id: grant.key_id.clone(),
    };
    let order = revision_order(&candidate, cached);
    facets.revision = Some(order);
    match order {
        RevisionOrder::Lower => refuse(Refusal::Revision, facets),
        RevisionOrder::Equal if cached.is_some_and(|c| c.bytes != candidate.bytes) => {
            refuse(Refusal::RevisionConflict, facets)
        }
        _ => Ok(Accepted {
            grant: candidate,
            verdict,
            facets,
        }),
    }
}

/// Trusted key, strict Ed25519 signature and canonical base64; the one place a signature is checked.
pub fn authenticate(
    grant: &DetachedGrant,
    policy: &Policy,
    max_bytes: usize,
) -> Result<Vec<u8>, Refusal> {
    signed_bytes(grant, policy, max_bytes, &mut Facets::default())
}

fn signed_bytes(
    grant: &DetachedGrant,
    policy: &Policy,
    max_bytes: usize,
    facets: &mut Facets,
) -> Result<Vec<u8>, Refusal> {
    if grant.alg != ED25519 {
        facets.trusted_key = Facet::Failed;
        return Err(Refusal::Algorithm);
    }
    let Some(key) = policy.key(&grant.key_id) else {
        facets.trusted_key = Facet::Failed;
        return Err(Refusal::UnknownKey);
    };
    facets.trusted_key = Facet::Passed;
    let signature = URL_SAFE_NO_PAD
        .decode(&grant.signature)
        .map_err(|_| Refusal::SignatureEncoding)?;
    let signature = <[u8; SIGNATURE_BYTES]>::try_from(signature.as_slice())
        .map_err(|_| Refusal::SignatureLength)?;
    let bytes = URL_SAFE_NO_PAD
        .decode(&grant.payload)
        .map_err(|_| Refusal::PayloadEncoding)?;
    if URL_SAFE_NO_PAD.encode(&bytes) != grant.payload {
        return Err(Refusal::PayloadEncoding);
    }
    if bytes.len() > max_bytes {
        return Err(Refusal::PayloadSize);
    }
    if key
        .verify_strict(&bytes, &Signature::from_bytes(&signature))
        .is_err()
    {
        facets.mathematical_signature = Facet::Failed;
        return Err(Refusal::Signature);
    }
    facets.mathematical_signature = Facet::Passed;
    Ok(bytes)
}

fn parse_canonical(bytes: &[u8]) -> Result<GrantPayload, Refusal> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| Refusal::Malformed)?;
    let encoded = jcs::encode(&value).map_err(|_| Refusal::NonCanonical)?;
    if encoded.as_bytes() != bytes {
        return Err(Refusal::NonCanonical);
    }
    let payload: GrantPayload = serde_json::from_value(value).map_err(|_| Refusal::Schema)?;
    payload.check_invariants().map_err(Refusal::Payload)?;
    Ok(payload)
}

fn check_context(
    payload: &GrantPayload,
    policy: &Policy,
    binding: &Binding<'_>,
) -> Result<(), Refusal> {
    let spec = policy.spec();
    if payload.aud.project != spec.project || payload.aud.env != spec.mode {
        return Err(Refusal::Audience);
    }
    if !policy.accepts_version(payload.policy_version) {
        return Err(Refusal::PolicyVersion);
    }
    if payload.policy_epoch != spec.policy_epoch {
        return Err(Refusal::PolicyEpoch);
    }
    if payload.enforcement != spec.enforcement {
        return Err(Refusal::Enforcement);
    }
    if payload.sub != binding.uid {
        return Err(Refusal::Subject);
    }
    if !valid_installation_id(binding.installation_id) || payload.inst != binding.installation_id {
        return Err(Refusal::Installation);
    }
    Ok(())
}

fn revision_order(candidate: &VerifiedGrant, cached: Option<&VerifiedGrant>) -> RevisionOrder {
    let Some(cached) = cached.filter(|cached| same_context(&cached.payload, &candidate.payload))
    else {
        return RevisionOrder::First;
    };
    match candidate.payload.revision.cmp(&cached.payload.revision) {
        Ordering::Greater => RevisionOrder::Higher,
        Ordering::Equal => RevisionOrder::Equal,
        Ordering::Less => RevisionOrder::Lower,
    }
}

fn same_context(a: &GrantPayload, b: &GrantPayload) -> bool {
    a.sub == b.sub
        && a.inst == b.inst
        && a.aud == b.aud
        && a.policy_epoch == b.policy_epoch
        && a.policy_version == b.policy_version
}
