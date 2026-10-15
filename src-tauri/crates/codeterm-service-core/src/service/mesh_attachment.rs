//! Mesh attachment payloads exchanged between the origin host and the target host.

use axum::http::StatusCode;
use base64::Engine as _;
use serde::{Deserialize, Serialize};

pub const MESH_ATTACHMENT_MAX_BYTES: u64 = 25 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    Voice,
    File,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IngestRequest {
    pub kind: AttachmentKind,
    pub name: String,
    pub size: u64,
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestedAttachment {
    pub path: String,
    pub kind: AttachmentKind,
    pub size: u64,
    #[serde(default)]
    pub reference: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum IngestRefusal {
    TooLarge(u64),
    Malformed(&'static str),
}

impl IngestRefusal {
    pub fn status(&self) -> StatusCode {
        match self {
            Self::TooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Malformed(_) => StatusCode::BAD_REQUEST,
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::TooLarge(size) => format!(
                "attachment of {size} bytes exceeds the {MESH_ATTACHMENT_MAX_BYTES}-byte mesh limit"
            ),
            Self::Malformed(reason) => format!("malformed attachment: {reason}"),
        }
    }
}

/// Read a source attachment on the origin host into a bounded ingest payload.
pub fn build_ingest_request(
    source: &std::path::Path,
    kind: AttachmentKind,
) -> Result<IngestRequest, String> {
    let meta = std::fs::metadata(source)
        .map_err(|e| format!("attachment is not readable on this host: {e}"))?;
    if !meta.is_file() {
        return Err("attachment is not a regular file".into());
    }
    let too_large = |size: u64| IngestRefusal::TooLarge(size).message();
    if meta.len() > MESH_ATTACHMENT_MAX_BYTES {
        return Err(too_large(meta.len()));
    }
    let bytes = std::fs::read(source)
        .map_err(|e| format!("attachment is not readable on this host: {e}"))?;
    if bytes.is_empty() {
        return Err("attachment is empty".into());
    }
    if bytes.len() as u64 > MESH_ATTACHMENT_MAX_BYTES {
        return Err(too_large(bytes.len() as u64));
    }
    let name = source
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    Ok(IngestRequest {
        kind,
        name,
        size: bytes.len() as u64,
        data: base64::engine::general_purpose::STANDARD.encode(&bytes),
    })
}

/// A transfer only succeeds with a target-local copy that never echoes the source path.
pub fn parse_ingest_response(
    source: &str,
    resp: crate::mesh::types::RelayResponse,
) -> Result<IngestedAttachment, String> {
    if !resp.ok {
        let reason = resp
            .error
            .unwrap_or_else(|| format!("HTTP {}", resp.status));
        return Err(format!(
            "attachment transfer to the target host failed: {reason}"
        ));
    }
    let data = resp
        .data
        .ok_or("attachment transfer failed: the target host returned no reference")?;
    let ingested: IngestedAttachment = serde_json::from_value(data).map_err(|e| {
        format!("attachment transfer failed: the target host returned a malformed reference: {e}")
    })?;
    if ingested.path.trim().is_empty() {
        return Err("attachment transfer failed: the target host returned an empty path".into());
    }
    let echoes_source = ingested.path == source
        || ingested
            .reference
            .as_deref()
            .is_some_and(|reference| reference.contains(source));
    if echoes_source {
        return Err(
            "attachment transfer failed: the target host echoed the source-machine path".into(),
        );
    }
    Ok(ingested)
}
