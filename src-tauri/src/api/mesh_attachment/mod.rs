//! Mesh attachment ownership: an attachment belongs to the host of the target tab.
//! Bytes cross the signed relay and land in the target's upload dir; only a
//! target-local reference is ever delivered.

use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::api::routes::{check_auth, resolve_tab, AppState};

pub const MESH_ATTACHMENT_MAX_BYTES: u64 = 25 * 1024 * 1024;
const ENCODED_MAX_BYTES: usize = (MESH_ATTACHMENT_MAX_BYTES as usize).div_ceil(3) * 4;
/// Relay and ingest JSON bodies carry the base64 payload plus a small envelope.
pub const MESH_ATTACHMENT_RELAY_BODY_LIMIT: usize = ENCODED_MAX_BYTES + 1024 * 1024;
pub const MESH_ATTACHMENT_SUFFIX: &str = "/attachments";
pub const SOURCE_PATH_REFUSAL: &str = "source_attachment_path: a remote send references an attachment stored on this host; transfer it to the target host before sending";
const GUARD_SCAN_WINDOW: usize = 1024;
const GUARD_MAX_CANDIDATES: usize = 32;

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

/// Validate a received payload before any byte touches disk; returns the sanitized name.
pub fn decode_ingest(req: &IngestRequest) -> Result<(String, Vec<u8>), IngestRefusal> {
    if req.size > MESH_ATTACHMENT_MAX_BYTES {
        return Err(IngestRefusal::TooLarge(req.size));
    }
    if req.data.len() > ENCODED_MAX_BYTES {
        return Err(IngestRefusal::TooLarge(req.data.len() as u64 / 4 * 3));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(req.data.as_bytes())
        .map_err(|_| IngestRefusal::Malformed("data is not base64"))?;
    if bytes.is_empty() {
        return Err(IngestRefusal::Malformed("empty file"));
    }
    if bytes.len() as u64 != req.size {
        return Err(IngestRefusal::Malformed("size does not match data"));
    }
    Ok((crate::api::web_routes::sanitize_filename(&req.name), bytes))
}

/// `POST /api/tabs/{id}/attachments` — target-host ingest. Voice is transcribed
/// here, where the bytes live.
pub async fn ingest(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<IngestRequest>,
) -> Result<Json<IngestedAttachment>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    resolve_tab(&state, &id).map_err(|(s, e)| (s, e.to_string()))?;
    let (name, bytes) = decode_ingest(&req).map_err(|r| (r.status(), r.message()))?;
    let stored =
        crate::api::web_routes::store_upload(&crate::api::web_routes::uploads_dir(), &name, &bytes)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let path = stored.to_string_lossy().to_string();
    let reference = match req.kind {
        AttachmentKind::Voice => {
            let active = crate::config::load_config()
                .integrations
                .voice_transcribe
                .active;
            Some(
                crate::bridge::voice_transcribe::format_attachment_line(
                    &path,
                    "voice",
                    None,
                    &state.svc.transcribe_registry,
                    &active,
                )
                .await,
            )
        }
        AttachmentKind::File => None,
    };
    Ok(Json(IngestedAttachment {
        path,
        kind: req.kind,
        size: bytes.len() as u64,
        reference,
    }))
}

pub fn is_attachment_ingest(method: &str, path: &str) -> bool {
    let bare = path.split('?').next().unwrap_or(path);
    method.eq_ignore_ascii_case("POST")
        && bare.starts_with("/api/tabs/")
        && bare.ends_with(MESH_ATTACHMENT_SUFFIX)
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
    let bytes =
        std::fs::read(source).map_err(|e| format!("attachment is not readable on this host: {e}"))?;
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

/// Origin-side handoff shared by the Tauri command and the web mirror.
pub async fn transfer_to_peer(
    mesh: &crate::MeshStateHandle,
    ordinary_tab_id: &str,
    peer_id: &str,
    source: String,
    kind: AttachmentKind,
) -> Result<IngestedAttachment, String> {
    let (client, connected) = {
        let manager = mesh.lock();
        let connected = manager
            .paired_peers
            .get(peer_id)
            .is_some_and(|peer| peer.online);
        (manager.client_for(peer_id), connected)
    };
    let mut command = crate::api::mesh_routes::remote_tabs::route_remote_tab_action(
        ordinary_tab_id,
        peer_id,
        connected,
        "POST",
        MESH_ATTACHMENT_SUFFIX,
    )
    .map_err(str::to_string)?;
    let client = client.ok_or_else(|| "remote tab unavailable".to_string())?;
    let source_path = source.clone();
    let request = tokio::task::spawn_blocking(move || {
        build_ingest_request(std::path::Path::new(&source_path), kind)
    })
    .await
    .map_err(|e| e.to_string())??;
    command.body = serde_json::to_value(&request).map_err(|e| e.to_string())?;
    drop(request);
    let resp = client
        .send_relay(&command)
        .await
        .map_err(|e| format!("attachment transfer to the target host failed: {e}"))?;
    parse_ingest_response(&source, resp)
}

#[derive(Deserialize)]
pub struct TransferBody {
    pub ordinary_tab_id: String,
    pub peer_id: String,
    pub path: String,
    pub kind: AttachmentKind,
}

/// `POST /api/mesh/attachments/transfer` — web-mirror twin of `mesh_transfer_attachment`.
pub async fn transfer(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<TransferBody>,
) -> Result<Json<IngestedAttachment>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    transfer_to_peer(
        &state.mesh,
        &body.ordinary_tab_id,
        &body.peer_id,
        body.path,
        body.kind,
    )
    .await
    .map(Json)
    .map_err(|e| (StatusCode::BAD_GATEWAY, e))
}

