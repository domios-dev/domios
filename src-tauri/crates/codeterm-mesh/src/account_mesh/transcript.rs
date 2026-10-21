use base64::Engine;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::crown;

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::STANDARD;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Link,
    Adopt,
}

impl Purpose {
    fn domain(self) -> &'static [u8] {
        match self {
            Self::Link => b"domios-account-mesh-link-v1",
            Self::Adopt => b"domios-account-mesh-adopt-v1",
        }
    }
}

/// One side of a handshake: the Mesh transport identity bound to its enrolled installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Party {
    pub instance_id: String,
    pub mesh_public_key: String,
    pub installation_id: String,
}

pub fn fresh_nonce() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    B64.encode(bytes)
}

fn absorb(hasher: &mut Sha256, field: &str) {
    hasher.update((field.len() as u64).to_be_bytes());
    hasher.update(field.as_bytes());
}

/// Binds purpose, both identities, the shared account and both fresh nonces.
pub fn transcript(
    purpose: Purpose,
    initiator: &Party,
    responder: &Party,
    account_id: &str,
    initiator_nonce: &str,
    responder_nonce: &str,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(purpose.domain());
    for party in [initiator, responder] {
        absorb(&mut hasher, &party.instance_id);
        absorb(&mut hasher, &party.mesh_public_key);
        absorb(&mut hasher, &party.installation_id);
    }
    absorb(&mut hasher, account_id);
    absorb(&mut hasher, initiator_nonce);
    absorb(&mut hasher, responder_nonce);
    hasher.finalize().into()
}

fn role_message(transcript: &[u8; 32], role: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(transcript.len() + role.len());
    message.extend_from_slice(role);
    message.extend_from_slice(transcript);
    message
}

pub const INITIATOR_ROLE: &[u8] = b"initiator";
pub const RESPONDER_ROLE: &[u8] = b"responder";
pub const COMMIT_ROLE: &[u8] = b"initiator-commit";

/// Mesh-key possession over the transcript, role-separated so one side's proof cannot be reflected.
pub fn sign_mesh(
    private_key_b64: &str,
    transcript: &[u8; 32],
    role: &[u8],
) -> Result<String, String> {
    let bytes = B64.decode(private_key_b64).map_err(|e| e.to_string())?;
    let key: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| "Invalid private key length".to_string())?;
    let signature = SigningKey::from_bytes(&key).sign(&role_message(transcript, role));
    Ok(B64.encode(signature.to_bytes()))
}

pub fn verify_mesh(
    public_key_b64: &str,
    transcript: &[u8; 32],
    role: &[u8],
    signature_b64: &str,
) -> bool {
    let Ok(key) = crown::verifying_key_from_b64(public_key_b64) else {
        return false;
    };
    let Ok(bytes) = B64.decode(signature_b64) else {
        return false;
    };
    let Ok(signature) = Signature::from_slice(&bytes) else {
        return false;
    };
    key.verify(&role_message(transcript, role), &signature)
        .is_ok()
}

/// Installation possession signs the role-separated transcript digest.
pub fn possession_digest(transcript: &[u8; 32], role: &[u8]) -> [u8; 32] {
    Sha256::digest(role_message(transcript, role)).into()
}
