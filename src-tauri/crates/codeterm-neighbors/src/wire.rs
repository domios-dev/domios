use crate::{Error, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "domios.friend-envelope.v2";
pub const MAX_CIPHERTEXT_BYTES: usize = 8192;
pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EncryptionKeyIds {
    pub sender: String,
    pub recipient: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub schema: String,
    pub friendship_id: String,
    pub message_id: String,
    pub sender_uid: String,
    pub recipient_uid: String,
    pub kind: Kind,
    #[serde(deserialize_with = "required_nullable")]
    pub in_reply_to: Option<String>,
    pub ciphertext: String,
    pub nonce: String,
    pub enc_key_ids: EncryptionKeyIds,
    pub scope_sha256: String,
    pub verdict: String,
    pub verifier_key_id: String,
    pub issued_at: u64,
}

fn required_nullable<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Question,
    Answer,
    Refusal,
    TabRequest,
}

pub fn hex_id(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn key_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub fn uid(value: &str) -> bool {
    !value.is_empty()
        && value.encode_utf16().count() <= 128
        && value != "."
        && value != ".."
        && !value.chars().any(|c| c == '/' || c <= '\u{1f}')
}

pub fn decode_url(value: &str, min: usize, max: usize) -> Result<Vec<u8>> {
    if value.len() > max.saturating_mul(4).div_ceil(3) {
        return Err(Error::InvalidEnvelope);
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| Error::InvalidEnvelope)?;
    if bytes.len() < min || bytes.len() > max || URL_SAFE_NO_PAD.encode(&bytes) != value {
        return Err(Error::InvalidEnvelope);
    }
    Ok(bytes)
}

impl Envelope {
    pub fn validate(&self) -> Result<()> {
        if self.schema != SCHEMA
            || !hex_id(&self.friendship_id, 64)
            || !hex_id(&self.message_id, 32)
            || !uid(&self.sender_uid)
            || !uid(&self.recipient_uid)
            || self.sender_uid == self.recipient_uid
            || self.in_reply_to.as_ref().is_some_and(|id| !hex_id(id, 32))
            || !hex_id(&self.scope_sha256, 64)
            || self.verdict != "pass"
            || !key_id(&self.verifier_key_id)
            || !key_id(&self.enc_key_ids.sender)
            || !key_id(&self.enc_key_ids.recipient)
            || self.issued_at > MAX_SAFE_INTEGER
        {
            return Err(Error::InvalidEnvelope);
        }
        decode_url(&self.ciphertext, 16, MAX_CIPHERTEXT_BYTES)?;
        decode_url(&self.nonce, 12, 12)?;
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        self.validate()?;
        // The v2 schema has only ASCII object keys, strings, null and safe integers.
        fn sorted(value: serde_json::Value) -> serde_json::Value {
            match value {
                serde_json::Value::Object(map) => {
                    let entries: std::collections::BTreeMap<_, _> = map.into_iter().collect();
                    serde_json::Value::Object(
                        entries.into_iter().map(|(k, v)| (k, sorted(v))).collect(),
                    )
                }
                other => other,
            }
        }
        let value = serde_json::to_value(self).map_err(|_| Error::InvalidEnvelope)?;
        serde_json::to_vec(&sorted(value)).map_err(|_| Error::InvalidEnvelope)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SignedEnvelope {
    pub envelope: Envelope,
    pub signature: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PeerKey {
    pub verifier_key_id: String,
    pub public_key_spki_b64: String,
    pub encryption_public_key_spki_b64: String,
    pub enc_key_id: String,
    pub created_at: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PeerKeys {
    pub friendship_id: String,
    pub peer_uid: String,
    pub keys: Vec<PeerKey>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct Timestamp {
    pub seconds: u64,
    pub nanoseconds: u32,
}

impl Timestamp {
    pub fn valid(self) -> bool {
        self.seconds <= MAX_SAFE_INTEGER && self.nanoseconds < 1_000_000_000
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    pub received_at: Timestamp,
    pub id: String,
}

impl Cursor {
    pub fn validate(&self) -> Result<()> {
        if !self.received_at.valid()
            || self.id.is_empty()
            || self.id.len() > 512
            || self.id.contains('/')
        {
            return Err(Error::InvalidEnvelope);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InboxDocument {
    pub envelope: Envelope,
    pub signature: String,
    pub received_at: Timestamp,
    #[serde(rename = "expiresAt")]
    pub expires_at: Timestamp,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InboxMessage {
    pub id: String,
    pub doc: InboxDocument,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Pending,
    Delivered,
    NotDelivered,
}