fn is_remote_send(method: &str, path: &str) -> bool {
    let bare = path.split('?').next().unwrap_or(path);
    method.eq_ignore_ascii_case("POST")
        && (bare.ends_with("/send")
            || bare.ends_with("/chat-send")
            || bare.ends_with("/delayed-messages"))
}

fn outbound_texts(body: &serde_json::Value) -> Vec<&str> {
    let mut texts: Vec<&str> = body.get("text").and_then(|v| v.as_str()).into_iter().collect();
    if let Some(batch) = body.get("batch").and_then(|v| v.as_array()) {
        texts.extend(batch.iter().filter_map(|item| item.get("text")?.as_str()));
    }
    texts
}

pub fn source_attachment_roots() -> Vec<String> {
    vec![
        crate::api::web_routes::uploads_dir()
            .to_string_lossy()
            .to_string(),
        crate::attachments::bucket_dir("")
            .to_string_lossy()
            .to_string(),
    ]
}

fn normalize_for_match(value: &str) -> String {
    let slashed = value.replace('\\', "/");
    if cfg!(windows) {
        slashed.to_ascii_lowercase()
    } else {
        slashed
    }
}

fn decode_marker_escapes(text: &str) -> String {
    text.replace("%20", " ")
        .replace("%3D", "=")
        .replace("%0A", "\n")
        .replace("%09", "\t")
        .replace("%22", "\"")
        .replace("%25", "%")
}

/// Normalization keeps byte offsets aligned, so a root match indexes the original text.
fn existing_path_at(
    text: &str,
    start: usize,
    root_len: usize,
    is_file: &dyn Fn(&str) -> bool,
) -> Option<String> {
    let mut window_end = (start + root_len + GUARD_SCAN_WINDOW).min(text.len());
    while !text.is_char_boundary(window_end) {
        window_end -= 1;
    }
    let tail = &text[start..window_end];
    let mut ends: Vec<usize> = tail
        .char_indices()
        .filter(|&(i, c)| i > root_len && (c.is_whitespace() || matches!(c, '"' | '\'' | '`')))
        .map(|(i, _)| i)
        .chain(tail.match_indices("-=-").map(|(i, _)| i).filter(|&i| i > root_len))
        .collect();
    ends.sort_unstable();
    ends.dedup();
    ends.truncate(GUARD_MAX_CANDIDATES);
    ends.push(tail.len());
    ends.into_iter()
        .map(|end| &tail[..end])
        .find(|candidate| is_file(candidate))
        .map(str::to_string)
}

/// An existing file under a host attachment root means the text still points at this host.
pub fn find_source_attachment_path(
    text: &str,
    roots: &[String],
    is_file: &dyn Fn(&str) -> bool,
) -> Option<String> {
    let decoded = decode_marker_escapes(text);
    for candidate_text in [text, decoded.as_str()] {
        let haystack = normalize_for_match(candidate_text);
        for root in roots {
            let needle = normalize_for_match(root.trim_end_matches(['/', '\\']));
            if needle.is_empty() {
                continue;
            }
            let mut from = 0;
            while let Some(offset) = haystack[from..].find(&needle) {
                let start = from + offset;
                if let Some(found) = existing_path_at(candidate_text, start, needle.len(), is_file)
                {
                    return Some(found);
                }
                from = start + needle.len();
            }
        }
    }
    None
}

pub fn guard_remote_send(
    method: &str,
    path: &str,
    body: &serde_json::Value,
    roots: &[String],
    is_file: &dyn Fn(&str) -> bool,
) -> Result<(), String> {
    if !is_remote_send(method, path) {
        return Ok(());
    }
    let leaks = outbound_texts(body)
        .into_iter()
        .any(|text| find_source_attachment_path(text, roots, is_file).is_some());
    if leaks {
        return Err(SOURCE_PATH_REFUSAL.to_string());
    }
    Ok(())
}

/// Host-side backstop on every remote send leaving this machine.
pub async fn guard_remote_send_on_host(
    method: &str,
    path: &str,
    body: &serde_json::Value,
) -> Result<(), String> {
    if !is_remote_send(method, path) {
        return Ok(());
    }
    let (method, path, body) = (method.to_string(), path.to_string(), body.clone());
    tokio::task::spawn_blocking(move || {
        guard_remote_send(
            &method,
            &path,
            &body,
            &source_attachment_roots(),
            &|candidate| std::path::Path::new(candidate).is_file(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests;
