//! Secure pairing protocol using X25519 key exchange + AES-256-GCM.
//!
//! Flow:
//! 1. A generates ephemeral X25519 keypair, sends pubkey + nonce to B
//! 2. B generates keypair, computes shared secret, encrypts its mesh_token
//! 3. A decrypts, encrypts its own mesh_token, sends confirmation
//! 4. Both sides store each other's mesh_token → PAIRED

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::STANDARD;

/// Generate an X25519 keypair. Returns (private_key_bytes, public_key_base64).
pub fn generate_keypair() -> ([u8; 32], String) {
    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);
    let private_bytes: [u8; 32] = secret.to_bytes();
    let public_b64 = B64.encode(public.as_bytes());
    (private_bytes, public_b64)
}

/// Generate a random 12-byte nonce as base64.
pub fn generate_nonce() -> String {
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    B64.encode(nonce)
}

/// Derive AES-256 key from X25519 shared secret using SHA-256.
fn derive_aes_key(shared_secret: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(shared_secret);
    hasher.update(b"codeterm-mesh-v1");
    hasher.finalize().into()
}

/// Compute shared secret and encrypt a mesh token.
pub fn encrypt_with_shared_secret(
    our_private_key: &[u8; 32],
    their_public_key_b64: &str,
    nonce_b64: &str,
    plaintext: &str,
) -> Result<String, String> {
    let their_pubkey_bytes = B64
        .decode(their_public_key_b64)
        .map_err(|e| format!("Bad pubkey: {}", e))?;
    if their_pubkey_bytes.len() != 32 {
        return Err("Public key must be 32 bytes".to_string());
    }
    let mut pubkey_arr = [0u8; 32];
    pubkey_arr.copy_from_slice(&their_pubkey_bytes);

    let our_secret = StaticSecret::from(*our_private_key);
    let their_public = PublicKey::from(pubkey_arr);
    let shared = our_secret.diffie_hellman(&their_public);
    let aes_key = derive_aes_key(shared.as_bytes());

    let nonce_bytes = B64
        .decode(nonce_b64)
        .map_err(|e| format!("Bad nonce: {}", e))?;
    if nonce_bytes.len() != 12 {
        return Err("Nonce must be 12 bytes".to_string());
    }

    let cipher =
        Aes256Gcm::new_from_slice(&aes_key).map_err(|e| format!("Cipher init: {}", e))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| format!("Encrypt failed: {}", e))?;

    Ok(B64.encode(ciphertext))
}

/// Decrypt a mesh token using shared secret.
pub fn decrypt_with_shared_secret(
    our_private_key: &[u8; 32],
    their_public_key_b64: &str,
    nonce_b64: &str,
    ciphertext_b64: &str,
) -> Result<String, String> {
    let their_pubkey_bytes = B64
        .decode(their_public_key_b64)
        .map_err(|e| format!("Bad pubkey: {}", e))?;
    if their_pubkey_bytes.len() != 32 {
        return Err("Public key must be 32 bytes".to_string());
    }
    let mut pubkey_arr = [0u8; 32];
    pubkey_arr.copy_from_slice(&their_pubkey_bytes);

    let our_secret = StaticSecret::from(*our_private_key);
    let their_public = PublicKey::from(pubkey_arr);
    let shared = our_secret.diffie_hellman(&their_public);
    let aes_key = derive_aes_key(shared.as_bytes());

    let nonce_bytes = B64
        .decode(nonce_b64)
        .map_err(|e| format!("Bad nonce: {}", e))?;
    if nonce_bytes.len() != 12 {
        return Err("Nonce must be 12 bytes".to_string());
    }
    let ciphertext = B64
        .decode(ciphertext_b64)
        .map_err(|e| format!("Bad ciphertext: {}", e))?;

    let cipher =
        Aes256Gcm::new_from_slice(&aes_key).map_err(|e| format!("Cipher init: {}", e))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|e| format!("Decrypt failed: {}", e))?;

    String::from_utf8(plaintext).map_err(|e| format!("UTF-8 decode: {}", e))
}

// Legacy pairing functions removed — pairing now just exchanges Ed25519 public keys.

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_exchange_roundtrip() {
        let (priv_a, pub_a) = generate_keypair();
        let (priv_b, pub_b) = generate_keypair();
        let nonce = generate_nonce();

        let secret = "my-mesh-token-abc";

        // A encrypts for B
        let encrypted = encrypt_with_shared_secret(&priv_a, &pub_b, &nonce, secret).unwrap();

        // B decrypts
        let decrypted = decrypt_with_shared_secret(&priv_b, &pub_a, &nonce, &encrypted).unwrap();
        assert_eq!(decrypted, secret);
    }

    #[test]
    fn test_wrong_key_fails() {
        let (priv_a, _pub_a) = generate_keypair();
        let (_priv_b, pub_b) = generate_keypair();
        let (priv_c, _pub_c) = generate_keypair();
        let nonce = generate_nonce();

        let encrypted =
            encrypt_with_shared_secret(&priv_a, &pub_b, &nonce, "secret").unwrap();

        // C tries to decrypt (should fail)
        let result = decrypt_with_shared_secret(&priv_c, &pub_b, &nonce, &encrypted);
        assert!(result.is_err());
    }
}
