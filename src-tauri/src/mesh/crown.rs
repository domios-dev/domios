//! Crown system — distributed leadership with Ed25519 cryptographic verification.
//!
//! Crown = Ed25519 keypair. Whoever holds the private key is the legitimate leader.
//! All decisions (trust levels, peer management) are signed JWTs verifiable by any peer.
//!
//! Flow:
//! 1. First peer to accept a pairing request becomes crowned (generates keypair)
//! 2. Crown holder signs trust declarations for peers
//! 3. Transfer: new keypair generated, signed by old key, private key encrypted and sent
//! 4. Emergency: if crown holder offline >5min, trusted peers vote (quorum) for new leader

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::STANDARD;

// ── Crown keypair ───────────────────────────────────────────────────

/// Crown keypair stored on disk (only on the crown holder's machine).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrownKeyPair {
    /// Ed25519 private key (32 bytes, base64).
    pub private_key: String,
    /// Ed25519 public key (32 bytes, base64).
    pub public_key: String,
    /// Version counter — incremented on each transfer.
    pub version: u64,
}

impl CrownKeyPair {
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        Self {
            private_key: B64.encode(signing_key.to_bytes()),
            public_key: B64.encode(verifying_key.to_bytes()),
            version: 1,
        }
    }

    pub fn signing_key(&self) -> Result<SigningKey, String> {
        let bytes = B64.decode(&self.private_key).map_err(|e| e.to_string())?;
        if bytes.len() != 32 {
            return Err("Invalid private key length".to_string());
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(SigningKey::from_bytes(&arr))
    }

    pub fn verifying_key(&self) -> Result<VerifyingKey, String> {
        verifying_key_from_b64(&self.public_key)
    }
}

pub fn verifying_key_from_b64(b64: &str) -> Result<VerifyingKey, String> {
    let bytes = B64.decode(b64).map_err(|e| e.to_string())?;
    if bytes.len() != 32 {
        return Err("Invalid public key length".to_string());
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    VerifyingKey::from_bytes(&arr).map_err(|e| e.to_string())
}

// ── Crown JWT ───────────────────────────────────────────────────────

/// Claims inside a crown JWT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrownClaims {
    /// Instance ID of the crown holder.
    pub holder_id: String,
    /// Unique mesh identifier (created at first coronation).
    pub mesh_id: String,
    /// Coronation timestamp (Unix seconds).
    pub crowned_at: u64,
    /// Version counter (higher = newer, wins conflicts).
    pub version: u64,
    /// If this was an emergency election.
    #[serde(default)]
    pub emergency: bool,
    /// Peers that voted for this crown (only for emergency elections).
    #[serde(default)]
    pub voted_by: Vec<String>,
    /// JWT standard: issued at.
    pub iat: u64,
}

/// Create a signed crown token: base64(json) + "." + base64(signature).
pub fn create_crown_jwt(keypair: &CrownKeyPair, claims: &CrownClaims) -> Result<String, String> {
    let signing_key = keypair.signing_key()?;
    let payload = serde_json::to_vec(claims).map_err(|e| e.to_string())?;
    let payload_b64 = B64.encode(&payload);
    let signature = signing_key.sign(payload_b64.as_bytes());
    let sig_b64 = B64.encode(signature.to_bytes());
    Ok(format!("{}.{}", payload_b64, sig_b64))
}

/// Verify and decode a signed crown token.
pub fn verify_crown_jwt(token: &str, public_key_b64: &str) -> Result<CrownClaims, String> {
    let parts: Vec<&str> = token.splitn(2, '.').collect();
    if parts.len() != 2 {
        return Err("Invalid token format".to_string());
    }
    let vk = verifying_key_from_b64(public_key_b64)?;
    let sig_bytes = B64.decode(parts[1]).map_err(|e| format!("Bad signature: {}", e))?;
    if sig_bytes.len() != 64 {
        return Err("Invalid signature length".to_string());
    }
    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&sig_bytes);
    let signature = ed25519_dalek::Signature::from_bytes(&sig_arr);
    vk.verify(parts[0].as_bytes(), &signature)
        .map_err(|_| "Invalid signature".to_string())?;
    let payload = B64.decode(parts[0]).map_err(|e| format!("Bad payload: {}", e))?;
    serde_json::from_slice(&payload).map_err(|e| format!("Bad claims: {}", e))
}

// ── Trust declarations ──────────────────────────────────────────────

/// Trust level for a peer in the mesh.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    /// Default — all mesh peers are trusted unless King explicitly marks otherwise.
    Trusted,
    /// King-declared ban. Cannot send commands to the network.
    Untrusted,
    /// Can observe but not interact.
    Observer,
}

impl Default for TrustLevel {
    fn default() -> Self {
        Self::Trusted
    }
}

