use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    future::Future,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const MAX_BYTES: usize = 256 * 1024;
const MAX_REDIRECTS: usize = 3;
const TTL: Duration = Duration::from_secs(24 * 60 * 60);

pub fn valid_url(value: &str) -> bool {
    value.len() <= 4096
        && reqwest::Url::parse(value).ok().is_some_and(|url| {
            url.scheme() == "https"
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.fragment().is_none()
        })
}

fn redirect_allowed(url: &str, hops: usize) -> bool {
    hops < MAX_REDIRECTS && valid_url(url)
}

fn normalized_mime(mime: &str) -> Option<String> {
    Some(mime.split(';').next()?.trim().to_ascii_lowercase())
}

pub(crate) fn max_bytes_for_mime(mime: &str) -> Option<usize> {
    match normalized_mime(mime)?.as_str() {
        "image/svg+xml" => Some(super::MAX_PLUGIN_ICON_BYTES),
        "image/png" | "image/jpeg" | "image/webp" => Some(MAX_BYTES),
        _ => None,
    }
}

pub fn sanitize(mime: &str, bytes: &[u8]) -> Option<String> {
    let limit = max_bytes_for_mime(mime)?;
    if bytes.is_empty() || bytes.len() > limit {
        return None;
    }
    let mime = normalized_mime(mime)?;
    let valid = match mime.as_str() {
        "image/svg+xml" => return super::sanitize_plugin_icon_svg(bytes),
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(b"\xff\xd8\xff"),
        "image/webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        _ => false,
    };
    valid.then(|| format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\"><image width=\"24\" height=\"24\" href=\"data:{mime};base64,{}\"/></svg>", STANDARD.encode(bytes)))
}

#[derive(Serialize, Deserialize)]
struct Cached {
    timestamp: u64,
    mime: String,
    body: String,
}

pub(crate) enum FetchOutcome {
    Ok { mime: String, bytes: Vec<u8> },
    Unavailable,
    Rejected,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn cache_path(root: &Path, url: &str) -> PathBuf {
    root.join(format!("{:x}.json", Sha256::digest(url.as_bytes())))
}

fn cached(path: &Path, time: Option<u64>) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take((MAX_BYTES * 2 + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_BYTES * 2 {
        return None;
    }
    let entry: Cached = serde_json::from_slice(&bytes).ok()?;
    if time.is_some_and(|time| time < entry.timestamp || time - entry.timestamp >= TTL.as_secs()) {
        return None;
    }
    sanitize(&entry.mime, &STANDARD.decode(entry.body).ok()?)
}

fn persist_cached(path: &Path, timestamp: u64, mime: String, bytes: &[u8]) -> bool {
    let entry = Cached {
        timestamp,
        mime,
        body: STANDARD.encode(bytes),
    };
    let Ok(encoded) = serde_json::to_vec(&entry) else {
        return false;
    };
    codeterm_util::atomic_write::write_atomic(path, encoded).is_ok()
}

/// Revalidate a stored display icon independently of its network refresh age.
pub fn read_cached(root: &Path, url: &str) -> Option<String> {
    if !valid_url(url) {
        return None;
    }
    cached(&cache_path(root, url), None)
}

async fn fetch_https(url: String) -> FetchOutcome {
    let Ok(client) = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if redirect_allowed(attempt.url().as_str(), attempt.previous().len()) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .timeout(Duration::from_secs(5))
        .build()
    else {
        return FetchOutcome::Unavailable;
    };
    let Ok(mut response) = client.get(&url).send().await else {
        return FetchOutcome::Unavailable;
    };
    if response.status().is_server_error() {
        return FetchOutcome::Unavailable;
    }
    if response.status() != reqwest::StatusCode::OK || !valid_url(response.url().as_str()) {
        return FetchOutcome::Rejected;
    }
    let Some(mime) = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
    else {
        return FetchOutcome::Rejected;
    };
    let Some(limit) = max_bytes_for_mime(&mime) else {
        return FetchOutcome::Rejected;
    };
    if response
        .content_length()
        .is_some_and(|n| n > limit as u64)
    {
        return FetchOutcome::Rejected;
    }
    let mut bytes = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                let Some(next_len) = bytes.len().checked_add(chunk.len()) else {
                    return FetchOutcome::Rejected;
                };
                if next_len > limit {
                    return FetchOutcome::Rejected;
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(_) => return FetchOutcome::Unavailable,
        }
    }
    FetchOutcome::Ok { mime, bytes }
}

pub(crate) async fn resolve_with<F, Fut>(
    root: PathBuf,
    url: &str,
    now_secs: u64,
    fetch: F,
) -> Option<String>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = FetchOutcome>,
{
    if !valid_url(url) {
        return None;
    }
    let path = cache_path(&root, url);
    let read_path = path.clone();
    if let Some(icon) = tokio::task::spawn_blocking(move || cached(&read_path, Some(now_secs)))
        .await
        .ok()?
    {
        return Some(icon);
    }
    match fetch(url.to_owned()).await {
        FetchOutcome::Ok { mime, bytes } => {
            let icon = sanitize(&mime, &bytes)?;
            let write_path = path;
            let _ = tokio::task::spawn_blocking(move || {
                persist_cached(&write_path, now_secs, mime, &bytes)
            })
            .await;
            Some(icon)
        }
        FetchOutcome::Unavailable => tokio::task::spawn_blocking(move || cached(&path, None))
            .await
            .ok()
            .flatten(),
        FetchOutcome::Rejected => None,
    }
}

/// Fetch only in the daemon. Cache failures and all validation failures return no icon.
pub async fn resolve(root: PathBuf, url: &str) -> Option<String> {
    resolve_with(root, url, now(), fetch_https).await
}

#[cfg(test)]
mod tests;
