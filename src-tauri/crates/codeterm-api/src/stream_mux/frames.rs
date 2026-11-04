use axum::{body::Bytes, http::{HeaderMap, StatusCode}};
use base64::Engine;
use serde::Serialize;

pub(crate) use super::paths::classify_channel_path;

pub(crate) fn valid_mux_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "lowercase")]
pub(crate) enum ChannelEnd { Eof, Error { message: String }, Capacity { bytes: usize } }

fn encode(value: impl Serialize) -> Bytes {
    Bytes::from(format!("data: {}\n\n", serde_json::to_string(&value).unwrap_or_default()))
}

pub(crate) fn encode_head(channel: &str, status: StatusCode, headers: &HeaderMap) -> Bytes {
    let nominated = headers.get("connection").and_then(|value| value.to_str().ok()).unwrap_or_default().split(',').map(|name| name.trim().to_ascii_lowercase()).collect::<Vec<_>>();
    let filtered = headers.iter().filter_map(|(name, value)| {
        if matches!(name.as_str(), "connection" | "keep-alive" | "proxy-authenticate" | "proxy-authorization" | "te" | "trailer" | "transfer-encoding" | "upgrade" | "set-cookie" | "authorization") || nominated.iter().any(|key| key == name.as_str()) { return None; }
        let value = value.to_str().ok()?;
        Some((name.as_str(), value))
    }).collect::<Vec<_>>();
    encode(serde_json::json!({"channel":channel,"open":true,"status":status.as_u16(),"headers":filtered,"content_type":headers.get("content-type").and_then(|v|v.to_str().ok())}))
}

pub(crate) fn encode_chunk(channel: &str, bytes: &[u8], text: bool) -> Bytes {
    if text {
        if let Ok(chunk) = std::str::from_utf8(bytes) { return encode(serde_json::json!({"channel":channel,"chunk":chunk,"enc":"utf8"})); }
    }
    encode(serde_json::json!({"channel":channel,"chunk":base64::engine::general_purpose::STANDARD.encode(bytes),"enc":"base64"}))
}

pub(crate) fn encode_end(channel: &str, end: &ChannelEnd) -> Bytes {
    encode(serde_json::json!({"channel":channel,"end":end}))
}