/// A signed trust declaration from the crown holder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustDeclaration {
    pub peer_id: String,
    pub trust_level: TrustLevel,
    pub declared_by: String,
    pub version: u64,
    /// The signed JWT containing these claims.
    pub signed_jwt: String,
    /// Expiry timestamp (Unix seconds). Declarations past this are invalid.
    /// Legacy declarations (None) are treated as expired and must be refreshed.
    #[serde(default)]
    pub expires_at: Option<u64>,
}

/// Claims inside a trust declaration JWT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustClaims {
    pub peer_id: String,
    pub trust_level: TrustLevel,
    pub declared_by: String,
    pub version: u64,
    pub iat: u64,
    /// Expiry timestamp (Unix seconds).
    #[serde(default)]
    pub expires_at: Option<u64>,
}

/// Default TTL for trust declarations: 5 minutes.
pub const TRUST_TTL_SECS: u64 = 300;

/// Create a signed trust declaration with TTL.
pub fn create_trust_declaration(
    keypair: &CrownKeyPair,
    peer_id: &str,
    trust_level: TrustLevel,
    holder_id: &str,
    version: u64,
) -> Result<TrustDeclaration, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let expires_at = now + TRUST_TTL_SECS;

    let claims = TrustClaims {
        peer_id: peer_id.to_string(),
        trust_level: trust_level.clone(),
        declared_by: holder_id.to_string(),
        version,
        iat: now,
        expires_at: Some(expires_at),
    };

    let signing_key = keypair.signing_key()?;
    let payload = serde_json::to_vec(&claims).map_err(|e| e.to_string())?;
    let payload_b64 = B64.encode(&payload);
    let signature = signing_key.sign(payload_b64.as_bytes());
    let sig_b64 = B64.encode(signature.to_bytes());
    let signed_token = format!("{}.{}", payload_b64, sig_b64);

    Ok(TrustDeclaration {
        peer_id: peer_id.to_string(),
        trust_level,
        declared_by: holder_id.to_string(),
        version,
        signed_jwt: signed_token,
        expires_at: Some(expires_at),
    })
}

/// Verify a trust declaration signature and TTL.
/// Returns claims if signature valid AND not expired.
pub fn verify_trust_declaration(
    declaration: &TrustDeclaration,
    public_key_b64: &str,
) -> Result<TrustClaims, String> {
    let parts: Vec<&str> = declaration.signed_jwt.splitn(2, '.').collect();
    if parts.len() != 2 {
        return Err("Invalid token format".to_string());
    }
    let vk = verifying_key_from_b64(public_key_b64)?;
    let sig_bytes = B64.decode(parts[1]).map_err(|e| format!("Bad signature: {}", e))?;
    if sig_bytes.len() != 64 {
        return Err("Invalid signature length".to_string());
    }
    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&sig_bytes);
    let signature = ed25519_dalek::Signature::from_bytes(&sig_arr);
    vk.verify(parts[0].as_bytes(), &signature)
        .map_err(|_| "Invalid signature".to_string())?;
    let payload = B64.decode(parts[0]).map_err(|e| format!("Bad payload: {}", e))?;
    let claims: TrustClaims = serde_json::from_slice(&payload).map_err(|e| format!("Bad claims: {}", e))?;

    // Check TTL — legacy declarations (no expires_at) are treated as expired
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    match claims.expires_at {
        Some(exp) if now > exp => Err(format!("Declaration expired (expired {}s ago)", now - exp)),
        None => Err("Declaration has no expiry (legacy)".to_string()),
        _ => Ok(claims),
    }
}

/// Verify trust declaration signature only (no TTL check).
/// Used internally for re-signing expired declarations.
pub fn verify_trust_signature(
    declaration: &TrustDeclaration,
    public_key_b64: &str,
) -> Result<TrustClaims, String> {
    let parts: Vec<&str> = declaration.signed_jwt.splitn(2, '.').collect();
    if parts.len() != 2 {
        return Err("Invalid token format".to_string());
    }
    let vk = verifying_key_from_b64(public_key_b64)?;
    let sig_bytes = B64.decode(parts[1]).map_err(|e| format!("Bad signature: {}", e))?;
    if sig_bytes.len() != 64 {
        return Err("Invalid signature length".to_string());
    }
    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&sig_bytes);
    let signature = ed25519_dalek::Signature::from_bytes(&sig_arr);
    vk.verify(parts[0].as_bytes(), &signature)
        .map_err(|_| "Invalid signature".to_string())?;
    let payload = B64.decode(parts[0]).map_err(|e| format!("Bad payload: {}", e))?;
    serde_json::from_slice(&payload).map_err(|e| format!("Bad claims: {}", e))
}

