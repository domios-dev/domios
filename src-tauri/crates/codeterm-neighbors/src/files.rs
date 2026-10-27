use crate::{crypto::sha256_hex, wire::hex_id, Error, Result};
use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

pub const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
pub const FILE_OVERHEAD: usize = 32;
pub const MAX_ATTACHMENTS: usize = 4;
const BODY_SCHEMA: &str = "domios.neighbor-message.v1";

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub v: u8,
    pub file_id: String,
    pub object_id: String,
    key_b64u: String,
    pub plaintext_size: usize,
    pub ciphertext_size: usize,
    pub plaintext_sha256: String,
    pub ciphertext_sha256: String,
    pub name: String,
    pub mime: String,
}

impl Drop for Manifest {
    fn drop(&mut self) {
        self.key_b64u.zeroize();
    }
}

pub struct Binding<'a> {
    pub friendship_id: &'a str,
    pub sender_uid: &'a str,
    pub recipient_uid: &'a str,
    pub message_id: &'a str,
}

impl Binding<'_> {
    fn aad(&self, file_id: &str, plaintext_size: usize) -> Result<Vec<u8>> {
        if !hex_id(self.friendship_id, 64)
            || !hex_id(self.message_id, 32)
            || !hex_id(file_id, 32)
            || !crate::wire::uid(self.sender_uid)
            || !crate::wire::uid(self.recipient_uid)
            || self.sender_uid == self.recipient_uid
        {
            return Err(Error::InvalidEnvelope);
        }
        let fields = serde_json::json!({
            "file_id":file_id,"friendship_id":self.friendship_id,
            "message_id":self.message_id,"plaintext_size":plaintext_size,
            "recipient_uid":self.recipient_uid,"schema":"domios.neighbor-file.v1",
            "sender_uid":self.sender_uid,
        });
        let sorted: std::collections::BTreeMap<_, _> = fields.as_object()
            .ok_or(Error::InvalidEnvelope)?.iter().collect();
        serde_json::to_vec(&sorted).map_err(|_| Error::InvalidEnvelope)
    }
}

impl Manifest {
    pub fn validate(&self, binding: &Binding<'_>) -> Result<()> {
        binding.aad(&self.file_id, self.plaintext_size)?;
        if self.v != 1
            || self.object_id != object_id(binding.sender_uid, &self.file_id)
            || self.plaintext_size > MAX_FILE_BYTES - FILE_OVERHEAD
            || self.ciphertext_size != self.plaintext_size + FILE_OVERHEAD
            || !hex_id(&self.plaintext_sha256, 64)
            || !hex_id(&self.ciphertext_sha256, 64)
            || self.name.is_empty()
            || self.name.len() > 255
            || self.mime.len() > 255
        {
            return Err(Error::InvalidEnvelope);
        }
        crate::wire::decode_url(&self.key_b64u, 32, 32)?;
        Ok(())
    }

    pub fn open(&self, binding: &Binding<'_>, object: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        self.validate(binding)?;
        if object.len() != self.ciphertext_size
            || sha256_hex(object) != self.ciphertext_sha256
            || &object[..4] != b"DNF1"
        {
            return Err(Error::Decryption);
        }
        let key = Zeroizing::new(crate::wire::decode_url(&self.key_b64u, 32, 32)?);
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| Error::InvalidKey)?;
        let aad = binding.aad(&self.file_id, self.plaintext_size)?;
        let plaintext = Zeroizing::new(cipher.decrypt(
            Nonce::from_slice(&object[4..16]),
            Payload { msg: &object[16..], aad: &aad },
        ).map_err(|_| Error::Decryption)?);
        if plaintext.len() != self.plaintext_size || sha256_hex(&plaintext) != self.plaintext_sha256 {
            return Err(Error::Decryption);
        }
        Ok(plaintext)
    }
}

pub fn object_id(sender_uid: &str, file_id: &str) -> String {
    sha256_hex(format!("neighbor-file:v1:{sender_uid}:{file_id}").as_bytes())
}

