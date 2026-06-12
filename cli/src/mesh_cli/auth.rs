//! Outbound mesh request signing — Ed25519 over a (timestamp, nonce, instance_id)
//! envelope. Server-side verifier mirror lives in src-tauri/src/api/mesh_routes.rs.

/// Sign a mesh request with Ed25519 (same as server-side peer auth).
pub fn sign_request(private_key_b64: &str, instance_id: &str) -> Result<String, String> {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;

    let key_bytes = b64.decode(private_key_b64).map_err(|e| e.to_string())?;
    if key_bytes.len() != 32 {
        return Err("Invalid key length".to_string());
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&key_bytes);
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&arr);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let payload = serde_json::json!({
        "from_instance_id": instance_id,
        "timestamp": now,
        "nonce": uuid::Uuid::new_v4().to_string(),
    });
    let payload_bytes = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
    let payload_b64 = b64.encode(&payload_bytes);

    use ed25519_dalek::Signer;
    let signature = signing_key.sign(payload_b64.as_bytes());
    let sig_b64 = b64.encode(signature.to_bytes());
    Ok(format!("{}.{}", payload_b64, sig_b64))
}