// ── Emergency election ──────────────────────────────────────────────

/// Vote for emergency crown election.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmergencyVote {
    pub voter_id: String,
    pub candidate_id: String,
    pub reason: String,
    pub timestamp: u64,
}

/// Check if quorum is reached for emergency election.
pub fn check_quorum(votes: &[EmergencyVote], total_trusted: usize) -> bool {
    if total_trusted == 0 {
        return false;
    }
    let unique_voters: std::collections::HashSet<&str> =
        votes.iter().map(|v| v.voter_id.as_str()).collect();
    // Majority of trusted peers must vote
    unique_voters.len() > total_trusted / 2
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
    let kp = CrownKeyPair::generate();
    (kp.private_key, kp.public_key)
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
    let payload_bytes = B64.decode(parts[0]).map_err(|e| format!("Bad payload: {}", e))?;
    let req: SignedRequest = serde_json::from_slice(&payload_bytes)
        .map_err(|e| format!("Bad request claims: {}", e))?;

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
    let pubkey_b64 = known_keys.get(&req.from_instance_id)
        .ok_or_else(|| format!("Unknown peer: {}", req.from_instance_id))?;

    // Verify signature
    let vk = verifying_key_from_b64(pubkey_b64)?;
    let sig_bytes = B64.decode(parts[1]).map_err(|e| format!("Bad signature: {}", e))?;
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

// ── Storage ─────────────────────────────────────────────────────────

/// Load crown keypair from disk.
pub fn load_crown_keypair() -> Option<CrownKeyPair> {
    let path = super::peers::mesh_dir().join("crown_keypair.json");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|d| serde_json::from_str(&d).ok())
}

/// Save crown keypair to disk. Sets restrictive permissions.
pub fn save_crown_keypair(keypair: &CrownKeyPair) -> Result<(), String> {
    let path = super::peers::mesh_dir().join("crown_keypair.json");
    let json = serde_json::to_string_pretty(keypair).map_err(|e| e.to_string())?;
    std::fs::write(&path, &json).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Delete crown keypair (after transferring crown).
pub fn delete_crown_keypair() {
    let path = super::peers::mesh_dir().join("crown_keypair.json");
    let _ = std::fs::remove_file(&path);
}

/// Load trust declarations from disk.
pub fn load_trust_declarations() -> Vec<TrustDeclaration> {
    let path = super::peers::mesh_dir().join("trust_declarations.json");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|d| serde_json::from_str(&d).ok())
        .unwrap_or_default()
}

/// Save trust declarations to disk.
pub fn save_trust_declarations(declarations: &[TrustDeclaration]) -> Result<(), String> {
    let path = super::peers::mesh_dir().join("trust_declarations.json");
    let json = serde_json::to_string_pretty(declarations).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

/// Load the mesh-wide crown public key + JWT (known to all peers).
pub fn load_crown_public_info() -> Option<CrownPublicInfo> {
    let path = super::peers::mesh_dir().join("crown_public.json");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|d| serde_json::from_str(&d).ok())
}