pub fn seal(
    binding: &Binding<'_>, file_id: &str, name: &str, mime: &str, plaintext: &[u8],
    rng: &mut (impl RngCore + CryptoRng),
) -> Result<(Manifest, Vec<u8>)> {
    if plaintext.len() > MAX_FILE_BYTES - FILE_OVERHEAD {
        return Err(Error::MessageTooLarge { limit: (MAX_FILE_BYTES - FILE_OVERHEAD) as u64 });
    }
    let aad = binding.aad(file_id, plaintext.len())?;
    let mut key = Zeroizing::new([0u8; 32]);
    let mut nonce = [0u8; 12];
    rng.fill_bytes(key.as_mut());
    rng.fill_bytes(&mut nonce);
    let cipher = Aes256Gcm::new_from_slice(key.as_ref()).map_err(|_| Error::InvalidKey)?;
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), Payload { msg: plaintext, aad: &aad })
        .map_err(|_| Error::Decryption)?;
    let mut object = Vec::with_capacity(plaintext.len() + FILE_OVERHEAD);
    object.extend_from_slice(b"DNF1");
    object.extend_from_slice(&nonce);
    object.extend_from_slice(&ciphertext);
    let manifest = Manifest {
        v: 1, file_id: file_id.into(), object_id: object_id(binding.sender_uid, file_id),
        key_b64u: URL_SAFE_NO_PAD.encode(key.as_ref()), plaintext_size: plaintext.len(),
        ciphertext_size: object.len(), plaintext_sha256: sha256_hex(plaintext),
        ciphertext_sha256: sha256_hex(&object), name: safe_name(name), mime: mime.into(),
    };
    manifest.validate(binding)?;
    Ok((manifest, object))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentBody {
    schema: String,
    pub text: String,
    pub attachments: Vec<Manifest>,
}

impl AttachmentBody {
    pub fn new(text: String, attachments: Vec<Manifest>) -> Self {
        Self { schema: BODY_SCHEMA.into(), text, attachments }
    }

    pub fn encode(&self, binding: &Binding<'_>) -> Result<Zeroizing<String>> {
        self.validate(binding)?;
        let body = Zeroizing::new(serde_json::to_string(self).map_err(|_| Error::InvalidEnvelope)?);
        crate::wire::validate_message_size(&body)?;
        Ok(body)
    }

    pub fn parse(body: &str, binding: &Binding<'_>) -> Result<Option<Self>> {
        crate::wire::validate_message_size(body)?;
        let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else { return Ok(None) };
        if value.get("attachments").is_none()
            && !value.get("schema").and_then(serde_json::Value::as_str)
                .is_some_and(|schema| schema.starts_with("domios.neighbor-message."))
        {
            return Ok(None);
        }
        let typed: Self = serde_json::from_value(value).map_err(|_| Error::InvalidEnvelope)?;
        typed.validate(binding)?;
        Ok(Some(typed))
    }

    fn validate(&self, binding: &Binding<'_>) -> Result<()> {
        if self.schema != BODY_SCHEMA || self.attachments.is_empty() || self.attachments.len() > MAX_ATTACHMENTS {
            return Err(Error::InvalidEnvelope);
        }
        let mut ids = std::collections::HashSet::new();
        for file in &self.attachments {
            file.validate(binding)?;
            if !ids.insert(&file.file_id) { return Err(Error::InvalidEnvelope); }
        }
        Ok(())
    }

    pub fn agent_text(&self) -> String {
        let mut text = self.text.clone();
        for file in &self.attachments {
            text.push_str(&format!("\n[Neighbor attachment: {} ({} bytes), handle {}]", safe_name(&file.name), file.plaintext_size, file.file_id));
        }
        text
    }
}

pub fn safe_name(name: &str) -> String {
    let mut name: String = name.chars().map(|c| {
        if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }
    }).collect();
    name = name.trim_matches([' ', '.']).to_owned();
    while name.len() > 240 { name.pop(); }
    name = name.trim_end_matches([' ', '.']).to_owned();
    if name.is_empty() { return "attachment".into(); }
    let stem = name.split('.').next().unwrap_or_default().trim_end().to_ascii_uppercase();
    let numbered = stem.strip_prefix("COM").or_else(|| stem.strip_prefix("LPT"))
        .is_some_and(|suffix| matches!(suffix, "1"|"2"|"3"|"4"|"5"|"6"|"7"|"8"|"9"|"¹"|"²"|"³"));
    if matches!(stem.as_str(), "CON"|"PRN"|"AUX"|"NUL"|"CLOCK$"|"CONIN$"|"CONOUT$") || numbered {
        name.insert(0, '_');
    }
    name
}

#[cfg(test)]
mod tests;
