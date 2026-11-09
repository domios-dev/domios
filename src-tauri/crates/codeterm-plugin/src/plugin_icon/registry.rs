use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const MAX_BYTES: usize = 256 * 1024;
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

pub fn sanitize(mime: &str, bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return None;
    }
    let mime = mime.split(';').next()?.trim().to_ascii_lowercase();
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
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn cached(path: &std::path::Path, time: Option<u64>) -> Option<String> {
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

/// Revalidate a stored display icon independently of its network refresh age.
pub fn read_cached(root: &std::path::Path, url: &str) -> Option<String> {
    if !valid_url(url) {
        return None;
    }
    cached(
        &root.join(format!("{:x}.json", Sha256::digest(url.as_bytes()))),
        None,
    )
}

/// Fetch only in the daemon. Cache failures and all validation failures return no icon.
pub async fn resolve(root: PathBuf, url: &str) -> Option<String> {
    if !valid_url(url) {
        return None;
    }
    let path = root.join(format!("{:x}.json", Sha256::digest(url.as_bytes())));
    let read_path = path.clone();
    if let Some(icon) = tokio::task::spawn_blocking(move || cached(&read_path, Some(now())))
        .await
        .ok()?
    {
        return Some(icon);
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()
        .ok()?;
    let mut response = client.get(url).send().await.ok()?;
    if response.status() != reqwest::StatusCode::OK
        || response
            .content_length()
            .is_some_and(|n| n > MAX_BYTES as u64)
    {
        return None;
    }
    let mime = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)?
        .to_str()
        .ok()?
        .to_owned();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len().checked_add(chunk.len())? > MAX_BYTES {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    let icon = sanitize(&mime, &bytes)?;
    let entry = Cached {
        timestamp: now(),
        mime,
        body: STANDARD.encode(bytes),
    };
    tokio::task::spawn_blocking(move || {
        let _ = (|| -> Option<()> {
            std::fs::create_dir_all(root).ok()?;
            let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
            std::fs::write(&temp, serde_json::to_vec(&entry).ok()?).ok()?;
            if std::fs::rename(&temp, &path).is_err() {
                let _ = std::fs::remove_file(temp);
            }
            Some(())
        })();
    })
    .await
    .ok()?;
    Some(icon)
}

#[cfg(test)]
mod tests;