/// Save the mesh-wide crown public key + JWT.
pub fn save_crown_public_info(info: &CrownPublicInfo) -> Result<(), String> {
    let path = super::peers::mesh_dir().join("crown_public.json");
    let json = serde_json::to_string_pretty(info).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

/// Public crown info shared with all peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrownPublicInfo {
    /// The crown holder's public key (base64).
    pub public_key: String,
    /// Signed JWT proving who the crown holder is.
    pub crown_jwt: String,
    /// Crown version.
    pub version: u64,
    /// Crown holder instance ID.
    pub holder_id: String,
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_generation() {
        let kp = CrownKeyPair::generate();
        assert!(!kp.private_key.is_empty());
        assert!(!kp.public_key.is_empty());
        assert_eq!(kp.version, 1);
        assert!(kp.signing_key().is_ok());
        assert!(kp.verifying_key().is_ok());
    }

    #[test]
    fn test_crown_jwt_roundtrip() {
        let kp = CrownKeyPair::generate();
        let claims = CrownClaims {
            holder_id: "test-holder".to_string(),
            mesh_id: "test-mesh".to_string(),
            crowned_at: 1000,
            version: 1,
            emergency: false,
            voted_by: vec![],
            iat: 1000,
        };

        let jwt = create_crown_jwt(&kp, &claims).unwrap();
        assert!(!jwt.is_empty());

        let decoded = verify_crown_jwt(&jwt, &kp.public_key).unwrap();
        assert_eq!(decoded.holder_id, "test-holder");
        assert_eq!(decoded.mesh_id, "test-mesh");
        assert_eq!(decoded.version, 1);
    }

    #[test]
    fn test_crown_jwt_wrong_key_fails() {
        let kp1 = CrownKeyPair::generate();
        let kp2 = CrownKeyPair::generate();
        let claims = CrownClaims {
            holder_id: "holder".to_string(),
            mesh_id: "mesh".to_string(),
            crowned_at: 1000,
            version: 1,
            emergency: false,
            voted_by: vec![],
            iat: 1000,
        };

        let jwt = create_crown_jwt(&kp1, &claims).unwrap();
        // Verify with wrong key should fail
        assert!(verify_crown_jwt(&jwt, &kp2.public_key).is_err());
    }

    #[test]
    fn test_trust_declaration_roundtrip() {
        let kp = CrownKeyPair::generate();
        let decl = create_trust_declaration(
            &kp,
            "peer-123",
            TrustLevel::Trusted,
            "crown-holder",
            1,
        )
        .unwrap();

        assert_eq!(decl.peer_id, "peer-123");
        assert_eq!(decl.trust_level, TrustLevel::Trusted);

        let verified = verify_trust_declaration(&decl, &kp.public_key).unwrap();
        assert_eq!(verified.peer_id, "peer-123");
        assert_eq!(verified.trust_level, TrustLevel::Trusted);
    }

    #[test]
    fn test_trust_declaration_tampered_fails() {
        let kp = CrownKeyPair::generate();
        let mut decl = create_trust_declaration(
            &kp,
            "peer-123",
            TrustLevel::Untrusted,
            "crown-holder",
            1,
        )
        .unwrap();

        // Tamper with the trust level (but JWT is still the original)
        decl.trust_level = TrustLevel::Trusted;

        // JWT verification succeeds but claims inside JWT still say Untrusted
        let verified = verify_trust_declaration(&decl, &kp.public_key).unwrap();
        assert_eq!(verified.trust_level, TrustLevel::Untrusted); // Original, not tampered
    }

    #[test]
    fn test_quorum_majority() {
        let votes = vec![
            EmergencyVote {
                voter_id: "a".to_string(),
                candidate_id: "x".to_string(),
                reason: "offline".to_string(),
                timestamp: 0,
            },
            EmergencyVote {
                voter_id: "b".to_string(),
                candidate_id: "x".to_string(),
                reason: "offline".to_string(),
                timestamp: 0,
            },
        ];

        // 2 out of 3 = quorum
        assert!(check_quorum(&votes, 3));
        // 2 out of 5 = no quorum
        assert!(!check_quorum(&votes, 5));
        // 2 out of 2 = quorum
        assert!(check_quorum(&votes, 2));
    }

    #[test]
    fn test_quorum_dedup_voters() {
        let votes = vec![
            EmergencyVote {
                voter_id: "a".to_string(),
                candidate_id: "x".to_string(),
                reason: "offline".to_string(),
                timestamp: 0,
            },
            EmergencyVote {
                voter_id: "a".to_string(), // duplicate voter
                candidate_id: "x".to_string(),
                reason: "offline again".to_string(),
                timestamp: 1,
            },
        ];

        // Only 1 unique voter out of 3 — no quorum
        assert!(!check_quorum(&votes, 3));
    }

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

        // Verify with B's key — should fail
        let mut known = std::collections::HashMap::new();
        known.insert("peer-a".to_string(), pub_key_b);
        assert!(verify_signed_request(&token, &known).is_err());
    }

    #[test]
    fn test_peer_sign_unknown_peer_rejected() {
        let (priv_key, _pub_key) = generate_peer_keypair();
        let token = sign_request(&priv_key, "peer-unknown").unwrap();

        let known = std::collections::HashMap::new(); // empty
        assert!(verify_signed_request(&token, &known).is_err());
    }

    #[test]
    fn test_version_conflict_resolution() {
        let kp = CrownKeyPair::generate();

        let claims_v1 = CrownClaims {
            holder_id: "old-holder".to_string(),
            mesh_id: "mesh".to_string(),
            crowned_at: 1000,
            version: 1,
            emergency: false,
            voted_by: vec![],
            iat: 1000,
        };
        let claims_v2 = CrownClaims {
            holder_id: "new-holder".to_string(),
            mesh_id: "mesh".to_string(),
            crowned_at: 2000,
            version: 2,
            emergency: false,
            voted_by: vec![],
            iat: 2000,
        };

        let jwt_v1 = create_crown_jwt(&kp, &claims_v1).unwrap();
        let jwt_v2 = create_crown_jwt(&kp, &claims_v2).unwrap();

        let decoded_v1 = verify_crown_jwt(&jwt_v1, &kp.public_key).unwrap();
        let decoded_v2 = verify_crown_jwt(&jwt_v2, &kp.public_key).unwrap();

        // Higher version wins
        assert!(decoded_v2.version > decoded_v1.version);
    }
}
