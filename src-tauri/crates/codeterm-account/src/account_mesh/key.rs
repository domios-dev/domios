use super::wire::{InstallationProof, ProofEndpoint, PROOF_SCHEMA_VERSION};
use crate::Secret;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand::RngCore;
use serde::Serialize;
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

const ED25519_SPKI_PREFIX: [u8; 12] = [
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];
const PROOF_DOMAIN: &str = "domios.online-installation-proof.v1";
const POSSESSION_DOMAIN: &[u8] = b"domios.mesh-possession.v1\0";

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn jcs_sha256_hex<T: Serialize>(value: &T) -> Option<String> {
    let value = serde_json::to_value(value).ok()?;
    let text = codeterm_entitlements::jcs::encode(&value).ok()?;
    Some(sha256_hex(text.as_bytes()))
}

/// Server-bound context of one proof; project, mode and epoch come from the pinned trust.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProofContext {
    pub project: String,
    pub mode: &'static str,
    pub uid: String,
    pub installation_id: String,
    pub policy_epoch: String,
    pub method: &'static str,
    pub endpoint: ProofEndpoint,
    pub body_sha256: String,
}

/// This installation's private Ed25519 possession key; only its seed is ever persisted.
pub struct InstallationKey {
    signing: SigningKey,
}

impl InstallationKey {
    pub fn generate() -> Self {
        let mut seed = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut seed);
        let signing = SigningKey::from_bytes(&seed);
        seed.zeroize();
        Self { signing }
    }

    pub fn from_secret(secret: &Secret) -> Option<Self> {
        let mut bytes = URL_SAFE_NO_PAD.decode(secret.expose()).ok()?;
        let seed: Option<[u8; 32]> = bytes.as_slice().try_into().ok();
        bytes.zeroize();
        let mut seed = seed?;
        let signing = SigningKey::from_bytes(&seed);
        seed.zeroize();
        Some(Self { signing })
    }

    pub fn to_secret(&self) -> Secret {
        let mut seed = self.signing.to_bytes();
        let secret = Secret::new(URL_SAFE_NO_PAD.encode(seed));
        seed.zeroize();
        secret
    }

    pub fn spki_der(&self) -> Vec<u8> {
        spki_der(&self.signing.verifying_key())
    }

    pub fn spki_base64(&self) -> String {
        STANDARD.encode(self.spki_der())
    }

    /// Server key id: lowercase SHA-256 hex of the DER SPKI bytes.
    pub fn key_id(&self) -> String {
        sha256_hex(&self.spki_der())
    }

    pub fn prove(
        &self,
        context: &ProofContext,
        generation: u64,
        timestamp: u64,
        nonce: String,
    ) -> Option<InstallationProof> {
        let key_id = self.key_id();
        let mut signed = serde_json::to_value(context).ok()?;
        let object = signed.as_object_mut()?;
        object.insert("domain".into(), PROOF_DOMAIN.into());
        object.insert("schema_version".into(), PROOF_SCHEMA_VERSION.into());
        object.insert("key_id".into(), key_id.clone().into());
        object.insert("generation".into(), generation.into());
        object.insert("timestamp".into(), timestamp.into());
        object.insert("nonce".into(), nonce.clone().into());
        let bytes = codeterm_entitlements::jcs::encode(&signed).ok()?;
        let signature = self.signing.sign(bytes.as_bytes()).to_bytes();
        Some(InstallationProof {
            schema_version: PROOF_SCHEMA_VERSION,
            key_id,
            generation,
            timestamp,
            nonce,
            signature: URL_SAFE_NO_PAD.encode(signature),
        })
    }

    /// Peer-facing possession: `<spki_base64url>.<signature_base64url>` over the handshake transcript.
    pub fn sign_possession(&self, transcript: &[u8; 32]) -> String {
        let signature = self.signing.sign(&possession_bytes(transcript)).to_bytes();
        format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(self.spki_der()),
            URL_SAFE_NO_PAD.encode(signature)
        )
    }
}

fn spki_der(key: &VerifyingKey) -> Vec<u8> {
    let mut der = Vec::with_capacity(44);
    der.extend_from_slice(&ED25519_SPKI_PREFIX);
    der.extend_from_slice(key.as_bytes());
    der
}

fn possession_bytes(transcript: &[u8; 32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(POSSESSION_DOMAIN.len() + transcript.len());
    bytes.extend_from_slice(POSSESSION_DOMAIN);
    bytes.extend_from_slice(transcript);
    bytes
}

/// Accepts a possession only from the key whose SPKI hashes to the membership's enrolled key id.
pub fn verify_possession(key_id: &str, transcript: &[u8; 32], possession: &str) -> bool {
    let Some((spki, signature)) = possession.split_once('.') else {
        return false;
    };
    let (Ok(spki), Ok(signature)) = (
        URL_SAFE_NO_PAD.decode(spki),
        URL_SAFE_NO_PAD.decode(signature),
    ) else {
        return false;
    };
    if spki.len() != 44 || spki[..12] != ED25519_SPKI_PREFIX || sha256_hex(&spki) != key_id {
        return false;
    }
    let (Ok(point), Ok(signature)) = (
        <[u8; 32]>::try_from(&spki[12..]),
        <[u8; 64]>::try_from(signature.as_slice()),
    ) else {
        return false;
    };
    let Ok(key) = VerifyingKey::from_bytes(&point) else {
        return false;
    };
    key.verify_strict(
        &possession_bytes(transcript),
        &Signature::from_bytes(&signature),
    )
    .is_ok()
}
