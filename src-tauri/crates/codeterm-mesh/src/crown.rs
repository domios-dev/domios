//! Peer authentication — Ed25519 request signing and verification.
//!
//! Every paired peer has an Ed25519 keypair. Requests between peers are signed
//! with the sender's private key and verified against known public keys.

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::STANDARD;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthenticatedOrigin {
    Owner,
    Agent,
    Mesh,
    Service,
}

impl AuthenticatedOrigin {
    pub fn owner_trusted(self) -> bool {
        matches!(self, Self::Owner | Self::Mesh)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticatedAgent {
    pub instance_id: String,
    pub tab_id: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub can_spawn: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppearanceStoreScope {
    destination: String,
    method: String,
    path: String,
    body_sha256: [u8; 32],
}

impl AppearanceStoreScope {
    pub fn new(
        destination: &str,
        method: &str,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            destination: destination.into(),
            method: method.into(),
            path: path.into(),
            body_sha256: Sha256::digest(serde_json::to_vec(body)?).into(),
        })
    }
}

pub const SIGNED_REQUEST_MAX_AGE_SECS: u64 = 300;
pub const SIGNED_REQUEST_CLOCK_SKEW_SECS: u64 = 30;

pub fn verifying_key_from_b64(b64: &str) -> Result<VerifyingKey, String> {
    let bytes = B64.decode(b64).map_err(|e| e.to_string())?;
    if bytes.len() != 32 {
        return Err("Invalid public key length".to_string());
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    VerifyingKey::from_bytes(&arr).map_err(|e| e.to_string())
}

// ── Peer request signing (JWT-like) ─────────────────────────────────

/// Signed mesh request — replaces Bearer mesh_token auth.
/// Format: base64(json_payload).base64(ed25519_signature)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedRequest {
    pub from_instance_id: String,
    pub origin: AuthenticatedOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AuthenticatedAgent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance_store: Option<AppearanceStoreScope>,
    pub timestamp: u64,
    /// Nonce to prevent replay (UUID).
    pub nonce: String,
}

/// Generate Ed25519 keypair for peer identity.
pub fn generate_peer_keypair() -> (String, String) {
    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
    let signing_key = SigningKey::from_bytes(&seed);
    let verifying_key = signing_key.verifying_key();
    (
        B64.encode(signing_key.to_bytes()),
        B64.encode(verifying_key.to_bytes()),
    )
}

/// Derive the Ed25519 public key from a base64-encoded private key.
///
/// Used as a fallback in auth: an identity persisted before MeshIdentity
/// stored `public_key` may load with private_key=Some / public_key=None.
/// Without this, the CLI's mesh-signed token from outside a PTY would 401.
pub fn public_key_from_private(private_key_b64: &str) -> Result<String, String> {
    let bytes = B64.decode(private_key_b64).map_err(|e| e.to_string())?;
    if bytes.len() != 32 {
        return Err("Invalid private key length".to_string());
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    let signing_key = SigningKey::from_bytes(&arr);
    Ok(B64.encode(signing_key.verifying_key().to_bytes()))
}

/// Sign a mesh request with this peer's private key.
/// Returns "Authorization: MeshJWT <signed_token>" value.
pub fn sign_request(private_key_b64: &str, instance_id: &str) -> Result<String, String> {
    sign_request_with_origin(private_key_b64, instance_id, AuthenticatedOrigin::Mesh)
}

pub fn sign_request_with_origin(
    private_key_b64: &str,
    instance_id: &str,
    origin: AuthenticatedOrigin,
) -> Result<String, String> {
    sign_request_with_agent(private_key_b64, instance_id, origin, None)
}

pub fn sign_request_with_agent(
    private_key_b64: &str,
    instance_id: &str,
    origin: AuthenticatedOrigin,
    agent: Option<&AuthenticatedAgent>,
) -> Result<String, String> {
    sign_claims(private_key_b64, instance_id, origin, agent, None)
}

pub fn sign_appearance_store(
    private_key_b64: &str,
    agent: &AuthenticatedAgent,
    scope: AppearanceStoreScope,
) -> Result<String, String> {
    sign_claims(
        private_key_b64,
        &agent.instance_id,
        AuthenticatedOrigin::Agent,
        Some(agent),
        Some(scope),
    )
}

fn sign_claims(
    private_key_b64: &str,
    instance_id: &str,
    origin: AuthenticatedOrigin,
    agent: Option<&AuthenticatedAgent>,
    appearance_store: Option<AppearanceStoreScope>,
) -> Result<String, String> {
    let key_bytes = B64.decode(private_key_b64).map_err(|e| e.to_string())?;
    if key_bytes.len() != 32 {
        return Err("Invalid private key length".to_string());
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&key_bytes);
    let signing_key = SigningKey::from_bytes(&arr);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let req = SignedRequest {
        from_instance_id: instance_id.to_string(),
        origin,
        agent: agent.cloned(),
        appearance_store,
        timestamp: now,
        nonce: uuid::Uuid::new_v4().to_string(),
    };

    let payload = serde_json::to_vec(&req).map_err(|e| e.to_string())?;
    let payload_b64 = B64.encode(&payload);
    let signature = signing_key.sign(payload_b64.as_bytes());
    let sig_b64 = B64.encode(signature.to_bytes());
    Ok(format!("{}.{}", payload_b64, sig_b64))
}

/// Verify a signed mesh request. Returns the sender's instance_id if valid.
/// `known_keys`: map of instance_id → public_key_b64.
pub fn verify_signed_request(
    token: &str,
    known_keys: &std::collections::HashMap<String, String>,
) -> Result<String, String> {
    let claims = verify_request_claims(token, known_keys)?;
    if claims.appearance_store.is_some() {
        return Err("Scoped appearance credential requires relay ingress".into());
    }
    Ok(claims.from_instance_id)
}

pub fn verify_request_claims(
    token: &str,
    known_keys: &std::collections::HashMap<String, String>,
) -> Result<SignedRequest, String> {
    let parts: Vec<&str> = token.splitn(2, '.').collect();
    if parts.len() != 2 {
        return Err("Invalid signed request format".to_string());
    }

    let payload_bytes = B64
        .decode(parts[0])
        .map_err(|e| format!("Bad payload: {}", e))?;
    let req: SignedRequest =
        serde_json::from_slice(&payload_bytes).map_err(|e| format!("Bad request claims: {}", e))?;

    // Check timestamp (5 minute window, reject future-dated)
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if now.saturating_sub(req.timestamp) > SIGNED_REQUEST_MAX_AGE_SECS {
        return Err("Request expired".to_string());
    }
    if req.timestamp.saturating_sub(now) > SIGNED_REQUEST_CLOCK_SKEW_SECS {
        return Err("Request timestamp too far in the future".to_string());
    }

    let pubkey_b64 = known_keys
        .get(&req.from_instance_id)
        .ok_or_else(|| format!("Unknown peer: {}", req.from_instance_id))?;

    let vk = verifying_key_from_b64(pubkey_b64)?;
    let sig_bytes = B64
        .decode(parts[1])
        .map_err(|e| format!("Bad signature: {}", e))?;
    if sig_bytes.len() != 64 {
        return Err("Invalid signature length".to_string());
    }
    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&sig_bytes);
    let signature = ed25519_dalek::Signature::from_bytes(&sig_arr);
    vk.verify(parts[0].as_bytes(), &signature)
        .map_err(|_| "Invalid signature".to_string())?;

    Ok(req)
}

/// Extract nonce from a signed request token without verifying the signature.
/// Used by replay-prevention code after `verify_signed_request` already passed.
pub fn nonce_from_token(token: &str) -> Result<String, String> {
    let parts: Vec<&str> = token.splitn(2, '.').collect();
    if parts.len() != 2 {
        return Err("Invalid token format".to_string());
    }
    let payload_bytes = B64
        .decode(parts[0])
        .map_err(|e| format!("Bad payload: {}", e))?;
    let req: SignedRequest =
        serde_json::from_slice(&payload_bytes).map_err(|e| format!("Bad claims: {}", e))?;
    Ok(req.nonce)
}

#[cfg(test)]
mod tests;
