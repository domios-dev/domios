//! Peer authentication — Ed25519 request signing and verification.
//!
//! Every paired peer has an Ed25519 keypair. Requests between peers are signed
//! with the sender's private key and verified against known public keys.

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::STANDARD;

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
    pub timestamp: u64,
    /// Nonce to prevent replay (UUID).
    pub nonce: String,
}

/// Generate Ed25519 keypair for peer identity.
pub fn generate_peer_keypair() -> (String, String) {
    let signing_key = SigningKey::generate(&mut OsRng);
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
    let parts: Vec<&str> = token.splitn(2, '.').collect();
    if parts.len() != 2 {
        return Err("Invalid signed request format".to_string());
    }

    // Decode payload to get instance_id
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
    if now.saturating_sub(req.timestamp) > 300 {
        return Err("Request expired".to_string());
    }
    if req.timestamp.saturating_sub(now) > 30 {
        return Err("Request timestamp too far in the future".to_string());
    }

    // Find public key for this instance
    let pubkey_b64 = known_keys
        .get(&req.from_instance_id)
        .ok_or_else(|| format!("Unknown peer: {}", req.from_instance_id))?;

    // Verify signature
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

    Ok(req.from_instance_id)
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

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_sign_verify_roundtrip() {
        let (priv_key, pub_key) = generate_peer_keypair();
        let token = sign_request(&priv_key, "peer-abc").unwrap();
        assert!(!token.is_empty());
        assert!(token.contains('.'));

        let mut known = std::collections::HashMap::new();
        known.insert("peer-abc".to_string(), pub_key);
        let sender = verify_signed_request(&token, &known).unwrap();
        assert_eq!(sender, "peer-abc");
    }

    #[test]
    fn test_peer_sign_wrong_key_rejected() {
        let (priv_key_a, _pub_key_a) = generate_peer_keypair();
        let (_priv_key_b, pub_key_b) = generate_peer_keypair();
        let token = sign_request(&priv_key_a, "peer-a").unwrap();

        let mut known = std::collections::HashMap::new();
        known.insert("peer-a".to_string(), pub_key_b);
        assert!(verify_signed_request(&token, &known).is_err());
    }

    #[test]
    fn test_peer_sign_unknown_peer_rejected() {
        let (priv_key, _pub_key) = generate_peer_keypair();
        let token = sign_request(&priv_key, "peer-unknown").unwrap();

        let known = std::collections::HashMap::new();
        assert!(verify_signed_request(&token, &known).is_err());
    }

    #[test]
    fn public_key_from_private_matches_keypair_pub() {
        let (priv_key, pub_key) = generate_peer_keypair();
        let derived = public_key_from_private(&priv_key).unwrap();
        assert_eq!(derived, pub_key);
    }

    #[test]
    fn public_key_from_private_rejects_garbage() {
        assert!(public_key_from_private("not-base64!").is_err());
        assert!(public_key_from_private(&B64.encode([0u8; 16])).is_err());
    }

    #[test]
    fn nonce_from_token_roundtrip() {
        let (priv_key, _) = generate_peer_keypair();
        let token = sign_request(&priv_key, "peer-abc").unwrap();
        let nonce = nonce_from_token(&token).unwrap();
        assert!(!nonce.is_empty(), "nonce should be a non-empty UUID");
        // Two different tokens have different nonces
        let token2 = sign_request(&priv_key, "peer-abc").unwrap();
        let nonce2 = nonce_from_token(&token2).unwrap();
        assert_ne!(nonce, nonce2, "each sign produces a unique nonce");
    }
}
